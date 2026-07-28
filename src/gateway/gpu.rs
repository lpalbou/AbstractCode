//! `/gpu` meter poller (OBS-6): samples the gateway host's GPU
//! utilization on its OWN thread — the runner command loop must never
//! block behind a metrics read (the entity-lane rule applied here).
//!
//! Threading contract (the engine rule, same as `runner.rs`): this
//! thread never touches signals — it posts closures through a cloned
//! `WakeHandle`, and every posted closure re-checks the POLLER
//! GENERATION before writing (a disabled/replaced poller's late sample
//! must never overwrite `GpuMeter::Off`).
//!
//! Cadence: ~3s while a run/turn is active, ~30s idle — the activity
//! hint arrives through [`set_fast`] (a UI effect mirrors phase +
//! entity-turn activity into the atomic; the thread cannot read
//! signals). Sleep happens in short slices so stop/cadence changes
//! apply within ~250ms of SLEEPING time instead of a blind 30s. A
//! thread caught MID-HTTP-CALL is uninterruptible for up to the
//! client's 60s read timeout — bounded, and its post is generation-
//! gated either way, so a lingering superseded thread can never write
//! (cycle-2 integration review: the old "within ~250ms" claim
//! overstated the stop latency for that window).
//!
//! Honesty: `supported:false` (or a 404 — the endpoint predates this
//! gateway) posts [`GpuMeter::Unsupported`] and STOPS polling — the
//! meter never fabricates a number and never hammers an endpoint that
//! said no. Transient errors post [`GpuMeter::Error`] and keep polling.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use abstracttui::reactive::WakeHandle;
use serde_json::Value;

use crate::gateway::GatewayClient;
use crate::store::{GpuMeter, GpuSample, Store};

/// Poll cadence while a run / entity turn is active.
const POLL_ACTIVE_S: u64 = 3;
/// Poll cadence while everything idles (cheap keep-warm).
const POLL_IDLE_S: u64 = 30;
/// Sleep slice: stop/cadence flips apply within one slice.
const SLICE_MS: u64 = 250;

/// Poller generation: bumped by every start/stop. A thread whose
/// captured generation no longer matches exits at its next slice, and
/// its posted closures no-op (stale-sample guard).
static GEN: AtomicU64 = AtomicU64::new(0);
/// Cadence hint (true = a run/turn is active → 3s cadence).
static FAST: AtomicBool = AtomicBool::new(false);

/// UI-thread mirror of "is anything running" (phase / entity turns).
pub fn set_fast(fast: bool) {
    FAST.store(fast, Ordering::Relaxed);
}

/// Stop the poller (idempotent): invalidates the running thread AND
/// gates any in-flight sample's post. The `/gpu` dispatch sets the
/// signal to `Off` on the UI thread; this makes sure nothing overwrites
/// it afterwards.
pub fn stop() {
    GEN.fetch_add(1, Ordering::SeqCst);
}

/// Start (or restart) the poller thread. The first sample fires
/// immediately, so toggling the meter on answers within one round trip.
pub fn start(client: GatewayClient, wake: WakeHandle, store: Store) {
    let my_gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let panic_wake = wake.clone();
    let _ = std::thread::Builder::new()
        .name("gpu-poller".into())
        .spawn(move || {
            // Panic surfacing (the 0.2.0 worker discipline): a dead
            // poller must say so, never leave a frozen meter.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                poll_loop(client, wake, store, my_gen);
            }));
            if let Err(payload) = result {
                let msg = crate::runner::panic_text(payload.as_ref());
                panic_wake.post(move || {
                    if GEN.load(Ordering::SeqCst) != my_gen {
                        return; // superseded: a newer poller owns the meter
                    }
                    store
                        .gpu
                        .set(GpuMeter::Error(format!("gpu poller died: {msg}")));
                    store.notify(format!("gpu poller died: {msg} — /gpu restarts it"));
                });
            }
        });
}

