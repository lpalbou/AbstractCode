//! Entity-lane HTTP + the wake-posting thread half (turn threads, the
//! recovery loop, the conversation poller).
//!
//! Two DEDICATED ureq agents, never the shared 60s-read agent:
//! - slow lane (5s connect / 30s read): roster, card, cognition, visit
//!   status, transcript, close, task — reads that can hang behind the
//!   gateway's per-warm-home drives fold (measured 10-44s live).
//! - turn lane (5s connect / 600s read): visit turns are SYNCHRONOUS
//!   server-side (`_TURN_MAX_TICKS = 400` — minutes of model time).
//!
//! Threading contract (the engine rule, same as `runner.rs`): these
//! threads never touch signals — they post closures through a cloned
//! `WakeHandle`; the closures run on the UI thread and re-check the
//! convo/run_id/epoch guard before applying anything (EVERY state-touching
//! closure gets the guard; pure notices may skip it). Post-teardown wakes
//! are harmless no-ops by the engine contract (`term/waker.rs`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use abstracttui::reactive::WakeHandle;
use serde_json::{json, Value};

use crate::convo::{self, ConvoStatus};
use crate::entities::{
    self, close_from_response, cognition_from_response, transcript_from_response,
    turn_from_response, visit_open_from_response, visit_status_from_response,
};
use crate::gateway::{err_from_ureq, url_encode, GatewayClient, GwError, GwResult};
use crate::runner::Cmd;
use crate::store::Store;
use crate::transcript::Item;

/// Recovery poll cadence after a turn read-timeout.
const RECOVERY_POLL_S: u64 = 5;
/// Conversation poller cadence (visit status).
const POLL_VISIT_S: u64 = 7;
/// Cognition poll cadence (spend deltas) — every 4th visit poll ≈ 28s.
const POLL_COGNITION_EVERY: u64 = 4;

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct EntityClient {
    base_url: String,
    token: Option<String>,
    slow: ureq::Agent,
    turn: ureq::Agent,
}

impl EntityClient {
    pub fn new(base_url: &str, token: Option<&str>) -> EntityClient {
        let slow = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(30))
            .timeout_write(Duration::from_secs(30))
            .build();
        let turn = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(600))
            .timeout_write(Duration::from_secs(30))
            .build();
        EntityClient {
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.map(str::to_string).filter(|t| !t.is_empty()),
            slow,
            turn,
        }
    }

    pub fn from_gateway(client: &GatewayClient) -> EntityClient {
        let (base_url, token) = client.connection();
        EntityClient::new(&base_url, token.as_deref())
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/gateway{}", self.base_url, path)
    }

    fn with_auth(&self, req: ureq::Request) -> ureq::Request {
        match &self.token {
            Some(t) => req.set("Authorization", &format!("Bearer {t}")),
            None => req,
        }
    }

    fn get_json(&self, path: &str) -> GwResult<Value> {
        let req = self.with_auth(
            self.slow
                .get(&self.url(path))
                .set("Accept", "application/json"),
        );
        let resp = req.call().map_err(|e| err_from_ureq(path, e))?;
        read_json_body(path, resp)
    }

    fn post_json(&self, agent: &ureq::Agent, path: &str, payload: &Value) -> GwResult<Value> {
        let req = self.with_auth(
            agent
                .post(&self.url(path))
                .set("Accept", "application/json")
                .set("Content-Type", "application/json"),
        );
        let resp = req
            .send_string(&payload.to_string())
            .map_err(|e| err_from_ureq(path, e))?;
        read_json_body(path, resp)
    }

    // -- reads (slow lane) --------------------------------------------------

    pub fn list_entities(&self) -> GwResult<Value> {
        self.get_json("/entities")
    }

    pub fn card(&self, name: &str) -> GwResult<Value> {
        self.get_json(&format!("/entities/{}/card", url_encode(name)))
    }

    pub fn cognition(&self, name: &str) -> GwResult<Value> {
        self.get_json(&format!("/entities/{}/cognition", url_encode(name)))
    }

    pub fn visit_status(&self, name: &str) -> GwResult<Value> {
        self.get_json(&format!("/entities/{}/visit", url_encode(name)))
    }

    pub fn visit_transcript(&self, name: &str, run_id: &str) -> GwResult<Value> {
        self.get_json(&format!(
            "/entities/{}/visit/{}/transcript",
            url_encode(name),
            url_encode(run_id)
        ))
    }

    // -- visit writes ---------------------------------------------------------

    pub fn visit_open(&self, name: &str) -> GwResult<Value> {
        // Session id minted server-side; participants are DOOR-derived
        // (payload participants would be rejected as false co-presence).
        self.post_json(
            &self.slow,
            &format!("/entities/{}/visit/open", url_encode(name)),
            &json!({}),
        )
    }

    /// The 600s-read turn call. A read timeout surfaces as a transport
    /// error whose text names a timeout (see `is_read_timeout`).
    pub fn visit_turn(&self, name: &str, run_id: &str, text: &str) -> GwResult<Value> {
        self.post_json(
            &self.turn,
            &format!(
                "/entities/{}/visit/{}/turn",
                url_encode(name),
                url_encode(run_id)
            ),
            &json!({"text": text}),
        )
    }

    /// One flow-brain summon (the c5280-pinned path): POST
    /// `/entities/{name}/summon` with the `entity-chat` flow of the
    /// `entity-life` bundle. `input_data`/substrate overrides are
    /// DELIBERATELY omitted — the gateway resolves the home's stored
    /// mind (the reference implementation's adversary dropped exactly
    /// that override); `bundle_version` is omitted to ride the latest
    /// published. The client-minted `session_id` groups every summon of
    /// one conversation so continuity rides the entity's graph.
    pub fn summon(&self, name: &str, prompt: &str, session_id: &str) -> GwResult<Value> {
        // TURN lane, not slow lane (adversary P1-2): the summon can hang
        // behind the warm-home fold; a 30s start-timeout rendered as
        // "refused" while the run RAN — inviting a resend and double
        // delivery. The turn agent's long read absorbs the fold.
        // `context_window_tokens` is DELIBERATELY omitted (flow's
        // countersign, c5284, verified in gateway source): the declared
        // window SIZES the recall budget, so a hardcoded claim from a
        // client that cannot know the served model's real window would
        // inflate the budget beyond small minds (context-overflow risk).
        // Undeclared → the door's labeled #FALLBACK default; the durable
        // fix (door resolves the window from the home substrate) is the
        // gateway/entity lane's, on the record.
        self.post_json(
            &self.turn,
            &format!("/entities/{}/summon", url_encode(name)),
            &json!({
                "prompt": prompt,
                "flow_id": "entity-chat",
                "bundle_id": "entity-life",
                "session_id": session_id,
                // Seat-contract declaration (gateway c5603): /brain
                // turns are typed by a HUMAN at this keyboard, so the
                // summon declares caller_kind=human — the human-wins
                // preemption signal (a human summon preempts an
                // agent/unknown-held seat at the turn boundary).
                // Undeclared callers read as agents; pre-slice-2
                // gateways ignore the extra field harmlessly.
                "caller_kind": "human",
            }),
        )
    }

    /// Poll one run to terminal (the summon's other half). Plain
    /// `GET /runs/{id}` — status + output.
    pub fn run_status(&self, run_id: &str) -> GwResult<Value> {
        self.get_json(&format!("/runs/{}", url_encode(run_id)))
    }

    pub fn visit_close(&self, name: &str, run_id: &str, reason: &str) -> GwResult<Value> {
        // TURN lane, not slow lane: an operator close runs the entity's
        // REFLECTION segment (a real LLM call, minutes-scale) before the
        // response returns — the 30s read lane cut it off mid-reflection
        // (live gate finding, 2026-07-22).
        self.post_json(
            &self.turn,
            &format!(
                "/entities/{}/visit/{}/close",
                url_encode(name),
                url_encode(run_id)
            ),
            &json!({"closed_by": "operator", "reason": reason}),
        )
    }

    pub fn post_task(&self, name: &str, title: &str) -> GwResult<Value> {
        self.post_json(
            &self.slow,
            &format!("/entities/{}/tasks", url_encode(name)),
            &json!({"title": title}),
        )
    }
}

