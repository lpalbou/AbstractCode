//! Host audio — this computer's speaker and microphone, through AbstractVoice.
//!
//! A terminal cannot play or record audio, and this crate ships no audio
//! engine. Speech is synthesised and transcribed on the GATEWAY; what runs
//! here is only the last metre: play the gateway's WAV segments on the
//! chosen speaker, record the chosen microphone, list the devices. That is
//! AbstractVoice's job (`NonBlockingAudioPlayer`, its output-device list,
//! PortAudio capture), so the client runs `assets/voice_bridge.py` with the
//! Python that has AbstractVoice installed and talks to it in JSON lines.
//!
//! Which Python: `--voice-python <path>` when given; else the interpreter
//! next to the installed `abstractgateway` / `abstractvoice` commands (the
//! installer's tool environment, where AbstractVoice always is); else
//! `python3`. The first one whose bridge says `ready` wins; when none does,
//! the error names each one tried and why.
//!
//! The bridge starts on first use and stays warm (one process per app),
//! so a reply's first audio pays the interpreter start once per session.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value;

/// The bridge script (shipped in the crate; see its header for the protocol).
pub const BRIDGE_SOURCE: &str = include_str!("../assets/voice_bridge.py");

/// One event from the bridge (see `assets/voice_bridge.py`).
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    Started {
        gen: u64,
    },
    Done {
        gen: u64,
    },
    Stopped {
        gen: u64,
    },
    Level {
        gen: u64,
        rms: f32,
    },
    Recorded {
        gen: u64,
        path: String,
        duration_ms: u64,
        peak: f64,
    },
    Devices {
        gen: u64,
        output: Vec<(String, String)>,
        input: Vec<(String, String)>,
    },
    ToneDone {
        gen: u64,
    },
    Error {
        gen: u64,
        op: String,
        message: String,
    },
    /// The bridge process is gone (sent to every waiter).
    Exited(String),
}

