//! The person's own event stream (framework backlog 1046/1047) for the
//! status-line attention chip (`crate::attention`).
//!
//! `GET /api/gateway/me/events?after=<seq>&limit=<n>` →
//! `{stream, events[], next_after, head, more}` (catch-up pages), and
//! `GET /api/gateway/me/events/stream` (SSE: `id: <sequence>`, `event:
//! <type>`, `data: <CloudEvents JSON>`, `: keep-alive` comments), resumed
//! with `Last-Event-ID`. Always the CALLER's own stream.
//!
//! One follower thread per connection: catch-up first, then the live tail
//! while the terminal is in front (`attention::foreground`), else one
//! catch-up every `attention::CATCH_UP_EVERY`. A 404 means a gateway
//! without the stream: the follower stops and says so (the chip keeps the
//! list poll); 401/403 stop it too (signed out). A newer follower (another
//! `Cmd::FollowEvents`, i.e. a reconnect or a new sign-in) retires the old
//! one at its next check.

use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use abstracttui::reactive::WakeHandle;
use serde_json::Value;

use super::{err_from_ureq, sse, GatewayClient, GwError, GwResult};
use crate::attention::{self, EventsState};
use crate::store::Store;

/// The keep-alive cadence asked of the server (the read timeout of the
/// stream agent, 75 s, stays well above it).
pub const HEARTBEAT_S: u32 = 15;
const PAGE: u32 = 1000;

static GENERATION: AtomicU64 = AtomicU64::new(0);

impl GatewayClient {
    /// One catch-up page after `after`.
    pub fn my_events(&self, after: u64, limit: u32) -> GwResult<Value> {
        self.get_json(&format!("/me/events?after={after}&limit={limit}"))
    }

    /// Follow the live tail from `last_event_id` (sent as `Last-Event-ID`)
    /// until the server closes it, the read idles out, or `keep()` turns
    /// false (checked after every read: at least once per heartbeat).
    pub fn stream_my_events(
        &self,
        last_event_id: u64,
        mut keep: impl FnMut() -> bool,
        mut on_event: impl FnMut(Value),
    ) -> GwResult<()> {
        let path = format!("/me/events/stream?heartbeat_s={HEARTBEAT_S}");
        let mut req = self.with_auth(
            self.stream_agent
                .get(&self.url(&path))
                .set("Accept", "text/event-stream"),
        );
        if last_event_id > 0 {
            req = req.set("Last-Event-ID", &last_event_id.to_string());
        }
        let resp = req.call().map_err(|e| err_from_ureq(&path, e))?;
        let mut reader = resp.into_reader();
        let mut parser = sse::SseParser::new();
        let mut buf = [0u8; 16 * 1024];
        loop {
            if !keep() {
                return Ok(());
            }
            let n = match reader.read(&mut buf) {
                Ok(0) => return Ok(()),
                Ok(n) => n,
                Err(e) => {
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) {
                        return Ok(()); // idle past the read timeout: reconnect
                    }
                    return Err(GwError::from_io_read(
                        format!("event stream read failed: {e}"),
                        &e,
                    ));
                }
            };
            for ev in parser.push(&buf[..n]) {
                if let Ok(mut v) = serde_json::from_str::<Value>(&ev.data) {
                    if v.get("sequence").is_none() {
                        if let Ok(id) = ev.id.parse::<u64>() {
                            v["sequence"] = Value::from(id);
                        }
                    }
                    on_event(v);
                }
            }
        }
    }
}

fn post_state(wake: &WakeHandle, store: Store, state: EventsState) {
    wake.post(move || {
        if store.events_state.get_untracked() != state {
            store.events_state.set(state);
        }
    });
}

fn post_events(wake: &WakeHandle, store: Store, events: Vec<Value>) {
    if events.is_empty() {
        return;
    }
    wake.post(move || {
        store.event_attention.update(|a| {
            for e in &events {
                a.apply(e);
            }
        })
    });
}

fn seq(v: &Value) -> u64 {
    v.get("sequence").and_then(Value::as_u64).unwrap_or(0)
}

/// Read every page after `cursor` (each page posted as ONE store update).
fn catch_up(
    client: &GatewayClient,
    wake: &WakeHandle,
    store: Store,
    cursor: &mut u64,
) -> GwResult<()> {
    for _ in 0..100 {
        let page = client.my_events(*cursor, PAGE)?;
        let events: Vec<Value> = page
            .get("events")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|e| seq(e) > *cursor)
            .collect();
        if let Some(last) = events.iter().map(seq).max() {
            *cursor = last;
        }
        post_events(wake, store, events);
        if page.get("more").and_then(Value::as_bool) != Some(true) {
            return Ok(());
        }
    }
    Ok(())
}

/// Start following (and retire any earlier follower).
pub fn spawn_follower(client: &GatewayClient, wake: WakeHandle, store: Store) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let current = move || GENERATION.load(Ordering::SeqCst) == generation;
    let client = client.clone();
    crate::runner::spawn_host_thread("me-events", wake.clone(), store, move || {
        let mut cursor = 0u64;
        let mut backoff = Duration::from_secs(1);
        while current() {
            match catch_up(&client, &wake, store, &mut cursor) {
                Ok(()) => {
                    backoff = Duration::from_secs(1);
                    post_state(&wake, store, EventsState::Following);
                }
                Err(e) if e.status == Some(404) => {
                    post_state(&wake, store, EventsState::Unsupported);
                    return;
                }
                Err(e) if matches!(e.status, Some(401) | Some(403)) => return,
                Err(_) => {
                    std::thread::sleep(backoff);
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                    continue;
                }
            }
            if attention::foreground() {
                // The live tail, for as long as someone is at the terminal.
                let wake2 = wake.clone();
                let mut batch_cursor = cursor;
                let outcome = client.stream_my_events(
                    cursor,
                    || current() && attention::foreground(),
                    |ev| {
                        let s = seq(&ev);
                        if s > batch_cursor {
                            batch_cursor = s;
                            post_events(&wake2, store, vec![ev]);
                        }
                    },
                );
                cursor = cursor.max(batch_cursor);
                if outcome.is_err() {
                    std::thread::sleep(backoff);
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
            } else {
                // Away: no open stream; one catch-up a minute (sooner when
                // someone comes back).
                let until = Instant::now() + attention::CATCH_UP_EVERY;
                while Instant::now() < until && current() && !attention::foreground() {
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
    });
}