fn read_json_body(path: &str, resp: ureq::Response) -> GwResult<Value> {
    // The shared bounded reader (replay-integrity incident): visit
    // transcripts grow like every other durable surface — the hidden
    // ureq 10 MiB limit must never bite here either.
    let body = crate::gateway::read_body_capped_for_tests(resp, path)?;
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&body)
        .map_err(|e| GwError::transport(format!("{path}: invalid JSON: {e}")))
}

/// A transport error that reads as a TIMEOUT (the recovery-loop trigger).
/// Primary signal: the structural `GwErrorKind::Timeout` classification
/// (err_from_ureq / from_io_read read ureq's own error kinds). The text
/// match stays as a belt for shapes classification hasn't seen — on the
/// turn lane, over-matching sends us into the recovery POLL (safe),
/// while under-matching declares a running turn failed (harmful).
pub fn is_read_timeout(e: &GwError) -> bool {
    if e.status.is_some() {
        return false;
    }
    if e.kind == crate::gateway::GwErrorKind::Timeout {
        return true;
    }
    let m = e.message.to_lowercase();
    m.contains("timed out") || m.contains("timeout")
}

// ---------------------------------------------------------------------------
// Shared client instance (base_url/token are stable for the process life)
// ---------------------------------------------------------------------------

static CLIENT: OnceLock<EntityClient> = OnceLock::new();

pub fn client_for(gateway: &GatewayClient) -> EntityClient {
    CLIENT
        .get_or_init(|| EntityClient::from_gateway(gateway))
        .clone()
}

// ---------------------------------------------------------------------------
// Roster + card loads (own threads: a 30s roster fetch must never starve
// Start/Probe behind it on the runner command loop)
// ---------------------------------------------------------------------------

pub fn spawn_load_entities(client: EntityClient, wake: WakeHandle, store: Store) {
    // Panic fold: the modal's "refreshing…" must never spin forever over
    // a dead loader.
    let fold: PanicFold = Box::new(|store: Store| {
        store.entities_loading.set(false);
        store
            .entities_error
            .set("roster refresh thread died".to_string());
    });
    spawn_named(
        "entities-roster",
        wake.clone(),
        store,
        Some(fold),
        move |wake| {
            let outcome = client.list_entities();
            wake.post(move || {
                store.entities_loading.set(false);
                match outcome {
                    Ok(v) => {
                        let list = entities::entities_from_response(&v);
                        let as_of = entities::hhmm_now();
                        entities::save_cached_roster(&list, &as_of);
                        store.entities_as_of.set(as_of);
                        store.entities_error.set(String::new());
                        store.entities.set(list);
                    }
                    Err(e) => {
                        // Keep the cached roster; the error label is the honesty.
                        store.entities_error.set(e.to_string());
                    }
                }
            });
        },
    );
}

pub fn spawn_load_card(client: EntityClient, wake: WakeHandle, store: Store, name: String) {
    // No panic fold: the toast names the death; reopening /entities
    // re-requests the card (the dedup set is per-modal-open).
    spawn_named("entity-card", wake.clone(), store, None, move |wake| {
        let outcome = client.card(&name);
        wake.post(move || match outcome {
            Ok(v) => {
                let card = entities::card_from_response(&v);
                store.entity_cards.update(|cards| {
                    match cards.iter_mut().find(|(n, _)| *n == name) {
                        Some(slot) => slot.1 = card,
                        None => cards.push((name.clone(), card)),
                    }
                });
            }
            Err(e) => {
                store.notify(format!("entity card load failed: {e}"));
            }
        });
    });
}

