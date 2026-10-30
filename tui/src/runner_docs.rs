//! Off-loop docs execution: project only the dedicated docs run, never the coding fold.
use crate::{gateway::GatewayClient, store::Store};
use abstracttui::reactive::WakeHandle;
use std::time::Duration;

pub fn ask(
    client: GatewayClient,
    wake: WakeHandle,
    store: Store,
    session: String,
    operation: u64,
    question: String,
    corpus: Option<String>,
) {
    let error_wake = wake.clone();
    let error_session = session.clone();
    let spawn = std::thread::Builder::new()
        .name("docs-assistant".into())
        .spawn(move || {
            let work = || -> Result<(), String> {
                let docs = match corpus {
                    Some(text) => text,
                    None => client.docs_corpus().map_err(|e| e.to_string())?,
                };
                let run = client
                    .start_docs_question(&session, &question, &docs)
                    .map_err(|e| e.to_string())?;
                let sid = session.clone();
                let rid = run.clone();
                wake.post(move || {
                    if store
                        .docs_chat
                        .with_untracked(|s| s.matches(&sid, operation))
                    {
                        store.docs_chat.update(|s| {
                            s.corpus = Some(docs);
                            s.run_id = Some(rid);
                        });
                    }
                });
                let mut retry_delay = 1u64;
                loop {
                    let v = match client.get_run(&run) {
                        Ok(v) => {
                            if retry_delay > 1 {
                                let sid = session.clone();
                                wake.post(move || {
                                    if store
                                        .docs_chat
                                        .with_untracked(|s| s.matches(&sid, operation))
                                    {
                                        store.docs_chat.update(|s| s.error.clear());
                                    }
                                });
                            }
                            retry_delay = 1;
                            v
                        }
                        Err(e)
                            if e.is_transient() || e.status.is_some_and(|status| status >= 500) =>
                        {
                            let sid = session.clone();
                            let notice = format!(
                                "Connection interrupted; following the existing docs run. {e}"
                            );
                            wake.post(move || {
                                if store
                                    .docs_chat
                                    .with_untracked(|s| s.matches(&sid, operation))
                                {
                                    store.docs_chat.update(|s| s.error = notice);
                                }
                            });
                            std::thread::sleep(Duration::from_secs(retry_delay));
                            retry_delay = (retry_delay * 2).min(10);
                            continue;
                        }
                        Err(e) => return Err(e.to_string()),
                    };
                    match v
                        .get("status")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                    {
                        "completed" => {
                            let output = v.get("output").unwrap_or(&serde_json::Value::Null);
                            let answer = output
                                .as_str()
                                .or_else(|| {
                                    output.get("response").and_then(serde_json::Value::as_str)
                                })
                                .filter(|s| !s.trim().is_empty())
                                .ok_or("The docs run completed without a written answer.")?
                                .to_owned();
                            let sid = session.clone();
                            wake.post(move || {
                                if store
                                    .docs_chat
                                    .with_untracked(|s| s.matches(&sid, operation))
                                {
                                    store.docs_chat.update(|s| {
                                        s.messages.push(("assistant".into(), answer));
                                        s.busy = false;
                                        s.error.clear();
                                    });
                                }
                            });
                            return Ok(());
                        }
                        "failed" | "cancelled" => {
                            return Err(format!(
                                "The docs run {}. You can retry your question.",
                                v["status"].as_str().unwrap_or("failed")
                            ))
                        }
                        "running" | "waiting" | "queued" => {
                            std::thread::sleep(Duration::from_millis(1000))
                        }
                        other => {
                            return Err(format!(
                                "The Gateway returned an unknown docs run status: {other}"
                            ))
                        }
                    }
                }
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .unwrap_or_else(|_| {
                    Err("The docs assistant worker stopped unexpectedly. You can retry.".into())
                });
            if let Err(error) = result {
                wake.post(move || {
                    if store
                        .docs_chat
                        .with_untracked(|s| s.matches(&session, operation))
                    {
                        store.docs_chat.update(|s| {
                            s.error = error;
                            s.busy = false;
                        });
                    }
                });
            }
        });
    if let Err(e) = spawn {
        error_wake.post(move || {
            if store
                .docs_chat
                .with_untracked(|s| s.matches(&error_session, operation))
            {
                store.docs_chat.update(|s| {
                    s.error = format!("Could not start docs worker: {e}");
                    s.busy = false;
                });
            }
        });
    }
}

#[cfg(test)]
#[path = "runner_docs_tests.rs"]
mod tests;
