//! The `@entity` mentions lane (2026-11-08 contract): one message's
//! mention calls, sent in order on their own thread, and the UI-thread
//! fold of each answer into the conversation.
//!
//! Order is the contract: call `n + 1` is sent only after call `n`
//! answered, so the second entity reads the first one's reply in the
//! session history. A refusal never stops the chain (409: "<name> is in
//! another conversation", then the next); only a gateway WITHOUT the route
//! does (one sentence, mentions off, the rest dropped).
//!
//! Threading contract (the runner's rule): the thread never touches
//! signals; every result is posted through the `WakeHandle` and applied
//! on the UI thread, guarded by the session it was sent from.

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

    /// The chain ends here: this gateway has no mention route.
    fn ends_chain(&self) -> bool {
        matches!(self, Outcome::Refused(Refusal::NotOffered))
    }
}

/// Run one message's calls IN ORDER on the calling thread: each call is
/// sent after the previous one answered. `on` receives every outcome as it
/// lands; the chain stops after a [`Refusal::NotOffered`].
pub fn run_chain(
    client: &EntityClient,
    session_id: &str,
    text: &str,
    repeat_of: Option<&str>,
    calls: &[MentionCall],
    mut on: impl FnMut(&MentionCall, Outcome),
) {
    for call in calls {
        let body = mention::request_body(text, call, repeat_of);
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
    /// The raw draft (chips as markers): what a 503 puts back.
    pub draft: String,
    /// The run id of the turn these words started, when they started one.
    pub repeat_of: Option<String>,
    pub calls: Vec<MentionCall>,
}

/// The runner's `Cmd::Mentions`: the chain on its own thread (a mention
/// is a visit turn — minutes of model time must never hold the runner's
/// command loop).
pub fn spawn_mentions(client: EntityClient, wake: WakeHandle, store: Store, msg: MentionMessage) {
    let MentionMessage {
        session_id,
        text,
        draft,
        repeat_of,
        calls,
    } = msg;
    spawn_named("entity-mentions", wake.clone(), store, None, move |wake| {
        let repeat = repeat_of.as_deref();
        run_chain(
            &client,
            &session_id,
            &text,
            repeat,
            &calls,
            |call, outcome| {
                let (sid, draft, call) = (session_id.clone(), draft.clone(), call.clone());
                wake.post(move || apply(store, &sid, &call, outcome, &draft));
            },
        );
    });
}

/// Fold one outcome into the conversation it was sent from (UI thread).
pub fn apply(store: Store, session_id: &str, call: &MentionCall, outcome: Outcome, draft: &str) {
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
        Outcome::Refused(Refusal::CannotAnswer(sentence)) => {
            // The message goes back into the composer (dropped by root()
            // when the person has typed since).
            store.steer_restore.set(Some(draft.to_string()));
            Item::Info { text: sentence }
        }
        Outcome::Refused(Refusal::NotOffered) => Item::Info {
            text: format!("{}.", mention::NOT_OFFERED),
        },
        Outcome::Refused(Refusal::Failed(why)) => Item::Error {
            text: format!("the mention of {name} failed: {why}"),
        },
    };
    if store.session_id.with_untracked(|s| s != session_id) {
        // The person moved to another conversation meanwhile: the reply is
        // in that session's history; say so instead of writing it here.
        store.notify(format!(
            "{name}: the answer is in the conversation you left (its history keeps it)"
        ));
        return;
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
            &calls(),
            |_, o| seen.push(o),
        );
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(seen[0], Outcome::Refused(Refusal::NotFound));
        server.join().unwrap();
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