// ---------------------------------------------------------------------------
// Visit open (adopt-on-409: status code keyed, ZERO prose matching)
// ---------------------------------------------------------------------------

pub fn spawn_open(
    client: EntityClient,
    wake: WakeHandle,
    store: Store,
    tx: Sender<Cmd>,
    name: String,
) {
    let fold: PanicFold = {
        let name = name.clone();
        Box::new(move |store: Store| {
            store.convos.update(|cs| {
                // Same structural guard as the open outcome: only a convo
                // still waiting on THIS open (no run yet) is touched.
                if let Some(ix) = cs
                    .iter()
                    .position(|c| c.name == name && c.run_id.is_empty())
                {
                    convo::fold_open_refused(
                        &mut cs[ix],
                        "the open thread died before an outcome — @name retries",
                    );
                }
            });
        })
    };
    spawn_named(
        &format!("visit-open-{name}"),
        wake.clone(),
        store,
        Some(fold),
        move |wake| {
            match client.visit_open(&name) {
                Ok(v) => {
                    let open = visit_open_from_response(&v);
                    let n = name.clone();
                    let tx2 = tx.clone();
                    wake.post(move || {
                        let mut held: Option<(String, u64, String)> = None;
                        store.convos.update(|cs| {
                            // Open outcomes guard by (name, empty run_id): the
                            // convo has no run until this lands.
                            if let Some(ix) =
                                cs.iter().position(|c| c.name == n && c.run_id.is_empty())
                            {
                                convo::fold_open_success(&mut cs[ix], &open);
                                held = take_held_for_send(cs, ix);
                            }
                        });
                        let _ = tx2.send(Cmd::PollConvos);
                        dispatch_held(store, &tx2, &n, held);
                    });
                }
                Err(e) if e.status == Some(409) => {
                    // Structured adopt: NEVER match the prose. GET /visit; when
                    // a live visit exists, adopt it from the STATUS body.
                    let adopted = client.visit_status(&name).ok().and_then(|sv| {
                        let status = visit_status_from_response(&sv);
                        if !status.open {
                            return None;
                        }
                        let transcript = client
                            .visit_transcript(&name, &status.run_id)
                            .map(|tv| transcript_from_response(&tv))
                            .unwrap_or_default();
                        Some((status, transcript))
                    });
                    let n = name.clone();
                    let detail = e.message.clone();
                    let tx2 = tx.clone();
                    wake.post(move || {
                        let mut held: Option<(String, u64, String)> = None;
                        store.convos.update(|cs| {
                            let Some(ix) =
                                cs.iter().position(|c| c.name == n && c.run_id.is_empty())
                            else {
                                return;
                            };
                            match &adopted {
                                Some((status, transcript)) => {
                                    convo::fold_adopt(&mut cs[ix], status, transcript);
                                    held = take_held_for_send(cs, ix);
                                }
                                None => {
                                    // Non-adoptable refusal (paused / grace /
                                    // hosted chat / prelude): the 409 detail
                                    // VERBATIM — never guess which case.
                                    convo::fold_open_refused(&mut cs[ix], &detail);
                                }
                            }
                        });
                        if adopted.is_some() {
                            let _ = tx2.send(Cmd::PollConvos);
                        }
                        dispatch_held(store, &tx2, &n, held);
                    });
                }
                Err(e) => {
                    let n = name.clone();
                    let msg = e.to_string();
                    wake.post(move || {
                        store.convos.update(|cs| {
                            if let Some(ix) =
                                cs.iter().position(|c| c.name == n && c.run_id.is_empty())
                            {
                                convo::fold_open_refused(
                                    &mut cs[ix],
                                    &format!("open failed: {msg}"),
                                );
                            }
                        });
                    });
                }
            }
        },
    );
}

// ---------------------------------------------------------------------------
// Per-turn thread + recovery loop
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn spawn_turn(
    client: EntityClient,
    wake: WakeHandle,
    store: Store,
    tx: Sender<Cmd>,
    name: String,
    run_id: String,
    epoch: u64,
    text: String,
) {
    let fold: PanicFold = {
        let (name, run_id) = (name.clone(), run_id.clone());
        Box::new(move |store: Store| {
            store.convos.update(|cs| {
                if let Some(ix) = convo::guard(cs, &name, &run_id, epoch) {
                    convo::fold_turn_transport_error(
                        &mut cs[ix],
                        "the turn thread died — the turn may still be running server-side; \
                         the next message adopts whatever state the visit reached",
                    );
                }
            });
        })
    };
    spawn_named(
        &format!("visit-turn-{name}"),
        wake.clone(),
        store,
        Some(fold),
        move |wake| {
            match client.visit_turn(&name, &run_id, &text) {
                Ok(v) => {
                    let resp = turn_from_response(&v);
                    post_turn_outcome(wake, store, &tx, &name, &run_id, epoch, resp);
                }
                Err(e) if is_read_timeout(&e) => {
                    // The turn is still executing server-side. Announce, then
                    // recover ON THIS THREAD: poll /visit until the run parks,
                    // then diff the transcript by turn_n.
                    {
                        let (n, r) = (name.clone(), run_id.clone());
                        wake.post(move || {
                            store.convos.update(|cs| {
                                if let Some(ix) = convo::guard(cs, &n, &r, epoch) {
                                    convo::fold_timeout_notice(&mut cs[ix]);
                                }
                            });
                        });
                    }
                    recovery_loop(&client, &wake, store, &tx, &name, &run_id, epoch);
                }
                Err(e) => {
                    let (n, r) = (name.clone(), run_id.clone());
                    let msg = e.to_string();
                    wake.post(move || {
                        store.convos.update(|cs| {
                            if let Some(ix) = convo::guard(cs, &n, &r, epoch) {
                                convo::fold_turn_transport_error(&mut cs[ix], &msg);
                            }
                        });
                    });
                }
            }
        },
    );
}