impl HostEvent {
    /// Parse one bridge line; `None` for lines that are not events
    /// (`ready` is consumed by the spawner).
    pub fn parse(v: &Value) -> Option<HostEvent> {
        let gen = v.get("gen").and_then(Value::as_u64).unwrap_or(0);
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let pairs = |k: &str| -> Vec<(String, String)> {
            v.get(k)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|d| {
                            let id = d.get("id").and_then(Value::as_str)?;
                            let name = d.get("name").and_then(Value::as_str).unwrap_or(id);
                            Some((id.to_string(), name.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        Some(match v.get("event").and_then(Value::as_str)? {
            "started" => HostEvent::Started { gen },
            "done" => HostEvent::Done { gen },
            "stopped" => HostEvent::Stopped { gen },
            "level" => HostEvent::Level {
                gen,
                rms: v.get("rms").and_then(Value::as_f64).unwrap_or(0.0) as f32,
            },
            "recorded" => HostEvent::Recorded {
                gen,
                path: s("path"),
                duration_ms: v.get("duration_ms").and_then(Value::as_u64).unwrap_or(0),
                peak: v.get("peak").and_then(Value::as_f64).unwrap_or(0.0),
            },
            "devices" => HostEvent::Devices {
                gen,
                output: pairs("output"),
                input: pairs("input"),
            },
            "tone_done" => HostEvent::ToneDone { gen },
            "error" => HostEvent::Error {
                gen,
                op: s("op"),
                message: s("message"),
            },
            _ => return None,
        })
    }

    fn gen(&self) -> Option<u64> {
        match self {
            HostEvent::Started { gen }
            | HostEvent::Done { gen }
            | HostEvent::Stopped { gen }
            | HostEvent::Level { gen, .. }
            | HostEvent::Recorded { gen, .. }
            | HostEvent::Devices { gen, .. }
            | HostEvent::ToneDone { gen }
            | HostEvent::Error { gen, .. } => Some(*gen),
            HostEvent::Exited(_) => None,
        }
    }
}

/// Where events from a transport go.
pub type EventSink = Arc<dyn Fn(HostEvent) + Send + Sync>;

/// A running bridge (the Python process, or a test double).
pub trait Transport: Send {
    fn send(&mut self, line: &str) -> Result<(), String>;
    fn alive(&mut self) -> bool;
    fn kill(&mut self);
}

/// Starts a transport; `Err` is the sentence shown to the user.
pub type Factory = Box<dyn Fn(EventSink) -> Result<Box<dyn Transport>, String> + Send + Sync>;

/// The app's host-audio handle: one bridge, generation-routed events.
pub struct Host {
    factory: Factory,
    transport: Mutex<Option<Box<dyn Transport>>>,
    /// Held while a bridge starts (seconds), so the quick `transport` lock
    /// is never held across a spawn — a UI-thread Stop must not wait.
    spawning: Mutex<()>,
    routes: Arc<Mutex<HashMap<u64, Sender<HostEvent>>>>,
    gen: AtomicU64,
    /// The generation allowed to speak right now (0 = none). A reply whose
    /// generation is no longer current stops forwarding segments.
    speech: AtomicU64,
}

impl Host {
    pub fn new(factory: Factory) -> Host {
        Host {
            factory,
            transport: Mutex::new(None),
            spawning: Mutex::new(()),
            routes: Arc::new(Mutex::new(HashMap::new())),
            gen: AtomicU64::new(0),
            speech: AtomicU64::new(0),
        }
    }

    pub fn next_gen(&self) -> u64 {
        self.gen.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Make `gen` the reply allowed to speak (any earlier one stops forwarding).
    pub fn claim_speech(&self, gen: u64) {
        self.speech.store(gen, Ordering::SeqCst);
    }

    pub fn is_current_speech(&self, gen: u64) -> bool {
        self.speech.load(Ordering::SeqCst) == gen
    }

    /// Stop whatever is speaking: the queue is cleared at once on the bridge.
    /// Returns false when nothing was speaking. Never starts the bridge.
    pub fn stop_speech(&self) -> bool {
        let gen = self.speech.swap(0, Ordering::SeqCst);
        if gen == 0 {
            return false;
        }
        let mut slot = self.transport.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = slot.as_mut() {
            let _ = t.send(&serde_json::json!({"op": "stop", "gen": gen}).to_string());
        }
        true
    }

    pub fn subscribe(&self, gen: u64) -> Receiver<HostEvent> {
        let (tx, rx) = channel();
        self.routes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(gen, tx);
        rx
    }

    pub fn unsubscribe(&self, gen: u64) {
        self.routes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&gen);
    }

    /// Start the bridge if it is not running (blocking; never on the UI thread).
    pub fn ensure(&self) -> Result<(), String> {
        let _spawn = self.spawning.lock().unwrap_or_else(|e| e.into_inner());
        {
            let mut slot = self.transport.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(t) = slot.as_mut() {
                if t.alive() {
                    return Ok(());
                }
                *slot = None;
            }
        }
        let routes = self.routes.clone();
        let sink: EventSink = Arc::new(move |ev: HostEvent| {
            let routes = routes.lock().unwrap_or_else(|e| e.into_inner());
            match ev.gen() {
                Some(g) => {
                    if let Some(tx) = routes.get(&g) {
                        let _ = tx.send(ev);
                    }
                }
                None => {
                    for tx in routes.values() {
                        let _ = tx.send(ev.clone());
                    }
                }
            }
        });
        let started = (self.factory)(sink)?;
        *self.transport.lock().unwrap_or_else(|e| e.into_inner()) = Some(started);
        Ok(())
    }

    /// Send one command (JSON) to the bridge, starting it when needed.
    pub fn send(&self, cmd: &Value) -> Result<(), String> {
        self.ensure()?;
        let mut slot = self.transport.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_mut() {
            Some(t) => t.send(&cmd.to_string()),
            None => Err("the audio bridge is not running".into()),
        }
    }

    /// Send only when the bridge already runs (UI-thread callers: never wait
    /// on a starting bridge). Returns whether it was sent.
    pub fn send_if_running(&self, cmd: &Value) -> bool {
        let mut slot = self.transport.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_mut() {
            Some(t) => t.send(&cmd.to_string()).is_ok(),
            None => false,
        }
    }

    /// End `gen`'s claim on speech (no-op when a newer reply took over).
    pub fn release_speech(&self, gen: u64) {
        let _ = self
            .speech
            .compare_exchange(gen, 0, Ordering::SeqCst, Ordering::SeqCst);
    }

    pub fn shutdown(&self) {
        let mut slot = self.transport.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut t) = slot.take() {
            let _ = t.send(r#"{"op":"quit"}"#);
            t.kill();
        }
    }
}

static HOST: OnceLock<Mutex<Option<Arc<Host>>>> = OnceLock::new();
static PYTHON_FLAG: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn host_slot() -> &'static Mutex<Option<Arc<Host>>> {
    HOST.get_or_init(|| Mutex::new(None))
}

/// `--voice-python <path>` (launch flag; `None` = discover).
pub fn configure(python: Option<String>) {
    *PYTHON_FLAG
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = python;
}

/// Replace the host (tests install an in-process double).
pub fn install(host: Arc<Host>) {
    *host_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(host);
}

/// The app's host-audio handle (the real AbstractVoice bridge unless a test installed one).
pub fn host() -> Arc<Host> {
    let mut slot = host_slot().lock().unwrap_or_else(|e| e.into_inner());
    slot.get_or_insert_with(|| Arc::new(Host::new(Box::new(spawn_python_bridge))))
        .clone()
}

/// Stop the bridge process (app exit).
pub fn shutdown() {
    if let Some(h) = host_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        h.shutdown();
    }
}

// -- the Python bridge -------------------------------------------------------

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// Interpreters to try, in order (see the module doc).
pub fn python_candidates(flag: Option<&str>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    if let Some(f) = flag.filter(|f| !f.trim().is_empty()) {
        push(PathBuf::from(f));
        return out;
    }
    for tool in ["abstractgateway", "abstractvoice"] {
        if let Some(exe) = which(tool) {
            let real = std::fs::canonicalize(&exe).unwrap_or(exe);
            if let Some(dir) = real.parent() {
                for py in ["python3", "python"] {
                    let p = dir.join(py);
                    if p.is_file() {
                        push(p);
                        break;
                    }
                }
            }
        }
    }
    push(PathBuf::from("python3"));
    out
}

struct ProcessTransport {
    child: Child,
    stdin: ChildStdin,
}

impl Transport for ProcessTransport {
    fn send(&mut self, line: &str) -> Result<(), String> {
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("the audio bridge stopped ({e})"))
    }
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start `python -u -c <bridge> [extra…]` and wait for `ready` (or `fatal`).
pub fn spawn_bridge_with(
    python: &Path,
    extra: &[&str],
    sink: EventSink,
) -> Result<Box<dyn Transport>, String> {
    let mut child = Command::new(python)
        .arg("-u")
        .arg("-c")
        .arg(BRIDGE_SOURCE)
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", python.display()))?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let mut stderr = child.stderr.take().ok_or("no stderr")?;
    // Drain stderr (a full pipe would block the bridge); keep the tail for errors.
    let tail = Arc::new(Mutex::new(String::new()));
    let tail_w = tail.clone();
    std::thread::Builder::new()
        .name("voice-bridge-stderr".into())
        .spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut t = tail_w.lock().unwrap_or_else(|e| e.into_inner());
                t.push_str(&String::from_utf8_lossy(&buf[..n]));
                let len = t.len();
                if len > 2000 {
                    let cut = t
                        .char_indices()
                        .map(|(i, _)| i)
                        .find(|i| *i >= len - 2000)
                        .unwrap_or(0);
                    t.replace_range(..cut, "");
                }
            }
        })
        .map_err(|e| e.to_string())?;
    let (ready_tx, ready_rx) = channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("voice-bridge-events".into())
        .spawn(move || {
            let mut ready = Some(ready_tx);
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
                    continue;
                };
                match v.get("event").and_then(Value::as_str) {
                    Some("ready") => {
                        if let Some(tx) = ready.take() {
                            let _ = tx.send(Ok(()));
                        }
                    }
                    Some("fatal") => {
                        let msg = v
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        if let Some(tx) = ready.take() {
                            let _ = tx.send(Err(msg));
                        }
                    }
                    _ => {
                        if let Some(ev) = HostEvent::parse(&v) {
                            sink(ev);
                        }
                    }
                }
            }
            if let Some(tx) = ready.take() {
                let _ = tx.send(Err("the bridge exited before it was ready".into()));
            }
            sink(HostEvent::Exited(
                "the audio bridge (AbstractVoice) stopped".into(),
            ));
        })
        .map_err(|e| e.to_string())?;
    match ready_rx.recv_timeout(Duration::from_secs(20)) {
        Ok(Ok(())) => Ok(Box::new(ProcessTransport { child, stdin })),
        Ok(Err(msg)) => {
            let _ = child.kill();
            let _ = child.wait();
            let tail = tail
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .trim()
                .to_string();
            let last = tail.lines().last().unwrap_or("").to_string();
            Err(format!(
                "{}: {}",
                python.display(),
                if msg.is_empty() { last } else { msg }
            ))
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!(
                "{}: the bridge did not start within 20 s",
                python.display()
            ))
        }
    }
}

