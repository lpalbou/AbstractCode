//! The `@entity` mentions lane (2026-11-08 contract): one message's
//! mention calls, sent in order on their own thread, and the UI-thread
//! fold of each answer into the conversation.
//!
//! Order is the contract: call `n + 1` is sent only after call `n`
//! answered, so the second entity reads the first one's reply in the
//! session history. A refusal does not stop the chain (409: "<name> is in
//! another conversation", then the next). Three things do: a gateway
//! WITHOUT the route (one sentence, mentions off, the rest dropped), an
//! entity that cannot answer (503) and an unexplained failure. Those two
//! put the message back into the composer with ONLY the chips not answered
//! ([`Unsent`]); sending it again unchanged asks only those, with the same
//! `repeat_of` and no new turn (client review 2026-11-09 B2).
//!
//! One answer per mention (AbstractGateway f0f23bb0): every call carries
//! `mention_id = <message>:<entity>:<mention_index>`, the message being its
//! turn (`repeat_of`) or an id minted once for it (`mention::message_key`).
//! The resend carries the same key ([`Unsent::message`]), so a call whose
//! answer was lost gets that first answer back, never a second one.
//!
//! One chain at a time per session ([`MentionLanes`], the contract's "one
//! mention at a time per session"): a message's chain waits for the
//! previous chain of the same session, in send order.
//!
//! Threading contract (the runner's rule): the thread never touches
//! signals; every result is posted through the `WakeHandle` and applied
//! on the UI thread, guarded by the session it was sent from.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use abstracttui::reactive::WakeHandle;
use abstracttui::widgets::{AuthorKind, ChatAuthor};
use serde_json::Value;

use crate::gateway::entities::{spawn_named, EntityClient};
use crate::mention::{self, MentionCall, MentionFailure, Refusal};
use crate::store::Store;
use crate::transcript::Item;

/// One mention call's result, as the person will read it.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Answered {
        reply: String,
        author: Option<ChatAuthor>,
    },
    Refused(Refusal),
}

impl Outcome {
    fn from_result(result: Result<Value, MentionFailure>) -> Outcome {
        match result {
            Ok(v) => {
                let (reply, author) = mention::reply_from_response(&v);
                Outcome::Answered { reply, author }
            }
            Err(f) => Outcome::Refused(mention::classify(&f)),
        }
    }

    /// The chain ends here: this gateway has no mention route, or this
    /// entity could not answer, or the call failed.
    pub fn ends_chain(&self) -> bool {
        matches!(
            self,
            Outcome::Refused(Refusal::NotOffered | Refusal::CannotAnswer(_) | Refusal::Failed(_))
        )
    }

    /// The chain ended here and the message goes back into the composer
    /// for a resend (an entity could not answer, or the call failed).
    pub fn keeps_message(&self) -> bool {
        matches!(
            self,
            Outcome::Refused(Refusal::CannotAnswer(_) | Refusal::Failed(_))
        )
    }
}

/// What a stopped chain leaves for the resend: the draft put back (only
/// the unsent chips stay chips), the words as first sent, the turn they
/// repeat, and the calls not answered (the stopped one and those after it,
/// with their first `mention_index`). Submitting that exact draft again
/// asks only these calls — never a new turn, never an entity that answered.
/// `message`: the key the calls were first sent with (their `mention_id`s).
#[derive(Debug, Clone, PartialEq)]
pub struct Unsent {
    pub session_id: String,
    pub draft: String,
    pub text: String,
    pub repeat_of: Option<String>,
    pub message: String,
    pub calls: Vec<MentionCall>,
}

/// Run one message's calls IN ORDER on the calling thread: each call is
/// sent after the previous one answered. `on` receives every outcome as it
/// lands; the chain stops after [`Outcome::ends_chain`]. `message`: the
/// message's key (each call's `mention_id`).
pub fn run_chain(
    client: &EntityClient,
    session_id: &str,
    text: &str,
    repeat_of: Option<&str>,
    message: &str,
    calls: &[MentionCall],
    mut on: impl FnMut(&MentionCall, Outcome),
) {
    for call in calls {
        let body = mention::request_body(text, call, repeat_of, message);
        let outcome = Outcome::from_result(client.mention(session_id, &body));
        let stop = outcome.ends_chain();
        on(call, outcome);
        if stop {
            return;
        }
    }
}