fn poll_loop(client: GatewayClient, wake: WakeHandle, store: Store, my_gen: u64) {
    loop {
        if GEN.load(Ordering::SeqCst) != my_gen {
            return; // superseded or stopped
        }
        let meter = match client.host_gpu_metrics() {
            Ok(v) => meter_from_response(&v),
            // A 404 means the endpoint is not on this gateway — that is
            // an UNSUPPORTED truth (stop polling), not a transient error.
            Err(e) if e.status == Some(404) => {
                GpuMeter::Unsupported("endpoint not on this gateway (404)".into())
            }
            Err(e) => GpuMeter::Error(e.to_string()),
        };
        let unsupported = matches!(meter, GpuMeter::Unsupported(_));
        {
            let meter = meter.clone();
            wake.post(move || {
                if GEN.load(Ordering::SeqCst) != my_gen {
                    return; // stale: the meter was toggled off/replaced
                }
                let first = matches!(store.gpu.get_untracked(), GpuMeter::Pending);
                // Notices for the transitions the footer cannot carry
                // yet: the FIRST reading after /gpu, and the one-time
                // unsupported verdict.
                match &meter {
                    GpuMeter::Ready(s) if first => {
                        let name = if s.name.is_empty() {
                            String::new()
                        } else {
                            format!(" — {}", s.name)
                        };
                        store.notify(format!("gpu {:.0}%{name}", s.util_pct));
                    }
                    GpuMeter::Unsupported(why) => {
                        store.notify(format!("GPU metrics unavailable: {why}"));
                    }
                    _ => {}
                }
                store.gpu.set(meter);
            });
        }
        if unsupported {
            return; // honest stop: the host said no — never a fake meter
        }
        // Sliced sleep: react to stop/cadence flips within ~250ms.
        let period_s = if FAST.load(Ordering::Relaxed) {
            POLL_ACTIVE_S
        } else {
            POLL_IDLE_S
        };
        let mut slept = 0u64;
        while slept < period_s * 1000 {
            if GEN.load(Ordering::SeqCst) != my_gen {
                return;
            }
            std::thread::sleep(Duration::from_millis(SLICE_MS));
            slept += SLICE_MS;
            // A cadence SPEED-UP mid-sleep applies now (a run just
            // started; the meter should tighten without waiting out a
            // 30s idle period).
            if FAST.load(Ordering::Relaxed) && slept >= POLL_ACTIVE_S * 1000 {
                break;
            }
        }
    }
}

/// Fold one `/host/metrics/gpu` response into meter state. Pure; the
/// live shape (verified 2026-07-22): `{ts, supported, source,
/// utilization_gpu_pct, gpus: [{index, name, utilization_gpu_pct, …}]}`.
pub fn meter_from_response(v: &Value) -> GpuMeter {
    let supported = v.get("supported").and_then(Value::as_bool).unwrap_or(false);
    if !supported {
        let source = v.get("source").and_then(Value::as_str).unwrap_or("");
        return GpuMeter::Unsupported(if source.is_empty() {
            "host reports no GPU metrics".to_string()
        } else {
            format!("host reports no GPU metrics ({source})")
        });
    }
    let first_gpu = v
        .get("gpus")
        .and_then(Value::as_array)
        .and_then(|g| g.first());
    let util = v
        .get("utilization_gpu_pct")
        .and_then(Value::as_f64)
        .or_else(|| {
            first_gpu
                .and_then(|g| g.get("utilization_gpu_pct"))
                .and_then(Value::as_f64)
        });
    match util {
        Some(pct) => GpuMeter::Ready(GpuSample {
            util_pct: pct.clamp(0.0, 100.0),
            name: first_gpu
                .and_then(|g| g.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        }),
        // supported:true but no utilization field = shape drift; stop
        // (Unsupported) rather than spam-poll a response we cannot read.
        None => GpuMeter::Unsupported("metrics shape unrecognized (no utilization_gpu_pct)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn live_shape_parses_to_ready() {
        // The exact live response captured 2026-07-22 (Apple M5 Max).
        let v = json!({
            "ts": "2026-07-22T22:23:22.371875+00:00",
            "supported": true,
            "source": "ioreg",
            "utilization_gpu_pct": 21.0,
            "gpus": [{"index": 0, "name": "Apple M5 Max",
                       "utilization_gpu_pct": 21.0,
                       "renderer_utilization_pct": 21.0,
                       "tiler_utilization_pct": 18.0}]
        });
        assert_eq!(
            meter_from_response(&v),
            GpuMeter::Ready(GpuSample {
                util_pct: 21.0,
                name: "Apple M5 Max".into()
            })
        );
    }

    #[test]
    fn unsupported_is_honest_and_named() {
        let v = json!({"supported": false, "source": "none"});
        match meter_from_response(&v) {
            GpuMeter::Unsupported(why) => assert!(why.contains("none"), "{why}"),
            other => panic!("expected Unsupported, got {other:?}"),
        }
        // Missing `supported` reads as unsupported (never a fake meter).
        match meter_from_response(&json!({})) {
            GpuMeter::Unsupported(_) => {}
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn utilization_falls_back_to_first_gpu_and_clamps() {
        // Top-level field absent → the first GPU's own number serves.
        let v = json!({
            "supported": true,
            "gpus": [{"name": "X", "utilization_gpu_pct": 130.5}]
        });
        assert_eq!(
            meter_from_response(&v),
            GpuMeter::Ready(GpuSample {
                util_pct: 100.0,
                name: "X".into()
            })
        );
        // supported:true with NO readable utilization = shape drift →
        // Unsupported (stop), never a fabricated 0%.
        let drift = json!({"supported": true, "gpus": []});
        assert!(matches!(
            meter_from_response(&drift),
            GpuMeter::Unsupported(_)
        ));
    }
}