/// The production factory: the first interpreter whose bridge is ready.
fn spawn_python_bridge(sink: EventSink) -> Result<Box<dyn Transport>, String> {
    let flag = PYTHON_FLAG
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let mut reasons = Vec::new();
    for py in python_candidates(flag.as_deref()) {
        match spawn_bridge_with(&py, &[], sink.clone()) {
            Ok(t) => return Ok(t),
            Err(e) => reasons.push(e),
        }
    }
    Err(bridge_missing_sentence(&reasons))
}

/// The sentence shown when no interpreter could run the bridge.
pub fn bridge_missing_sentence(reasons: &[String]) -> String {
    format!(
        "This computer's speaker and microphone need AbstractVoice (pip install abstractframework), or start with --voice-python <path> — tried {}",
        reasons.join("; ")
    )
}

/// Standard base64 (the bridge decodes it with `base64.b64decode`).
pub fn b64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(b64_encode(b""), "");
        assert_eq!(b64_encode(b"f"), "Zg==");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64_encode(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn events_parse_with_their_generation() {
        let ev = HostEvent::parse(
            &serde_json::json!({"event": "recorded", "gen": 4, "path": "/x.wav", "duration_ms": 1200, "peak": 0.3}),
        );
        assert_eq!(
            ev,
            Some(HostEvent::Recorded {
                gen: 4,
                path: "/x.wav".into(),
                duration_ms: 1200,
                peak: 0.3
            })
        );
        let ev = HostEvent::parse(
            &serde_json::json!({"event": "devices", "gen": 2, "output": [{"id": "uid-1", "name": "Speakers"}], "input": []}),
        );
        assert_eq!(
            ev,
            Some(HostEvent::Devices {
                gen: 2,
                output: vec![("uid-1".into(), "Speakers".into())],
                input: vec![]
            })
        );
        assert_eq!(
            HostEvent::parse(&serde_json::json!({"event": "ready"})),
            None
        );
    }

    #[test]
    fn a_launch_flag_is_the_only_candidate() {
        assert_eq!(
            python_candidates(Some("/opt/py/bin/python3")),
            vec![PathBuf::from("/opt/py/bin/python3")]
        );
        assert_eq!(
            python_candidates(None).last(),
            Some(&PathBuf::from("python3"))
        );
    }
}