/// One message's mentions, as the runner hands them to the lane.
pub struct MentionMessage {
    pub session_id: String,
    /// The message as typed, chips spelled `@name`.
    pub text: String,
    /// The raw draft (chips as markers): what a 503 puts back, rebuilt.
    pub draft: String,
    /// Every chip of the draft in text order (`mention::chips`): which
    /// entity each marker of `draft` stands for.
    pub chips: Vec<MentionCall>,
    /// The run id of the turn these words started, when they started one.
    pub repeat_of: Option<String>,
    /// The message's key for every call's `mention_id`
    /// (`mention::message_key`; a resend carries the first send's).
    pub message: String,
    pub calls: Vec<MentionCall>,
}

impl MentionMessage {
    /// What the chain leaves for a resend when it stopped at `call`.
    pub fn unsent_from(&self, call: &MentionCall) -> Unsent {
        let at = self.calls.iter().position(|c| c == call).unwrap_or(0);
        let calls = self.calls[at..].to_vec();
        Unsent {
            session_id: self.session_id.clone(),
            draft: mention::restore_draft(&self.draft, &self.chips, &calls),
            text: self.text.clone(),
            repeat_of: self.repeat_of.clone(),
            message: self.message.clone(),
            calls,
        }
    }
}

type Job = Box<dyn FnOnce() + Send + 'static>;

/// One mention chain at a time per session: [`MentionLanes::submit`] runs a
/// job at once when its session has no chain running, else after the jobs
/// queued before it (FIFO), on the thread of the chain before it.
#[derive(Clone, Default)]
pub struct MentionLanes(Arc<Mutex<HashMap<String, VecDeque<Job>>>>);

impl MentionLanes {
    /// Run `job` for `session_id`: now (through `spawn`, which starts the
    /// session's worker thread) when no chain of that session runs, else
    /// once the chains queued before it ended. True when it waits.
    pub fn submit(&self, session_id: &str, job: Job, spawn: impl FnOnce(Job)) -> bool {
        {
            let mut lanes = self.0.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(queue) = lanes.get_mut(session_id) {
                queue.push_back(job);
                return true;
            }
            lanes.insert(session_id.to_string(), VecDeque::new());
        }
        let lanes = self.clone();
        let sid = session_id.to_string();
        spawn(Box::new(move || lanes.drain(&sid, job)));
        false
    }

    /// The session's worker: `first`, then every job queued meanwhile, in
    /// order; the lane is freed when none is left. A job that panics does
    /// not wedge the lane (the next ones still run); the first panic is
    /// raised again at the end, for the thread's own report.
    fn drain(&self, session_id: &str, first: Job) {
        let mut panic = None;
        let mut next = Some(first);
        while let Some(job) = next.take() {
            if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)) {
                panic.get_or_insert(p);
            }
            let mut lanes = self.0.lock().unwrap_or_else(|p| p.into_inner());
            next = lanes.get_mut(session_id).and_then(VecDeque::pop_front);
            if next.is_none() {
                lanes.remove(session_id);
            }
        }
        if let Some(p) = panic {
            std::panic::resume_unwind(p);
        }
    }
}