/// UI-thread half of a held-draft auto-send at a park boundary: fold the
/// send into the convo (user card, TurnRunning, epoch bump) and return
/// (run_id, epoch, text) for the caller to dispatch AFTER the update.
/// Every park path uses this — the hold banner promises "sends when the
/// turn parks", and a promise the machinery skips is a stranded draft
/// (cycle-2 review: open-success, adopt, and recovery parks all stranded).
fn take_held_for_send(
    cs: &mut [crate::convo::EntityConvo],
    ix: usize,
) -> Option<(String, u64, String)> {
    let draft = convo::take_held_draft(&mut cs[ix])?;
    let epoch = convo::fold_send_turn(&mut cs[ix], &draft);
    Some((cs[ix].run_id.clone(), epoch, draft))
}

/// Dispatch the auto-send prepared by `take_held_for_send` (outside the
/// convos update: the command send must never run inside the signal write).
fn dispatch_held(store: Store, tx: &Sender<Cmd>, name: &str, held: Option<(String, u64, String)>) {
    let Some((run_id, epoch, text)) = held else {
        return;
    };
    store.notify(format!("held draft sent to {name}"));
    let _ = tx.send(Cmd::EntityTurn {
        name: name.to_string(),
        run_id,
        epoch,
        text,
    });
}

/// Post a turn response fold + the held-draft auto-send (the ruled v1
/// between-turns steering: the UI-thread closure folds the reply, takes
/// the held draft, bumps the epoch, and sends the next turn command).
fn post_turn_outcome(
    wake: WakeHandle,
    store: Store,
    tx: &Sender<Cmd>,
    name: &str,
    run_id: &str,
    epoch: u64,
    resp: crate::entities::TurnResponse,
) {
    let (n, r) = (name.to_string(), run_id.to_string());
    let tx = tx.clone();
    wake.post(move || {
        let mut held: Option<(String, u64, String)> = None;
        store.convos.update(|cs| {
            let Some(ix) = convo::guard(cs, &n, &r, epoch) else {
                return; // stale result from an abandoned/ended convo
            };
            if let Some(draft) = convo::fold_turn_reply(&mut cs[ix], &resp) {
                let next_epoch = convo::fold_send_turn(&mut cs[ix], &draft);
                held = Some((cs[ix].run_id.clone(), next_epoch, draft));
            }
        });
        dispatch_held(store, &tx, &n, held);
    });
}

/// The same-thread recovery loop after a turn read-timeout: `GET /visit`
/// every 5s; parked (`status == "waiting"`) → transcript diff by turn_n;
/// `open: false` (idle-close or failure raced us) → final transcript →
/// Closed with the last words rendered. A recovered park auto-sends the
/// held draft exactly like a normal turn completion.
fn recovery_loop(
    client: &EntityClient,
    wake: &WakeHandle,
    store: Store,
    tx: &Sender<Cmd>,
    name: &str,
    run_id: &str,
    epoch: u64,
) {
    loop {
        std::thread::sleep(Duration::from_secs(RECOVERY_POLL_S));
        let status = match client.visit_status(name) {
            Ok(v) => visit_status_from_response(&v),
            Err(_) => continue, // transient; the visit is durable server-side
        };
        let ours = status.open && status.run_id == run_id;
        if ours && status.status != "waiting" {
            continue; // still running the turn
        }
        // Parked, or the visit is gone/replaced: one transcript read tells
        // the rest (the transcript endpoint works on terminal runs too).
        let transcript = match client.visit_transcript(name, run_id) {
            Ok(v) => transcript_from_response(&v),
            Err(e) => {
                let (n, r) = (name.to_string(), run_id.to_string());
                let msg = e.to_string();
                wake.post(move || {
                    store.convos.update(|cs| {
                        if let Some(ix) = convo::guard(cs, &n, &r, epoch) {
                            cs[ix].items.push(Item::Error {
                                text: format!("turn recovery could not read the transcript: {msg}"),
                            });
                            cs[ix].status = ConvoStatus::Parked;
                            cs[ix].turn_started = None;
                            // Recovery exit path: release the poller skip.
                            cs[ix].recovery_owned = false;
                        }
                    });
                });
                return;
            }
        };
        let (n, r) = (name.to_string(), run_id.to_string());
        let tx = tx.clone();
        wake.post(move || {
            let mut held: Option<(String, u64, String)> = None;
            store.convos.update(|cs| {
                let Some(ix) = convo::guard(cs, &n, &r, epoch) else {
                    return;
                };
                if ours {
                    if let Some(draft) = convo::fold_recovery_parked(&mut cs[ix], &transcript) {
                        let next_epoch = convo::fold_send_turn(&mut cs[ix], &draft);
                        held = Some((cs[ix].run_id.clone(), next_epoch, draft));
                    }
                } else {
                    convo::fold_recovery_closed(&mut cs[ix], &transcript);
                }
            });
            dispatch_held(store, &tx, &n, held);
        });
        return;
    }
}

// ---------------------------------------------------------------------------
// Close + task
// ---------------------------------------------------------------------------

