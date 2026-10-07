//! The settings rail's and the conversations board's gateway half (R7.3):
//! one short-lived thread per action, results posted to `store.rail` /
//! `store.session_index` / `store.automations` through the wake handle
//! (worker threads never touch signals).
//!
//! Routes — the SAME ones the Code web calls, nothing TUI-only:
//! - `GET /workspace/policy` (Workspace panel; the Voice panel reads the
//!   voice module's `store.voice`, the one `GET /voice/defaults` reader);
//! - `GET /runs/{id}/ledger` (an automation run's Activity groups);
//! - `GET /runs?root_only=true&archived_only=true&…` (the `Archived · N` rows);
//! - `POST /sessions/{id}/archive|unarchive` (Archive / Unarchive a conversation);
//! - `PATCH /automations/{id}` with `expected_revision` (a settings change
//!   saved as a new revision), then `GET /automations/{id}` (the new revision).

use abstracttui::reactive::WakeHandle;
use serde_json::Value;

use crate::automations as auto;
use crate::gateway::automations::AutomationClient;
use crate::gateway::{GatewayClient, GwError};

/// The gateway's own sentence when it gave one, else the evidence class.
fn reason(e: &GwError) -> String {
    if e.message.trim().is_empty() {
        e.compact_reason()
    } else {
        e.message.trim().to_string()
    }
}
use crate::rail::SaveState;
use crate::store::{SessionIndex, Store};
use crate::transcript::{Fold, FoldEffect, Item};

/// One command of the rail lane (sent through `Cmd::Rail`).
#[derive(Debug, Clone)]
pub enum RailCmd {
    /// `GET /workspace/policy`.
    LoadPanels,
    /// An automation run's ledger, folded into transcript items.
    LoadRunActivity { run_id: String },
    /// The archived sessions (rows under `Archived · N`).
    LoadArchived,
    /// Archive (`archive: true`) or unarchive one conversation; `next` is
    /// the conversation to open when the archived one was open here.
    SetArchived {
        session_id: String,
        archive: bool,
        next: Option<String>,
    },
    /// Save one settings change of an automation as a new revision.
    SaveRevision {
        id: String,
        command_id: String,
        expected_revision: u64,
        changes: Value,
    },
}

/// Ledger records read for one automation run's Activity (the web reads
/// at most 2000 the same way).
const RUN_LEDGER_CAP: usize = 2000;
const ARCHIVED_LIMIT: u32 = 200;
/// Prompts fetched for archived rows (one request each, bounded).
const ARCHIVED_PROMPTS: usize = 40;

pub fn spawn(client: &GatewayClient, wake: WakeHandle, store: Store, cmd: RailCmd) {
    let client = client.clone();
    let post = wake.clone();
    crate::runner::spawn_host_thread("rail", wake, store, move || run(&client, &post, store, cmd));
}

fn run(client: &GatewayClient, wake: &WakeHandle, store: Store, cmd: RailCmd) {
    match cmd {
        RailCmd::LoadPanels => {
            // The voice routes are NOT read here: `ui::voice_view::load_defaults`
            // is the one `GET /voice/defaults` reader (R7-W3's voice module).
            let policy = client.get_json("/workspace/policy").map_err(|e| reason(&e));
            wake.post(move || store.rail.update(|r| r.policy = Some(policy)));
        }
        RailCmd::LoadRunActivity { run_id } => {
            let out = run_items(client, &run_id);
            wake.post(move || {
                store.rail.update(|r| {
                    r.run_activity.retain(|(id, _)| id != &run_id);
                    r.run_activity.push((run_id, Some(out)));
                })
            });
        }
        RailCmd::LoadArchived => load_archived(client, wake, store),
        RailCmd::SetArchived {
            session_id,
            archive,
            next,
        } => {
            let out = client.set_session_archived(&session_id, archive);
            let ok = out.is_ok();
            wake.post(move || {
                store.rail.update(|r| {
                    r.archive_busy = false;
                    match out {
                        Ok(_) => {
                            r.board_error.clear();
                            if archive {
                                r.handover = Some((session_id, next));
                            }
                        }
                        Err(e) => {
                            let verb = if archive {
                                "Not archived"
                            } else {
                                "Not unarchived"
                            };
                            r.board_error = format!("{verb}: {}", reason(&e));
                        }
                    }
                })
            });
            if ok {
                // Both lists move: re-read them (the gateway's counts).
                crate::runner::spawn_load_sessions(
                    client.clone(),
                    wake.clone(),
                    store,
                    crate::ui::modals::SESSION_LIST_LIMIT,
                );
                load_archived(client, wake, store);
            }
        }
        RailCmd::SaveRevision {
            id,
            command_id,
            expected_revision,
            changes,
        } => {
            let _ = save_revision_now(
                client,
                wake,
                store,
                &id,
                &command_id,
                expected_revision,
                changes,
            );
        }
    }
}