/// The runner's `Cmd::Mentions`: the chain on its own thread (a mention
/// is a visit turn — minutes of model time must never hold the runner's
/// command loop), after any chain of the same session still running.
pub fn spawn_mentions(
    client: EntityClient,
    wake: WakeHandle,
    store: Store,
    lanes: &MentionLanes,
    msg: MentionMessage,
) {
    let session_id = msg.session_id.clone();
    let names = msg
        .calls
        .iter()
        .map(|c| c.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let post = wake.clone();
    let job: Job = Box::new(move || {
        let msg = Arc::new(msg);
        let repeat = msg.repeat_of.clone();
        run_chain(
            &client,
            &msg.session_id,
            &msg.text,
            repeat.as_deref(),
            &msg.message,
            &msg.calls,
            |call, outcome| {
                let unsent = outcome.keeps_message().then(|| msg.unsent_from(call));
                let (sid, call) = (msg.session_id.clone(), call.clone());
                post.post(move || apply(store, &sid, &call, outcome, unsent));
            },
        );
    });
    let waits = lanes.submit(&session_id, job, |job| {
        spawn_named("entity-mentions", wake.clone(), store, None, move |_| job());
    });
    if waits {
        wake.post(move || store.notify(format!("{names}: asked when the mentions before end")));
    }
}

/// Fold one outcome into the conversation it was sent from (UI thread).
/// `unsent`: what a stopped chain leaves for the resend (a 503 or a
/// failure): the draft goes back into the composer and is remembered.
pub fn apply(
    store: Store,
    session_id: &str,
    call: &MentionCall,
    outcome: Outcome,
    unsent: Option<Unsent>,
) {
    if matches!(outcome, Outcome::Refused(Refusal::NotOffered)) {
        // Process-wide: the route is missing on this gateway, whatever
        // the session.
        store.mentions_off.set(true);
    }
    let name = call.name.as_str();
    let item = match outcome {
        Outcome::Answered { reply, .. } if reply.is_empty() => Item::Error {
            text: format!("{name} gave no reply"),
        },
        Outcome::Answered { reply, author } => Item::Assistant {
            text: reply,
            final_answer: true,
            // The contract's 200 always names the author; we asked this
            // entity, so a missing one is still this entity, never the agent.
            author: Some(author.unwrap_or_else(|| ChatAuthor::new(AuthorKind::Entity, name))),
        },
        Outcome::Refused(Refusal::Elsewhere) => Item::Info {
            text: format!("{name} is in another conversation."),
        },
        Outcome::Refused(Refusal::Refused(sentence)) => Item::Info { text: sentence },
        Outcome::Refused(Refusal::NotFound) => Item::Info {
            text: format!("{name} cannot be mentioned here."),
        },
        Outcome::Refused(Refusal::CannotAnswer(sentence)) => Item::Info { text: sentence },
        Outcome::Refused(Refusal::NotOffered) => Item::Info {
            text: format!("{}.", mention::NOT_OFFERED),
        },
        Outcome::Refused(Refusal::Failed(why)) => Item::Error {
            text: format!("the mention of {name} failed: {why}"),
        },
    };
    if store.session_id.with_untracked(|s| s != session_id) {
        // The person moved to another conversation meanwhile: the reply is
        // in that session's history; say so instead of writing it here
        // (and never put that conversation's message into this composer).
        store.notify(format!(
            "{name}: the answer is in the conversation you left (its history keeps it)"
        ));
        return;
    }
    if let Some(unsent) = unsent {
        // The message goes back into the composer with only the chips not
        // answered (dropped by root() when the person has typed since), and
        // is remembered: sending it again unchanged asks only those.
        store.steer_restore.set(Some(unsent.draft.clone()));
        store.mention_retry.set(Some(unsent));
    }
    store.fold.update(|f| f.push_item(item));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A fake gateway answering one connection at a time, logging
    /// `recv <mention_index>` as each request arrives.
    fn serve(
        replies: Vec<(u16, &'static str, Value)>,
        log: Arc<Mutex<Vec<String>>>,
    ) -> (String, std::thread::JoinHandle<Vec<Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for (status, code, value) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                // Read headers + body (Content-Length bounded).
                loop {
                    let n = stream.read(&mut chunk).unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(at) = text.find("\r\n\r\n") {
                        let len = text[..at]
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if buf.len() >= at + 4 + len {
                            let body: Value =
                                serde_json::from_slice(&buf[at + 4..at + 4 + len]).unwrap();
                            assert!(
                                text.starts_with("POST /api/gateway/sessions/sess-1/mentions "),
                                "{text}"
                            );
                            log.lock()
                                .unwrap()
                                .push(format!("recv {}", body["mention_index"]));
                            bodies.push(body);
                            break;
                        }
                    }
                }
                let body = value.to_string();
                let header = if code.is_empty() {
                    String::new()
                } else {
                    format!("X-Gateway-Error-Code: {code}\r\n")
                };
                write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
            bodies
        });
        (url, handle)
    }

    fn calls() -> Vec<MentionCall> {
        vec![
            MentionCall {
                entity: "castor".into(),
                name: "Castor".into(),
                index: 0,
            },
            MentionCall {
                entity: "pollux".into(),
                name: "Pollux".into(),
                index: 1,
            },
        ]
    }

    fn answer(name: &str, reply: &str) -> Value {
        json!({"run_id": "r", "turn_id": "t", "session_id": "sess-1", "mention_index": 0,
            "reply": reply, "visit": {},
            "author": {"kind": "entity", "name": name, "entity_id": format!("entity:{}", name.to_lowercase()),
                       "identity_ref": null, "avatar": {"style": "watch-halo", "color": "#336699"}}})
    }

    #[test]
    fn calls_are_sequential_each_after_the_previous_answered() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (url, server) = serve(
            vec![
                (200, "", answer("Castor", "The plan holds.")),
                (200, "", answer("Pollux", "Castor is right.")),
            ],
            log.clone(),
        );
        let client = EntityClient::new(&url, None);
        let mut seen = Vec::new();
        run_chain(
            &client,
            "sess-1",
            "@Castor @Pollux does it hold?",
            Some("run-a1"),
            "run-a1",
            &calls(),
            |call, outcome| {
                log.lock().unwrap().push(format!("done {}", call.index));
                seen.push(outcome);
            },
        );
        // Asserted BEFORE the join: a chain that sent fewer calls than the
        // server waits for must fail here, never hang there.
        assert_eq!(
            *log.lock().unwrap(),
            ["recv 0", "done 0", "recv 1", "done 1"],
            "the second call leaves only after the first answered"
        );
        let bodies = server.join().unwrap();
        assert_eq!(bodies[0]["entity"], "castor");
        assert_eq!(bodies[1]["entity"], "pollux");
        assert_eq!(bodies[1]["text"], "@Castor @Pollux does it hold?");
        assert_eq!(bodies[0]["repeat_of"], "run-a1", "each call names the turn");
        assert_eq!(bodies[1]["repeat_of"], "run-a1");
        assert!(
            matches!(&seen[1], Outcome::Answered { reply, author: Some(a) }
            if reply == "Castor is right." && a.name == "Pollux")
        );
    }

    #[test]
    fn a_409_skips_to_the_next_mention_and_keeps_its_code() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (url, server) = serve(
            vec![
                (
                    409,
                    "in_another_conversation",
                    json!({"detail": "Castor is in another conversation (session s9).",
                        "code": "in_another_conversation", "session_id": "s9"}),
                ),
                (200, "", answer("Pollux", "Here.")),
            ],
            log,
        );
        let client = EntityClient::new(&url, None);
        let mut seen = Vec::new();
        run_chain(
            &client,
            "sess-1",
            "@Castor @Pollux hi",
            None,
            "msg-1",
            &calls(),
            |_, o| seen.push(o),
        );
        assert_eq!(seen.len(), 2, "the chain went on after the 409: {seen:?}");
        assert_eq!(seen[0], Outcome::Refused(Refusal::Elsewhere));
        assert!(matches!(&seen[1], Outcome::Answered { .. }), "{seen:?}");
        server.join().unwrap();
    }

    #[test]
    fn the_header_code_answers_when_the_body_has_none() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (url, server) = serve(
            vec![(
                503,
                "entity_cannot_answer",
                json!({"detail": "Castor cannot answer: no model chosen"}),
            )],
            log,
        );
        let client = EntityClient::new(&url, None);
        let mut seen = Vec::new();
        run_chain(
            &client,
            "sess-1",
            "@Castor hi",
            None,
            "msg-1",
            &calls()[..1],
            |_, o| seen.push(o),
        );
        assert_eq!(
            seen,
            [Outcome::Refused(Refusal::CannotAnswer(
                "Castor cannot answer: no model chosen".into()
            ))]
        );
        server.join().unwrap();
    }

    #[test]
    fn an_entity_not_found_is_skipped_and_the_next_one_is_asked() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (url, server) = serve(
            vec![
                (
                    404,
                    "entity_not_found",
                    json!({"detail": "entity 'castor' not found under /x", "code": "entity_not_found"}),
                ),
                (200, "", answer("Pollux", "Here.")),
            ],
            log,
        );
        let client = EntityClient::new(&url, None);
        let mut seen = Vec::new();
        run_chain(
            &client,
            "sess-1",
            "@Castor @Pollux hi",
            None,
            "msg-1",
            &calls(),
            |_, o| seen.push(o),
        );
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(seen[0], Outcome::Refused(Refusal::NotFound));
        server.join().unwrap();
    }

    /// Client review 2026-11-09 B2: a 503 (or a failure) ends the chain;
    /// the next mention is never sent.
    #[test]
    fn a_503_or_a_failure_ends_the_chain_after_that_call() {
        for (status, code, body) in [
            (
                503,
                "entity_cannot_answer",
                json!({"detail": "Castor cannot answer right now.", "code": "entity_cannot_answer"}),
            ),
            (500, "", json!({"detail": "boom"})),
        ] {
            let log = Arc::new(Mutex::new(Vec::new()));
            let (url, server) = serve(vec![(status, code, body)], log.clone());
            let client = EntityClient::new(&url, None);
            let mut seen = Vec::new();
            run_chain(
                &client,
                "sess-1",
                "@Castor @Pollux hi",
                None,
                "msg-1",
                &calls(),
                |_, o| seen.push(o),
            );
            assert_eq!(seen.len(), 1, "{status}: {seen:?}");
            assert!(seen[0].ends_chain() && seen[0].keeps_message(), "{status}");
            server.join().unwrap();
            assert_eq!(
                *log.lock().unwrap(),
                ["recv 0"],
                "{status}: Pollux is never asked"
            );
        }
        let refused = Outcome::Refused(Refusal::Elsewhere);
        assert!(
            !refused.ends_chain() && !refused.keeps_message(),
            "a 409 goes on"
        );
        let missing = Outcome::Refused(Refusal::NotOffered);
        assert!(
            missing.ends_chain() && !missing.keeps_message(),
            "no route: mentions off, nothing kept"
        );
    }

    #[test]
    fn a_stop_leaves_only_the_unsent_calls_and_their_chips() {
        let m = |k: u32| char::from_u32(0xF_0000 + k).unwrap();
        let msg = MentionMessage {
            session_id: "sess-1".into(),
            text: "@Castor and @Pollux hi".into(),
            draft: format!("{} and {} hi", m(0), m(1)),
            chips: calls(),
            repeat_of: Some("run-a1".into()),
            message: "run-a1".into(),
            calls: calls(),
        };
        let unsent = msg.unsent_from(&calls()[1]);
        assert_eq!(
            unsent,
            Unsent {
                session_id: "sess-1".into(),
                draft: format!("Castor and {} hi", m(1)),
                text: "@Castor and @Pollux hi".into(),
                repeat_of: Some("run-a1".into()),
                message: "run-a1".into(),
                calls: calls()[1..].to_vec(),
            }
        );
        assert_eq!(
            msg.unsent_from(&calls()[0]).calls,
            calls(),
            "a stop on the first keeps both"
        );
    }

    /// One answer per mention (AbstractGateway f0f23bb0): every call names
    /// `<message>:<entity>:<index>`; the resend of a stopped chain (its
    /// [`Unsent`]) names the same ids, so a call whose answer was lost gets
    /// that first answer back; a new message names new ones.
    #[test]
    fn a_resend_names_the_same_mention_ids_and_a_new_message_new_ones() {
        for repeat_of in [Some("run-a1"), None] {
            let message = crate::mention::message_key(repeat_of);
            let log = Arc::new(Mutex::new(Vec::new()));
            let (url, server) = serve(
                vec![
                    (200, "", answer("Castor", "Here.")),
                    (
                        503,
                        "entity_cannot_answer",
                        json!({"detail": "Pollux cannot answer right now."}),
                    ),
                    (200, "", answer("Pollux", "Here.")),
                ],
                log,
            );
            let client = EntityClient::new(&url, None);
            let msg = MentionMessage {
                session_id: "sess-1".into(),
                text: "@Castor @Pollux hi".into(),
                draft: "@Castor @Pollux hi".into(),
                chips: calls(),
                repeat_of: repeat_of.map(String::from),
                message: message.clone(),
                calls: calls(),
            };
            let mut unsent = None;
            run_chain(
                &client,
                "sess-1",
                &msg.text,
                repeat_of,
                &msg.message,
                &msg.calls,
                |call, o| {
                    if o.keeps_message() {
                        unsent = Some(msg.unsent_from(call));
                    }
                },
            );
            let held = unsent.expect("the 503 holds the message");
            run_chain(
                &client,
                "sess-1",
                &held.text,
                held.repeat_of.as_deref(),
                &held.message,
                &held.calls,
                |_, _| {},
            );
            let bodies = server.join().unwrap();
            let ids: Vec<_> = bodies.iter().map(|b| b["mention_id"].clone()).collect();
            assert_eq!(
                ids,
                [
                    json!(format!("{message}:castor:0")),
                    json!(format!("{message}:pollux:1")),
                    json!(format!("{message}:pollux:1")),
                ],
                "{repeat_of:?}: the resend names Pollux's first id"
            );
            if let Some(turn) = repeat_of {
                assert_eq!(message, turn, "the key is the turn the words started");
            }
        }
        assert_ne!(
            crate::mention::message_key(None),
            crate::mention::message_key(None),
            "a new message, a new key"
        );
    }

    /// Client review 2026-11-09 S4: one chain at a time per session, in
    /// send order; another session's chain does not wait.
    #[test]
    fn chains_of_one_session_run_one_after_the_other() {
        use std::sync::mpsc;
        let lanes = MentionLanes::default();
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let spawn = |job: Job| {
            std::thread::spawn(job);
        };
        let (release_a, wait_a) = mpsc::channel::<()>();
        let (a_started, a_started_rx) = mpsc::channel::<()>();
        let (done_tx, done_rx) = mpsc::channel::<&'static str>();
        let (l, d) = (log.clone(), done_tx.clone());
        let waits = lanes.submit(
            "s1",
            Box::new(move || {
                l.lock().unwrap().push("a start".into());
                a_started.send(()).unwrap();
                wait_a.recv().unwrap();
                l.lock().unwrap().push("a end".into());
                d.send("a").unwrap();
            }),
            spawn,
        );
        assert!(!waits, "the first chain runs at once");
        a_started_rx.recv().unwrap();
        let (l, d) = (log.clone(), done_tx.clone());
        assert!(
            lanes.submit(
                "s1",
                Box::new(move || {
                    l.lock().unwrap().push("b".into());
                    d.send("b").unwrap();
                }),
                spawn
            ),
            "the second chain of s1 waits"
        );
        let (l, d) = (log.clone(), done_tx.clone());
        assert!(
            !lanes.submit(
                "s2",
                Box::new(move || {
                    l.lock().unwrap().push("other".into());
                    d.send("other").unwrap();
                }),
                spawn
            ),
            "another session does not wait"
        );
        assert_eq!(done_rx.recv().unwrap(), "other");
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert_eq!(*log.lock().unwrap(), ["a start", "other"], "b waits for a");
        release_a.send(()).unwrap();
        assert_eq!(done_rx.recv().unwrap(), "a");
        assert_eq!(done_rx.recv().unwrap(), "b");
        assert_eq!(*log.lock().unwrap(), ["a start", "other", "a end", "b"]);
        // The lane is free again: the next chain runs at once.
        std::thread::sleep(std::time::Duration::from_millis(30));
        let d = done_tx.clone();
        assert!(!lanes.submit("s1", Box::new(move || d.send("c").unwrap()), spawn));
        assert_eq!(done_rx.recv().unwrap(), "c");
    }

    #[test]
    fn a_chain_that_panics_does_not_wedge_its_session() {
        // A wedged lane must fail this test, never hang it.
        const WEDGED: std::time::Duration = std::time::Duration::from_secs(5);
        let lanes = MentionLanes::default();
        let (tx, rx) = std::sync::mpsc::channel::<&'static str>();
        let (gate_tx, gate_rx) = std::sync::mpsc::channel::<()>();
        let handles = Arc::new(Mutex::new(Vec::new()));
        let h = handles.clone();
        let spawn = move |job: Job| {
            h.lock().unwrap().push(std::thread::spawn(job));
        };
        let t = tx.clone();
        lanes.submit(
            "s1",
            Box::new(move || {
                gate_rx.recv().unwrap();
                t.send("a").unwrap();
                panic!("a died");
            }),
            spawn.clone(),
        );
        let t = tx.clone();
        assert!(lanes.submit("s1", Box::new(move || t.send("b").unwrap()), spawn.clone()));
        gate_tx.send(()).unwrap();
        assert_eq!(rx.recv_timeout(WEDGED).expect("the lane is wedged"), "a");
        assert_eq!(
            rx.recv_timeout(WEDGED).expect("the lane is wedged"),
            "b",
            "the queued chain still runs"
        );
        let worker = handles.lock().unwrap().pop().unwrap();
        assert!(
            worker.join().is_err(),
            "the panic is raised again for the thread's report"
        );
        let t = tx.clone();
        assert!(
            !lanes.submit("s1", Box::new(move || t.send("c").unwrap()), spawn),
            "the lane is free"
        );
        assert_eq!(rx.recv_timeout(WEDGED).expect("the lane is wedged"), "c");
    }

    #[test]
    fn a_gateway_without_the_route_ends_the_chain_after_one_call() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (url, server) = serve(vec![(404, "", json!({"detail": "Not Found"}))], log.clone());
        let client = EntityClient::new(&url, None);
        let mut seen = Vec::new();
        run_chain(
            &client,
            "sess-1",
            "@Castor @Pollux hi",
            None,
            "msg-1",
            &calls(),
            |_, o| seen.push(o),
        );
        assert_eq!(seen, [Outcome::Refused(Refusal::NotOffered)]);
        server.join().unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            ["recv 0"],
            "the second call is never sent"
        );
    }
}