pub fn spawn_close(
    client: EntityClient,
    wake: WakeHandle,
    store: Store,
    name: String,
    run_id: String,
    epoch: u64,
    reason: String,
) {
    let fold: PanicFold = {
        let (name, run_id) = (name.clone(), run_id.clone());
        Box::new(move |store: Store| {
            store.convos.update(|cs| {
                if let Some(ix) = convo::guard(cs, &name, &run_id, epoch) {
                    cs[ix].items.push(Item::Error {
                        text: "the close thread died — the visit may still be open; /end retries"
                            .into(),
                    });
                }
            });
        })
    };
    spawn_named(
        &format!("visit-close-{name}"),
        wake.clone(),
        store,
        Some(fold),
        move |wake| {
            let outcome = client.visit_close(&name, &run_id, &reason);
            let (n, r) = (name.clone(), run_id.clone());
            wake.post(move || match outcome {
                Ok(v) => {
                    let resp = close_from_response(&v);
                    store.convos.update(|cs| {
                        if let Some(ix) = convo::guard(cs, &n, &r, epoch) {
                            convo::fold_close(&mut cs[ix], &resp);
                        }
                    });
                }
                Err(e) => {
                    store.notify(format!("/end failed: {e}"));
                    store.convos.update(|cs| {
                        if let Some(ix) = convo::guard(cs, &n, &r, epoch) {
                            cs[ix].items.push(Item::Error {
                                text: format!("close failed: {e} — the visit stays open"),
                            });
                        }
                    });
                }
            });
        },
    );
}

/// VERBATIM confirmation copy (entity seat, commons 4312; adopted by the
/// plan): engagement is boundary-scale, never mid-turn — no "immediately",
/// no "notified", no minutes estimate (the wake cadence is a server dial
/// clients must not shadow). Character-exact; pinned by test.
pub const TASK_CONFIRMATION_COPY: &str =
    "Recorded on his desk — he takes it up at his next boundary: day end, wake check, or visit close.";

pub fn spawn_task(
    client: EntityClient,
    wake: WakeHandle,
    store: Store,
    name: String,
    title: String,
) {
    spawn_named(
        &format!("entity-task-{name}"),
        wake.clone(),
        store,
        // No panic fold: nothing was optimistically flipped for a task;
        // the death toast is the whole truth ("nothing recorded").
        None,
        move |wake| {
            let outcome = client.post_task(&name, &title);
            let n = name.clone();
            wake.post(move || match outcome {
                Ok(v) => {
                    let pending = v.get("pending").and_then(Value::as_u64);
                    let mut text = format!("{n}: {TASK_CONFIRMATION_COPY}");
                    if let Some(p) = pending {
                        text.push_str(&format!(" ({p} pending)"));
                    }
                    store.notify(text.clone());
                    // Deliberately find-by-name, NOT the epoch guard: a task
                    // confirmation is visit-independent (the desk outlives
                    // every visit), so it may annotate whatever conversation
                    // exists with this entity — even one that closed or
                    // reopened while the POST was in flight.
                    store.convos.update(|cs| {
                        if let Some(ix) = convo::find(cs, &n) {
                            cs[ix].items.push(Item::Info { text: text.clone() });
                        }
                    });
                }
                Err(e) => store.notify(format!("/task failed: {e}")),
            });
        },
    );
}

// ---------------------------------------------------------------------------
// Conversation poller (ONE sequential thread; zero polling with none open)
// ---------------------------------------------------------------------------

/// What the poller may see (it cannot read signals): open conversations +
/// a stop flag. The UI thread rewrites it whenever `store.convos` changes.
#[derive(Debug, Default)]
pub struct PollerView {
    pub stop: bool,
    /// (name, run_id, epoch) of every OPEN conversation.
    pub open: Vec<(String, String, u64)>,
}

static POLLER_VIEW: OnceLock<Arc<Mutex<PollerView>>> = OnceLock::new();
static POLLER_STARTED: AtomicBool = AtomicBool::new(false);

pub fn poller_view() -> Arc<Mutex<PollerView>> {
    POLLER_VIEW
        .get_or_init(|| Arc::new(Mutex::new(PollerView::default())))
        .clone()
}

/// UI-thread sync: mirror the open conversations into the poller view.
/// A recovery-latched conversation is SKIPPED: the turn-recovery loop
/// already polls that run at 5s, so the 7s poller polling it too was a
/// benign double-poll (cycle-2 leftover). The skip requires BOTH the
/// latch and TurnRunning — a stuck latch on a parked conversation (a
/// clearing bug) must never starve idle-close detection.
pub fn sync_poller_view(convos: &[crate::convo::EntityConvo]) {
    let open: Vec<(String, String, u64)> = convos
        .iter()
        .filter(|c| {
            // FLOW-BRAIN conversations have no visit run to poll — each
            // summon completes and its own turn thread polls it; the
            // convo's run_id is a COMPLETED run (polling /visit against
            // it would be nonsense).
            c.brain == crate::convo::Brain::Visit
                && !c.run_id.is_empty()
                && matches!(
                    c.status,
                    ConvoStatus::Ready | ConvoStatus::Parked | ConvoStatus::TurnRunning
                )
                && !(c.recovery_owned && c.status == ConvoStatus::TurnRunning)
        })
        .map(|c| (c.name.clone(), c.run_id.clone(), c.turn_epoch))
        .collect();
    if let Ok(mut view) = poller_view().lock() {
        view.open = open;
    }
}

pub fn stop_poller() {
    if let Ok(mut view) = poller_view().lock() {
        view.stop = true;
    }
}