/// Save one settings change of an automation as a new revision (`PATCH
/// /automations/{id}` with `expected_revision`), post the revision line,
/// then re-read the definition (the web's +1.5 s / +4 s re-reads). Blocking:
/// call it on a lane thread. `Err` = the sentence shown.
pub fn save_revision_now(
    client: &GatewayClient,
    wake: &WakeHandle,
    store: Store,
    id: &str,
    command_id: &str,
    expected_revision: u64,
    changes: Value,
) -> Result<(), String> {
    let auto_client = AutomationClient::from_gateway(client);
    let out = auto_client.send(&auto::revise_request(
        id,
        command_id,
        Some(expected_revision),
        changes,
    ));
    let state = match &out {
        Ok(v) => SaveState::Saved(
            v.pointer("/definition/revision")
                .or_else(|| v.pointer("/summary/revision"))
                .and_then(Value::as_u64)
                .unwrap_or(expected_revision + 1),
        ),
        Err(e) if e.code == "revision_conflict" => SaveState::Conflict,
        Err(e) => SaveState::Refused(auto::api_error_text(e)),
    };
    let result = match &state {
        SaveState::Saved(_) => Ok(()),
        SaveState::Refused(why) => Err(why.clone()),
        SaveState::Conflict => Err(
            "The automation changed elsewhere. The latest revision is shown; make the change again.".into(),
        ),
        _ => Err(crate::rail::save_line(&state)),
    };
    let transport = matches!(&out, Err(e) if e.is_transport());
    wake.post(move || {
        store.rail.update(|r| r.save = state);
        store.automations.update(|v| v.ids.settle(transport));
    });
    // The latest definition either way (a conflict shows the revision
    // someone else saved). The gateway applies a revision moments after
    // accepting it, so re-read now and twice more.
    for pause in [0u64, 1500, 2500, 4000] {
        std::thread::sleep(std::time::Duration::from_millis(pause));
        let detail = auto_client.detail(id);
        let id = id.to_string();
        wake.post(move || {
            store.automations.update(|v| {
                if let Ok((def, summary, page)) = detail {
                    v.apply_detail(&id, def, summary, page);
                }
            })
        });
    }
    result
}

/// Fold a run's ledger (and its sub-runs', as the live transcript does)
/// into transcript items.
fn run_items(client: &GatewayClient, run_id: &str) -> Result<Vec<Item>, String> {
    let mut fold = Fold::new();
    fold.begin_run(run_id);
    let mut queue = vec![run_id.to_string()];
    let mut seen = std::collections::HashSet::new();
    let mut read = 0usize;
    while let Some(rid) = queue.pop() {
        if !seen.insert(rid.clone()) || read >= RUN_LEDGER_CAP {
            continue;
        }
        let mut after = 0u64;
        loop {
            let (items, next) = client
                .get_ledger(&rid, after, 500)
                .map_err(|e| reason(&e))?;
            if items.is_empty() {
                break;
            }
            for rec in &items {
                for eff in fold.apply(&rid, rec) {
                    if let FoldEffect::FollowRun(sub) = eff {
                        queue.push(sub);
                    }
                }
            }
            read += items.len();
            if next <= after || read >= RUN_LEDGER_CAP {
                break;
            }
            after = next;
        }
    }
    Ok(fold.items)
}

fn load_archived(client: &GatewayClient, wake: &WakeHandle, store: Store) {
    wake.post(move || store.rail.update(|r| r.archived = SessionIndex::Loading));
    let v = match client.list_archived_runs(ARCHIVED_LIMIT) {
        Ok(v) => v,
        Err(e) => {
            let msg = reason(&e);
            wake.post(move || {
                store
                    .rail
                    .update(|r| r.archived = SessionIndex::Failed(msg))
            });
            return;
        }
    };
    let items = v
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let truncated = v.get("has_more").and_then(Value::as_bool).unwrap_or(true);
    let mut rows = crate::runner::fold_session_rows(&items);
    let labeled = rows.len().min(ARCHIVED_PROMPTS);
    for row in rows.iter_mut().take(ARCHIVED_PROMPTS) {
        if !row.first_run.is_empty() {
            row.prompt = crate::runner::session_prompt(client, &row.first_run);
        }
    }
    let archived = rows.len();
    wake.post(move || {
        store.rail.update(|r| {
            r.archived = SessionIndex::Loaded {
                rows,
                truncated,
                labeled,
                archived,
            }
        })
    });
}