/// Spawn the ONE poller thread (idempotent). Sequential by design: the
/// gateway constructs cold homes inside a global lock, so overlapping
/// polls are actively harmful — natural backpressure instead.
pub fn ensure_poller(client: EntityClient, wake: WakeHandle, store: Store) {
    if POLLER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let view = poller_view();
    // Panic fold: clear the started latch so the NEXT open respawns the
    // poller (a dead poller with the latch stuck meant no idle-close
    // detection for the rest of the session).
    let fold: PanicFold = Box::new(|_store: Store| {
        POLLER_STARTED.store(false, Ordering::SeqCst);
    });
    spawn_named(
        "entity-poller",
        wake.clone(),
        store,
        Some(fold),
        move |wake| {
            let mut tick: u64 = 0;
            loop {
                std::thread::sleep(Duration::from_secs(POLL_VISIT_S));
                tick = tick.wrapping_add(1);
                let snapshot: Vec<(String, String, u64)> = {
                    let Ok(view) = view.lock() else { break };
                    if view.stop {
                        break;
                    }
                    view.open.clone()
                };
                if snapshot.is_empty() {
                    continue; // zero polling with none open
                }
                for (name, run_id, epoch) in &snapshot {
                    // Visit status: detect server-side closes under a PARKED
                    // conversation (the reaper's idle close; TurnRunning is
                    // the turn thread's/recovery loop's business).
                    if let Ok(v) = client.visit_status(name) {
                        let status = visit_status_from_response(&v);
                        let gone = !status.open || status.run_id != *run_id;
                        if gone {
                            // Learn WHY before wording the close: the OLD
                            // run's transcript serves on terminal runs and
                            // names its status (completed/failed/cancelled).
                            // A failed read leaves it unknown ("") — the
                            // fold words each state honestly and applies
                            // nothing on a live status (transient misread).
                            let observed = client
                                .visit_transcript(name, run_id)
                                .map(|tv| transcript_from_response(&tv).status)
                                .unwrap_or_default();
                            let (n, r, e) = (name.clone(), run_id.clone(), *epoch);
                            wake.post(move || {
                                store.convos.update(|cs| {
                                    if let Some(ix) = convo::guard(cs, &n, &r, e) {
                                        convo::fold_poll_closed(&mut cs[ix], &observed);
                                    }
                                });
                            });
                            continue;
                        }
                    }
                    // Spend deltas from /cognition — the ONLY honest token
                    // source (never fabricated) — at a slower cadence.
                    if tick.is_multiple_of(POLL_COGNITION_EVERY) {
                        if let Ok(v) = client.cognition(name) {
                            let cog = cognition_from_response(&v);
                            let (n, r, e) = (name.clone(), run_id.clone(), *epoch);
                            wake.post(move || {
                                store.convos.update(|cs| {
                                    if let Some(ix) = convo::guard(cs, &n, &r, e) {
                                        convo::fold_poll_cognition(
                                            &mut cs[ix],
                                            &cog.state,
                                            cog.spend.live_visit_tokens,
                                        );
                                    }
                                });
                            });
                        }
                    }
                }
            }
        },
    );
}

// ---------------------------------------------------------------------------
// Thread spawn with panic surfacing (mirror of runner::spawn_stream)
// ---------------------------------------------------------------------------

/// UI-thread fold a dying thread posts so its conversation never absorbs
/// in Opening/TurnRunning (cycle-2 review: a panicked turn thread left
/// TurnRunning forever — /end refused, chip spinning, no recovery). Runs
/// on the UI thread with full store access; the fold must guard itself
/// (convo/run/epoch or the open-outcome shape) before touching state.
type PanicFold = Box<dyn FnOnce(Store) + Send>;

/// One flow-brain turn on its own thread: summon → poll to terminal →
/// guarded fold. The poll pace is 2s to a 300s bound (the reference
/// implementation's bound; uncontended turns land in ~10-20s, contended
/// ones ran 1-8 min there — the bound is honesty's edge, and hitting it
/// folds a truthful still-running line, never a fake failure). Every
/// posted closure re-checks `guard_flow(name, epoch)` — a send/end after
/// this thread started makes its posts apply NOTHING.
#[allow(clippy::too_many_arguments)]
pub fn spawn_flow_turn(
    client: EntityClient,
    wake: WakeHandle,
    store: Store,
    tx: Sender<Cmd>,
    name: String,
    session_id: String,
    epoch: u64,
    text: String,
) {
    let fold: PanicFold = {
        let (name, sid) = (name.clone(), session_id.clone());
        Box::new(move |store: Store| {
            store.convos.update(|cs| {
                if let Some(ix) = convo::guard_flow(cs, &name, &sid, epoch) {
                    convo::fold_flow_failure(
                        &mut cs[ix],
                        "the summon thread died — the entity's memory of anything already \
                         formed persists; send again",
                    );
                }
            });
        })
    };
    spawn_named(
        &format!("flow-turn-{name}"),
        wake.clone(),
        store,
        Some(fold),
        move |wake| {
            let post_failure = |wake: &WakeHandle, msg: String| {
                let (n, sid) = (name.clone(), session_id.clone());
                wake.post(move || {
                    store.convos.update(|cs| {
                        if let Some(ix) = convo::guard_flow(cs, &n, &sid, epoch) {
                            convo::fold_flow_failure(&mut cs[ix], &msg);
                        }
                    });
                });
            };
            let run_id = match client.summon(&name, &text, &session_id) {
                Ok(v) => {
                    let rid = v
                        .get("run_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if rid.is_empty() {
                        post_failure(
                            &wake,
                            "the summon returned no run id — see the gateway log".to_string(),
                        );
                        return;
                    }
                    rid
                }
                Err(e) => {
                    // Transport death after the POST left is OUTCOME
                    // UNKNOWN — the summon may have started and the turn
                    // then completes server-side, forming memory. Say
                    // that; a "refused" here invited double-sends
                    // (adversary P1-2).
                    post_failure(
                        &wake,
                        format!(
                            "summon outcome unknown ({e}) — if it started, the turn \
                             completes server-side; wait a moment before resending"
                        ),
                    );
                    return;
                }
            };
            // Poll to terminal. Transient poll errors don't kill the turn
            // (the run is durable server-side); the BOUND does, honestly.
            let deadline = std::time::Instant::now() + Duration::from_secs(300);
            loop {
                std::thread::sleep(Duration::from_secs(2));
                if std::time::Instant::now() > deadline {
                    post_failure(
                        &wake,
                        format!(
                            "the summon is still running after 300s (run {}) — if the turn \
                             is executing, it completes server-side and its memory forms \
                             there; wait a moment before resending",
                            run_id.get(..8).unwrap_or(&run_id)
                        ),
                    );
                    return;
                }
                let v = match client.run_status(&run_id) {
                    Ok(v) => v,
                    Err(_) => continue, // transient; the deadline bounds us
                };
                let status = v.get("status").and_then(Value::as_str).unwrap_or("");
                if status == "running" || status == "waiting" || status.is_empty() {
                    continue;
                }
                // Terminal: read the STRUCTURED contract — answer +
                // degraded + moment_error, never success alone (the
                // c5280-pinned read rule).
                let (answer, degraded, moment_error) = parse_summon_output(&v);
                if status != "completed" {
                    let err = v
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    post_failure(
                        &wake,
                        format!(
                            "the summon ended {status}{}",
                            if err.is_empty() {
                                String::new()
                            } else {
                                format!(": {err}")
                            }
                        ),
                    );
                    return;
                }
                let n = name.clone();
                let rid = run_id.clone();
                let sid = session_id.clone();
                let tx = tx.clone();
                wake.post(move || {
                    let mut held: Option<(String, u64, String)> = None;
                    store.convos.update(|cs| {
                        if let Some(ix) = convo::guard_flow(cs, &n, &sid, epoch) {
                            if let Some(draft) = convo::fold_flow_reply(
                                &mut cs[ix],
                                &rid,
                                &answer,
                                degraded,
                                &moment_error,
                            ) {
                                let next = convo::fold_send_turn(&mut cs[ix], &draft);
                                // The auto-send's session id comes from the
                                // CONVO under the guard, never the thread's
                                // capture (adversary P0-1: a captured sid
                                // could ride a held draft into a REPLACED
                                // conversation's thread).
                                held = Some((cs[ix].session_id.clone(), next, draft));
                            }
                        }
                    });
                    if let Some((convo_sid, next_epoch, draft)) = held {
                        let _ = tx.send(Cmd::EntityFlowTurn {
                            name: n,
                            session_id: convo_sid,
                            epoch: next_epoch,
                            text: draft,
                        });
                    }
                });
                return;
            }
        },
    );
}

/// The summon run's terminal output, read by the STRUCTURED contract
/// (answer + degraded + moment_error — never success alone; the
/// c5280-pinned rule). Pure so the parse is testable outside the thread.
pub fn parse_summon_output(v: &Value) -> (String, i64, String) {
    let out = v.get("output").cloned().unwrap_or(Value::Null);
    let answer = out
        .get("answer")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let degraded = out.get("degraded").and_then(Value::as_i64).unwrap_or(0);
    let moment_error = out
        .get("moment_error")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    (answer, degraded, moment_error)
}

fn spawn_named(
    name: &str,
    wake: WakeHandle,
    store: Store,
    panic_fold: Option<PanicFold>,
    body: impl FnOnce(WakeHandle) + Send + 'static,
) {
    let thread_name = name.to_string();
    let panic_wake = wake.clone();
    let label = thread_name.clone();
    let _ = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                body(wake);
            }));
            if let Err(payload) = result {
                let msg = crate::runner::panic_text(payload.as_ref());
                // A dead thread must say so, never freeze silently — and
                // its conversation must land in a state the user can act
                // on (the fold is guarded; a stale one applies nothing).
                panic_wake.post(move || {
                    store.notify(format!("{label} thread died: {msg}"));
                    if let Some(fold) = panic_fold {
                        fold(store);
                    }
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_timeout_detection_is_transport_only() {
        // Structural kind is the primary signal…
        assert!(is_read_timeout(&GwError::timeout(
            "turn: Network Error: timed out reading response"
        )));
        // …and the text belt still catches timeout-worded transport
        // shapes classification hasn't seen (recovery-poll is the safe
        // failure mode on the turn lane).
        assert!(is_read_timeout(&GwError::transport("Connection timeout")));
        // An HTTP answer is never a read timeout, whatever the words.
        assert!(!is_read_timeout(&GwError::http(
            409,
            "Run is not waiting (timed out server-side)"
        )));
        // A refused connect is gone-evidence, not a slow turn.
        assert!(!is_read_timeout(&GwError::unreachable(
            "connection refused"
        )));
    }

    #[test]
    fn task_confirmation_copy_is_character_exact() {
        // ADOPTED VERBATIM from the entity seat's answer (commons 4312).
        // Never "immediately", never "notified", never a minutes estimate.
        assert_eq!(
            TASK_CONFIRMATION_COPY,
            "Recorded on his desk — he takes it up at his next boundary: \
             day end, wake check, or visit close."
        );
        assert!(!TASK_CONFIRMATION_COPY.contains("immediately"));
        assert!(!TASK_CONFIRMATION_COPY.contains("notified"));
    }

    #[test]
    fn held_draft_auto_send_wiring_folds_and_dispatches() {
        // The UI-thread half every park path shares: take the held draft,
        // fold the send (user card + TurnRunning + epoch bump), dispatch
        // Cmd::EntityTurn with the FOLDED epoch — pinned here because three
        // park paths (open success, adopt, recovery) stranded held drafts
        // in cycle 1 (the hold banner promised an auto-send that never ran).
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
            let mut convos = vec![crate::convo::EntityConvo::opening("doorcheck", "awake")];
            crate::convo::fold_open_success(&mut convos[0], &crate::entities::VisitOpen::default());
            convos[0].run_id = "r1".into();
            crate::convo::hold_draft(&mut convos[0], "held words");
            let held = take_held_for_send(&mut convos, 0);
            let (run_id, epoch, text) = held.clone().expect("draft taken");
            assert_eq!(run_id, "r1");
            assert_eq!(text, "held words");
            assert_eq!(convos[0].status, ConvoStatus::TurnRunning);
            assert_eq!(
                convos[0].turn_epoch, epoch,
                "command carries the folded epoch"
            );
            assert!(convos[0].held_draft.is_empty());
            assert!(convos[0]
                .items
                .iter()
                .any(|i| matches!(i, Item::User { text } if text == "held words")));
            dispatch_held(store, &tx, "doorcheck", held);
            match rx.try_recv().expect("EntityTurn dispatched") {
                Cmd::EntityTurn {
                    name,
                    run_id,
                    epoch: e,
                    text,
                } => {
                    assert_eq!(name, "doorcheck");
                    assert_eq!(run_id, "r1");
                    assert_eq!(e, epoch);
                    assert_eq!(text, "held words");
                }
                other => panic!("wrong command: {other:?}"),
            }
            assert!(store
                .notices
                .get_untracked()
                .iter()
                .any(|n| n.contains("held draft sent")));
            // Empty hold: nothing dispatched, nothing folded.
            assert!(take_held_for_send(&mut convos, 0).is_none());
            dispatch_held(store, &tx, "doorcheck", None);
            assert!(rx.try_recv().is_err());
        });
        root.dispose();
    }

    #[test]
    fn panic_fold_travels_the_wake_queue_end_to_end() {
        // The REAL panic path, end to end: a `spawn_named` thread dies,
        // `catch_unwind` posts the notice + fold through the WakeHandle,
        // and the UI-thread drain applies both. `drain_posted()` is
        // EXACTLY what `Driver::turn()` runs first every frame (engine
        // app/driver.rs), so this pump is the headless twin of the
        // production loop. This lives at the UNIT boundary deliberately:
        // the integration harness cannot reach `spawn_named` with a
        // panicking body (private fn; no public entry takes an injectable
        // body) — see the note in tests/headless_ui.rs.
        //
        // `resume_unwind` instead of `panic!`: same unwind payload
        // through the same catch, WITHOUT invoking the global panic hook
        // (no stderr noise into parallel test output).
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            let wake = abstracttui::reactive::wake_handle();
            let fold: PanicFold = Box::new(|store: Store| {
                store.entities_error.set("fold applied".into());
            });
            spawn_named("panic-probe", wake, store, Some(fold), |_wake| {
                std::panic::resume_unwind(Box::new("deliberate probe panic".to_string()));
            });
            // Bounded pump: the thread needs a moment to die and post —
            // drain, check, sleep; never an unbounded wait.
            let mut applied = false;
            for _ in 0..500 {
                abstracttui::reactive::drain_posted();
                if store.entities_error.get_untracked() == "fold applied" {
                    applied = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(applied, "the panic fold landed through the wake queue");
            assert!(
                store.notices.get_untracked().iter().any(|n| {
                    n.contains("panic-probe thread died") && n.contains("deliberate probe panic")
                }),
                "the death notice names the thread and the payload: {:?}",
                store.notices.get_untracked()
            );
        });
        root.dispose();
    }

    #[test]
    fn poller_view_sync_keeps_open_convos_only() {
        // ONE test owns the process-global poller view (a second test
        // syncing it in parallel would race this one's reads) — the
        // recovery-latch cases live here too, sequentially.
        let mut convos = vec![crate::convo::EntityConvo::opening("castor", "awake")];
        crate::convo::fold_open_success(&mut convos[0], &crate::entities::VisitOpen::default());
        convos[0].run_id = "r1".into();
        let mut closed = crate::convo::EntityConvo::opening("hypnos", "awake");
        closed.run_id = "r2".into();
        closed.status = ConvoStatus::Closed;
        convos.push(closed);
        convos.push(crate::convo::EntityConvo::opening("ghost", "awake")); // no run yet
        sync_poller_view(&convos);
        {
            let view = poller_view();
            let v = view.lock().unwrap();
            assert_eq!(v.open.len(), 1);
            assert_eq!(v.open[0].0, "castor");
            assert_eq!(v.open[0].1, "r1");
        }
        // Recovery latch: while the recovery loop owns the run (timeout →
        // TurnRunning + latch), the poller view SKIPS it (task-1 fix: the
        // 5s recovery poll and the 7s poller double-polled one run).
        crate::convo::fold_send_turn(&mut convos[0], "x");
        crate::convo::fold_timeout_notice(&mut convos[0]);
        assert!(convos[0].recovery_owned);
        sync_poller_view(&convos);
        {
            let view = poller_view();
            let v = view.lock().unwrap();
            assert!(
                v.open.is_empty(),
                "a recovery-latched convo is not polled: {:?}",
                v.open
            );
        }
        // Belt: a STUCK latch on a parked conversation never starves
        // idle-close detection — the skip requires TurnRunning too.
        convos[0].status = ConvoStatus::Parked;
        convos[0].turn_started = None;
        sync_poller_view(&convos);
        {
            let view = poller_view();
            let v = view.lock().unwrap();
            assert_eq!(v.open.len(), 1, "parked + stuck latch still polls");
        }
        // Latch released (recovery exit): the running convo polls again.
        convos[0].status = ConvoStatus::TurnRunning;
        convos[0].recovery_owned = false;
        sync_poller_view(&convos);
        {
            let view = poller_view();
            let v = view.lock().unwrap();
            assert_eq!(v.open.len(), 1);
        }
    }
}
