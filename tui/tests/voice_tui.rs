//! Voice in the terminal (round 7, R7.1): speak replies through the
//! gateway's streaming voice route, dictate through its default speech-to-
//! text route, and the `/voice` screen — all against a FAKE gateway (a local
//! HTTP listener on an ephemeral port; no model, no network) and a FAKE host
//! audio bridge (in-process; the real one is AbstractVoice in Python, run by
//! `voice_bridge_python_null_output` below, `--ignored`).
//!
//! What would go red if the feature were removed: the request seams
//! (`/voice/defaults`, `/runs/{id}/voice/tts/stream`, `/attachments/upload`
//! then `/runs/{id}/audio/transcribe`), the segment order reaching the
//! speaker, Esc stopping playback, the "Gateway default · supertonic / supertonic-3"
//! wording (never "openai"), and the transcript landing in the composer.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use abstracttui::app::Driver;
use abstracttui::prelude::*;
use abstracttui::testing::CaptureTerm;
use serde_json::{json, Value};

use abstractcode::config::Prefs;
use abstractcode::runner::Cmd;
use abstractcode::store::Store;
use abstractcode::transcript::Item;
use abstractcode::ui::{self, UiCtx};
use abstractcode::voice::{self, VoiceGateway, VoicePrefs};
use abstractcode::voice_host::{self, EventSink, Host, HostEvent, Transport};

/// Tests here share the process-wide host-audio slot: one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

// -- fake gateway ------------------------------------------------------------

/// A tiny WAV (PCM16 mono 16 kHz) carrying `n` as its only sample value, so
/// segment order is visible in what reaches the speaker.
fn wav(n: i16) -> Vec<u8> {
    let samples: Vec<i16> = vec![n; 160];
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes());
    out.extend_from_slice(&32000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

#[derive(Clone)]
struct FakeGateway {
    url: String,
    /// (method, path, body) of every request, in order.
    log: Arc<Mutex<Vec<(String, String, String)>>>,
}

struct GatewayScript {
    /// Delay before each spoken segment.
    segment_delay: Duration,
    segments: Vec<i16>,
    /// `Some((status, detail))` = the TTS route refuses.
    tts_refusal: Option<(u16, String)>,
    defaults: Value,
}

impl Default for GatewayScript {
    fn default() -> Self {
        GatewayScript {
            segment_delay: Duration::from_millis(20),
            segments: vec![1, 2, 3],
            tts_refusal: None,
            defaults: json!({
                "tts": {"route": "output.voice", "configured": true, "provider": "supertonic", "model": "supertonic-3", "voice": "M3"},
                "stt": {"route": "input.voice", "configured": true, "provider": "faster-whisper", "model": "large-v3"},
                "source": "capability_defaults"
            }),
        }
    }
}

fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn serve(script: GatewayScript) -> FakeGateway {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let log: Arc<Mutex<Vec<(String, String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let script = Arc::new(script);
    let log_w = log.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut stream) = conn else { continue };
            let log = log_w.clone();
            let script = script.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                let body_s = String::from_utf8_lossy(&body).to_string();
                log.lock()
                    .unwrap()
                    .push((method.clone(), path.clone(), body_s));
                let p = path.split('?').next().unwrap_or("");
                if p == "/api/gateway/voice/defaults" {
                    respond(&mut stream, 200, &script.defaults.to_string());
                } else if p == "/api/gateway/voice/voices" {
                    let cat = json!({
                        "items": [{"id": "M3", "label": "M3", "provider": "supertonic", "model": "supertonic-3", "voice_kind": "profile"}],
                        "stt_providers": ["faster-whisper"],
                        "stt_models_by_provider": {"faster-whisper": ["large-v3", "small"]},
                        "active_tts_provider": "supertonic"
                    });
                    respond(&mut stream, 200, &cat.to_string());
                } else if p.ends_with("/voice/tts/stream") {
                    if let Some((status, detail)) = &script.tts_refusal {
                        respond(&mut stream, *status, &json!({"detail": detail}).to_string());
                        return;
                    }
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n"
                    );
                    let _ = stream.flush();
                    let _ = writeln!(
                        stream,
                        "{}",
                        json!({"type": "start", "child_run_id": "tts-child"})
                    );
                    for n in &script.segments {
                        std::thread::sleep(script.segment_delay);
                        let ev = json!({"type": "segment", "audio_b64": voice_host::b64_encode(&wav(*n))});
                        if writeln!(stream, "{ev}").is_err() || stream.flush().is_err() {
                            return;
                        }
                    }
                    let _ = writeln!(
                        stream,
                        "{}",
                        json!({"type": "done", "metrics": {"ttfb_s": 0.4, "device": "mps"}})
                    );
                    let _ = stream.flush();
                } else if p == "/api/gateway/attachments/upload" {
                    respond(
                        &mut stream,
                        200,
                        &json!({"attachment": {"$artifact": "att-1", "artifact_id": "att-1", "content_type": "audio/wav"}}).to_string(),
                    );
                } else if p.ends_with("/audio/transcribe") {
                    respond(
                        &mut stream,
                        200,
                        &json!({"ok": true, "text": "hello from the microphone", "provider": "faster-whisper", "model": "large-v3", "duration_ms": 900}).to_string(),
                    );
                } else {
                    respond(&mut stream, 404, r#"{"detail":"not in the fake"}"#);
                }
            });
        }
    });
    FakeGateway { url, log }
}

impl FakeGateway {
    fn requests(&self, needle: &str) -> Vec<(String, String, String)> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, p, _)| p.contains(needle))
            .cloned()
            .collect()
    }
}

// -- fake host audio -----------------------------------------------------------

#[derive(Default)]
struct HostLog {
    /// Every command line the app sent, parsed.
    commands: Vec<Value>,
}

struct FakeTransport {
    sink: EventSink,
    log: Arc<Mutex<HostLog>>,
    started: Vec<u64>,
    recording: Option<(u64, String)>,
    silent: bool,
}

impl Transport for FakeTransport {
    fn send(&mut self, line: &str) -> Result<(), String> {
        let cmd: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        self.log.lock().unwrap().commands.push(cmd.clone());
        let gen = cmd.get("gen").and_then(Value::as_u64).unwrap_or(0);
        let sink = self.sink.clone();
        match cmd.get("op").and_then(Value::as_str).unwrap_or("") {
            "play" => {
                if !self.started.contains(&gen) {
                    self.started.push(gen);
                    sink(HostEvent::Started { gen });
                }
            }
            "end" => {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(30));
                    sink(HostEvent::Done { gen });
                });
            }
            "stop" => sink(HostEvent::Stopped { gen }),
            "devices" => sink(HostEvent::Devices {
                gen,
                output: vec![("uid-speakers".into(), "Desk Speakers".into())],
                input: vec![("Studio Mic".into(), "Studio Mic".into())],
            }),
            "tone" => sink(HostEvent::ToneDone { gen }),
            "record" => {
                let path = cmd
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                self.recording = Some((gen, path));
                sink(HostEvent::Level { gen, rms: 0.4 });
                if let Some(max) = cmd.get("max_s").and_then(Value::as_u64).filter(|m| *m <= 3) {
                    // The 3 s microphone test stops by itself.
                    let _ = max;
                    self.finish_recording(gen);
                }
            }
            "record_stop" => self.finish_recording(gen),
            _ => {}
        }
        Ok(())
    }
    fn alive(&mut self) -> bool {
        true
    }
    fn kill(&mut self) {}
}

impl FakeTransport {
    fn finish_recording(&mut self, gen: u64) {
        if let Some((g, path)) = self.recording.take() {
            if g == gen {
                std::fs::write(&path, wav(if self.silent { 0 } else { 9000 })).unwrap();
                (self.sink)(HostEvent::Recorded {
                    gen,
                    path,
                    duration_ms: 1500,
                    peak: if self.silent { 0.0 } else { 0.27 },
                });
            }
        }
    }
}

fn fake_host(silent: bool) -> (Arc<Host>, Arc<Mutex<HostLog>>, Arc<AtomicUsize>) {
    let log = Arc::new(Mutex::new(HostLog::default()));
    let spawns = Arc::new(AtomicUsize::new(0));
    let (l, s) = (log.clone(), spawns.clone());
    let host = Arc::new(Host::new(Box::new(move |sink| {
        s.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeTransport {
            sink,
            log: l.clone(),
            started: Vec::new(),
            recording: None,
            silent,
        }) as Box<dyn Transport>)
    })));
    voice_host::install(host.clone());
    (host, log, spawns)
}

fn played(log: &Arc<Mutex<HostLog>>) -> Vec<String> {
    log.lock()
        .unwrap()
        .commands
        .iter()
        .filter(|c| c.get("op").and_then(Value::as_str) == Some("play"))
        .map(|c| {
            c.get("b64")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

fn ops(log: &Arc<Mutex<HostLog>>) -> Vec<String> {
    log.lock()
        .unwrap()
        .commands
        .iter()
        .map(|c| {
            c.get("op")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

// -- core lane -------------------------------------------------------------------

#[test]
fn speak_streams_each_segment_to_the_speaker_in_order() {
    let _g = serial();
    let gw = serve(GatewayScript::default());
    let (host, log, _) = fake_host(false);
    let client = abstractcode::gateway::GatewayClient::new(&gw.url, Some("tok"));
    let vg = VoiceGateway::new(&client);
    let gen = host.next_gen();
    host.claim_speech(gen);
    let mut started = None;
    let reply = voice::speak_blocking(
        &host,
        &vg,
        gen,
        "run-1",
        "Hello there. Second sentence.",
        &VoicePrefs::default(),
        &mut |d| started = Some(d),
    )
    .expect("spoken");
    assert!(!reply.stopped);
    assert!(
        started.is_some() && reply.first_audio.is_some(),
        "first audio is measured"
    );
    assert_eq!(reply.metrics.get("device"), Some(&json!("mps")));
    let expected: Vec<String> = [1, 2, 3]
        .iter()
        .map(|n| voice_host::b64_encode(&wav(*n)))
        .collect();
    assert_eq!(
        played(&log),
        expected,
        "every segment, in order, as it arrives"
    );
    assert_eq!(ops(&log).last().map(String::as_str), Some("end"));
    // The gateway default: the request names no engine (the gateway fills output.voice).
    let req = gw.requests("/runs/run-1/voice/tts/stream");
    assert_eq!(req.len(), 1);
    let body: Value = serde_json::from_str(&req[0].2).unwrap();
    assert_eq!(
        body.get("text"),
        Some(&json!("Hello there. Second sentence."))
    );
    for k in [
        "provider",
        "model",
        "voice",
        "profile",
        "output_device",
        "read_aloud",
    ] {
        assert!(
            body.get(k).is_none(),
            "{k} must not ride a gateway-default request: {body}"
        );
    }
    assert_eq!(
        voice::reply_line(&reply).split(" · ").nth(1),
        Some("engine on mps")
    );
}

#[test]
fn an_override_rides_the_request_and_a_stop_ends_forwarding() {
    let _g = serial();
    let gw = serve(GatewayScript {
        segment_delay: Duration::from_millis(150),
        ..Default::default()
    });
    let (host, log, _) = fake_host(false);
    let client = abstractcode::gateway::GatewayClient::new(&gw.url, None);
    let vg = VoiceGateway::new(&client);
    let gen = host.next_gen();
    host.claim_speech(gen);
    let prefs = VoicePrefs {
        provider: "piper".into(),
        voice: "amy".into(),
        ..Default::default()
    };
    let h2 = host.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(220)); // after the first segment
        h2.stop_speech()
    });
    let reply = voice::speak_blocking(&host, &vg, gen, "run-2", "Long reply.", &prefs, &mut |_| {})
        .expect("ok");
    assert!(stopper.join().unwrap(), "something was speaking");
    assert!(reply.stopped);
    assert_eq!(
        played(&log).len(),
        1,
        "no segment after the stop: {:?}",
        ops(&log)
    );
    assert!(
        ops(&log).contains(&"stop".to_string()),
        "the bridge is told to stop at once"
    );
    let body: Value = serde_json::from_str(&gw.requests("/voice/tts/stream")[0].2).unwrap();
    assert_eq!(body.get("provider"), Some(&json!("piper")));
    assert_eq!(body.get("voice"), Some(&json!("amy")));
}

#[test]
fn a_refused_stream_is_one_sentence() {
    let _g = serial();
    let gw = serve(GatewayScript {
        tts_refusal: Some((
            503,
            "Gateway runtime does not expose streaming voice synthesis.".into(),
        )),
        ..Default::default()
    });
    let (host, _log, _) = fake_host(false);
    let vg = VoiceGateway::new(&abstractcode::gateway::GatewayClient::new(&gw.url, None));
    let gen = host.next_gen();
    host.claim_speech(gen);
    let err = voice::speak_blocking(
        &host,
        &vg,
        gen,
        "run-3",
        "x",
        &VoicePrefs::default(),
        &mut |_| {},
    )
    .unwrap_err();
    assert_eq!(
        voice::error_sentence("Reading aloud failed", &err),
        "Reading aloud failed: Gateway runtime does not expose streaming voice synthesis."
    );
}

#[test]
fn dictation_uploads_the_recording_and_uses_the_default_route() {
    let _g = serial();
    let gw = serve(GatewayScript::default());
    let (host, _log, _) = fake_host(false);
    let vg = VoiceGateway::new(&abstractcode::gateway::GatewayClient::new(&gw.url, None));
    let gen = host.next_gen();
    let h2 = host.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        h2.send_if_running(&json!({"op": "record_stop", "gen": gen}));
    });
    let mut levels = Vec::new();
    let rec = voice::record_blocking(&host, gen, &VoicePrefs::default(), 120, &mut |l| {
        levels.push(l)
    })
    .expect("recorded");
    assert_eq!(levels, vec![0.4]);
    let bytes = std::fs::read(&rec.path).unwrap();
    let _ = std::fs::remove_file(&rec.path);
    let t = vg
        .transcribe("sess-1", "run-4", &bytes, &VoicePrefs::default())
        .expect("transcribed");
    assert_eq!(t.text, "hello from the microphone");
    assert_eq!(
        voice::route_text(&t.provider, &t.model),
        "faster-whisper / large-v3"
    );
    assert_eq!(gw.requests("/attachments/upload").len(), 1);
    let body: Value =
        serde_json::from_str(&gw.requests("/runs/run-4/audio/transcribe")[0].2).unwrap();
    assert_eq!(
        body.pointer("/audio_artifact/artifact_id"),
        Some(&json!("att-1"))
    );
    assert!(
        body.get("provider").is_none() && body.get("model").is_none(),
        "gateway default = no route in the request"
    );
    // An override rides the request; a language never does (round 18: the
    // gateway applies the account's spoken language).
    let o = VoicePrefs {
        stt_provider: "faster-whisper".into(),
        stt_model: "small".into(),
        ..Default::default()
    };
    vg.transcribe("sess-1", "run-4", &bytes, &o).unwrap();
    let body: Value = serde_json::from_str(&gw.requests("/audio/transcribe")[1].2).unwrap();
    assert_eq!(
        (
            body.get("provider"),
            body.get("model"),
            body.get("language")
        ),
        (Some(&json!("faster-whisper")), Some(&json!("small")), None)
    );
}

#[test]
fn a_missing_abstractvoice_is_named_with_the_way_out() {
    let _g = serial();
    let host = Arc::new(Host::new(Box::new(|_sink| {
        Err(voice_host::bridge_missing_sentence(&[
            "/usr/bin/python3: ModuleNotFoundError: No module named 'abstractvoice'".into(),
        ]))
    })));
    let err = host.ensure().unwrap_err();
    assert!(
        err.contains("need AbstractVoice")
            && err.contains("--voice-python")
            && err.contains("No module named 'abstractvoice'"),
        "{err}"
    );
}

// -- the real bridge (Python + AbstractVoice), null output ---------------------------

/// Runs `assets/voice_bridge.py` with `python3` (AbstractVoice + numpy
/// installed) in `--null-output` mode: segments are written to files in real
/// time, a recording is a tone. `cargo test --test voice_tui -- --ignored`.
#[test]
#[ignore = "needs python3 with abstractvoice + numpy"]
fn voice_bridge_python_null_output() {
    let _g = serial();
    let dir = std::env::temp_dir().join(format!("acode-voice-bridge-{}", std::process::id()));
    let dir_s = dir.to_string_lossy().to_string();
    let (tx, rx) = mpsc::channel::<HostEvent>();
    let tx = Mutex::new(tx);
    let sink: EventSink = Arc::new(move |ev| {
        let _ = tx.lock().unwrap().send(ev);
    });
    let py = std::path::PathBuf::from(
        std::env::var("ACODE_TEST_PYTHON").unwrap_or_else(|_| "python3".into()),
    );
    let mut t =
        voice_host::spawn_bridge_with(&py, &["--null-output", &dir_s], sink).expect("bridge ready");
    for n in [1i16, 2] {
        t.send(&json!({"op": "play", "gen": 7, "b64": voice_host::b64_encode(&wav(n)), "device": "", "volume": 1.0}).to_string()).unwrap();
    }
    t.send(&json!({"op": "end", "gen": 7}).to_string()).unwrap();
    let mut got = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(ev) = rx.recv_timeout(Duration::from_millis(200)) {
            let done = matches!(ev, HostEvent::Done { .. });
            got.push(ev);
            if done {
                break;
            }
        }
    }
    assert_eq!(got.first(), Some(&HostEvent::Started { gen: 7 }), "{got:?}");
    assert_eq!(got.last(), Some(&HostEvent::Done { gen: 7 }), "{got:?}");
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        2,
        "two segments played"
    );
    let wav_path = dir.join("rec.wav");
    t.send(
        &json!({"op": "record", "gen": 8, "path": wav_path.to_string_lossy(), "max_s": 0})
            .to_string(),
    )
    .unwrap();
    t.send(&json!({"op": "record_stop", "gen": 8}).to_string())
        .unwrap();
    let rec = loop {
        match rx.recv_timeout(Duration::from_secs(10)).expect("recorded") {
            HostEvent::Recorded {
                gen,
                duration_ms,
                peak,
                ..
            } => break (gen, duration_ms, peak),
            _ => continue,
        }
    };
    assert_eq!(rec.0, 8);
    assert!(rec.1 >= 900 && rec.2 > 0.2, "{rec:?}");
    t.send(r#"{"op":"devices","gen":9}"#).unwrap();
    let dev = loop {
        if let HostEvent::Devices { output, .. } =
            rx.recv_timeout(Duration::from_secs(10)).expect("devices")
        {
            break output;
        }
    };
    assert_eq!(
        dev,
        vec![("null-speaker".to_string(), "Null speaker".to_string())]
    );
    t.kill();
    let _ = std::fs::remove_dir_all(&dir);
}

// -- the real interface (AbstractTUI capture harness) ----------------------------------

struct Harness {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
    prefs: Rc<RefCell<Prefs>>,
    _rx: mpsc::Receiver<Cmd>,
}

fn harness(gateway_url: &str) -> Harness {
    abstracttui::app::set_theme_by_id("abstract-dark");
    let size = Size::new(110, 34);
    let mut app = App::new(size);
    let overlays = app.overlays();
    let quitter = app.quitter();
    let (tx, rx) = mpsc::channel::<Cmd>();
    let store_slot: Rc<RefCell<Option<Store>>> = Rc::new(RefCell::new(None));
    let store_out = store_slot.clone();
    let prefs = Rc::new(RefCell::new(Prefs::default()));
    let prefs_for_ctx = prefs.clone();
    let actions = app.actions();
    let url = gateway_url.to_string();
    app.mount(move |cx| {
        let store = Store::create(cx);
        *store_out.borrow_mut() = Some(store);
        store.session_id.set("acode-voice-session".into());
        let ctx = UiCtx {
            tx,
            client: abstractcode::gateway::GatewayClient::new(&url, Some("test-token")),
            overlays: overlays.clone(),
            quitter: quitter.clone(),
            prefs: prefs_for_ctx.clone(),
            workspace_root: Some("/tmp/ws".into()),
            max_iterations_explicit: false,
            max_iterations: 50,
            no_project_context: true,
            no_prompt_cache: false,
            replay_turns: 20,
            gateway_label: "127.0.0.1:18731".into(),
            modal: Rc::new(RefCell::new(None)),
            modal_epoch: cx.signal(0u64),
            dismissed_wait: Rc::new(RefCell::new(None)),
            wait_modal_for: Rc::new(RefCell::new(None)),
        };
        ui::root(cx, store, ctx, &actions)
    })
    .expect("mount");
    let mut term = CaptureTerm::new(size);
    let cfg = RunConfig {
        probe: false,
        caps: Some(abstracttui::term::Capabilities::with(|c| {
            c.truecolor = true;
            c.colors_256 = true;
            c.unicode_ok = true;
        })),
        ..RunConfig::default()
    };
    let driver = Driver::new(&mut app, &mut term, cfg).expect("driver");
    let store = store_slot.borrow().expect("store");
    Harness {
        app,
        term,
        driver,
        store,
        prefs,
        _rx: rx,
    }
}

impl Harness {
    fn turn(&mut self) -> String {
        self.driver
            .turn(&mut self.app, &mut self.term)
            .expect("turn");
        self.term.screen().to_text()
    }
    fn type_text(&mut self, s: &str) {
        self.term.push_input(s.as_bytes());
    }
    /// Turn until `pred(screen)` holds (background threads post into the loop).
    fn until(&mut self, what: &str, pred: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let s = self.turn();
            if pred(&s) {
                return s;
            }
            if Instant::now() > deadline {
                panic!("timed out waiting for {what}; screen:\n{s}");
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    fn escape(&mut self) {
        self.term.push_input(&[0x1b]);
        self.turn();
        std::thread::sleep(Duration::from_millis(45));
        self.turn();
    }
    /// Leave the splash and give the conversation a reply + a run.
    fn with_reply(&mut self, reply: &str) {
        self.store.run_id.set("run-ui".into());
        self.store.fold.update(|f| {
            f.push_item(Item::User {
                text: "say something".into(),
            });
            f.push_item(Item::Assistant {
                text: reply.into(),
                final_answer: true,
            });
        });
        for _ in 0..3 {
            self.turn();
        }
    }
}

#[test]
fn voice_screen_names_the_gateway_routes_and_lists_host_devices() {
    let _g = serial();
    let gw = serve(GatewayScript::default());
    let (_host, _log, _) = fake_host(false);
    let mut h = harness(&gw.url);
    h.with_reply("ok");
    h.type_text("/voice\r");
    let s = h.until("the gateway defaults on the voice screen", |s| {
        s.contains("supertonic / supertonic-3") && s.contains("faster-whisper / large-v3")
    });
    assert!(
        s.contains("Text → speech     Gateway default · supertonic / supertonic-3"),
        "{s}"
    );
    assert!(
        s.contains("Speech → text     Gateway default · faster-whisper / large-v3"),
        "{s}"
    );
    assert!(
        !s.to_lowercase().contains("openai"),
        "openai must never appear unless it IS the route:\n{s}"
    );
    for row in [
        "Output device",
        "Test speaker",
        "Reply volume",
        "Input device",
        "Test microphone",
        "Spoken language",
        "Input level",
        "[ ] Read aloud — Speak each new reply.",
        "Voice latency",
    ] {
        assert!(s.contains(row), "missing {row:?}:\n{s}");
    }
    assert_eq!(
        gw.requests("/voice/defaults").len(),
        1,
        "ONE answer for the defaults"
    );
    // Devices come from the host bridge (AbstractVoice); pick the speaker.
    // Rows: 0 Engines, 1 TTS, 2 STT, 3 Output, 4 Output device.
    h.term.push_input(b"\x1b[B\x1b[B\x1b[B");
    h.turn();
    h.type_text("\r");
    let s = h.until("the output device picker", |s| s.contains("Desk Speakers"));
    assert!(s.contains("System default"), "{s}");
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.type_text("\r");
    let s = h.until("the chosen speaker on the voice screen", |s| {
        s.contains("Output device     Desk Speakers")
    });
    assert!(s.contains("Voice ·"), "{s}");
    let saved = h.prefs.borrow().voice.clone().expect("saved");
    assert_eq!(saved.get("output_device"), Some(&json!("uid-speakers")));
}

#[test]
fn read_aloud_switch_persists_from_the_command() {
    let _g = serial();
    let gw = serve(GatewayScript::default());
    let (_host, _log, _) = fake_host(false);
    let mut h = harness(&gw.url);
    h.with_reply("ok");
    h.type_text("/voice read-aloud on\r");
    h.until("the switch notice", |s| s.contains("Read aloud [x]"));
    assert_eq!(
        h.prefs
            .borrow()
            .voice
            .as_ref()
            .and_then(|v| v.get("read_aloud")),
        Some(&json!(true))
    );
    h.type_text("/voice\r");
    let s = h.until("the switch on the screen", |s| {
        s.contains("[x] Read aloud — Speak each new reply.")
    });
    assert!(s.contains("Voice ·"));
}

#[test]
fn ctrl_p_speaks_the_latest_reply_and_esc_stops_it() {
    let _g = serial();
    let gw = serve(GatewayScript {
        segment_delay: Duration::from_millis(300),
        segments: vec![1, 2, 3, 4, 5, 6],
        ..Default::default()
    });
    let (_host, log, _) = fake_host(false);
    let mut h = harness(&gw.url);
    h.with_reply("The answer is forty-two. That is all.");
    h.term.push_input(&[0x10]); // Ctrl+P
    let s = h.until("speaking", |s| s.contains("♪ Speaking… · Esc stops"));
    assert!(!s.contains("Preparing"), "{s}");
    let body: Value =
        serde_json::from_str(&gw.requests("/runs/run-ui/voice/tts/stream")[0].2).unwrap();
    assert_eq!(
        body.get("text"),
        Some(&json!("The answer is forty-two. That is all."))
    );
    h.escape();
    let s = h.until("speech stopped", |s| !s.contains("♪"));
    assert!(
        ops(&log).contains(&"stop".to_string()),
        "Esc tells the speaker to stop: {:?}",
        ops(&log)
    );
    let n = played(&log).len();
    std::thread::sleep(Duration::from_millis(700));
    h.turn();
    assert_eq!(played(&log).len(), n, "nothing plays after Esc");
    // Esc was consumed by the stop: the draft area is untouched, no cancel armed.
    assert!(!s.contains("press Esc again"), "{s}");
}

#[test]
fn a_refused_reply_reads_as_a_sentence_in_the_transcript() {
    let _g = serial();
    let gw = serve(GatewayScript {
        tts_refusal: Some((429, "openai quota exceeded".into())),
        ..Default::default()
    });
    let (_host, _log, _) = fake_host(false);
    let mut h = harness(&gw.url);
    h.with_reply("hi");
    h.type_text("/speak\r");
    h.until("the error sentence", |s| {
        s.contains("Reading aloud failed: openai quota exceeded.")
    });
}

#[test]
fn ctrl_r_dictates_into_the_composer_with_the_route_shown() {
    let _g = serial();
    let gw = serve(GatewayScript::default());
    let (_host, log, _) = fake_host(false);
    let mut h = harness(&gw.url);
    h.with_reply("ready");
    h.type_text("draft:");
    h.turn();
    h.term.push_input(&[0x12]); // Ctrl+R starts
    h.until("recording", |s| {
        s.contains("● Recording… 0 s · Ctrl+R transcribes · Esc cancels")
    });
    // The defaults land while recording: the Transcribing line names the route.
    h.until("defaults fetched", |_| {
        !gw.requests("/voice/defaults").is_empty()
    });
    h.turn();
    h.term.push_input(&[0x12]); // Ctrl+R stops → transcribe
    let s = h.until("the transcript in the composer", |s| {
        s.contains("draft: hello from the microphone")
    });
    assert!(
        !s.contains("Recording…") && !s.contains("Transcribing…"),
        "{s}"
    );
    assert!(ops(&log).contains(&"record_stop".to_string()));
    let body: Value =
        serde_json::from_str(&gw.requests("/runs/run-ui/audio/transcribe")[0].2).unwrap();
    assert!(
        body.get("provider").is_none(),
        "gateway default route: {body}"
    );
}

#[test]
fn transcribing_line_is_shown_while_the_gateway_works() {
    let _g = serial();
    let gw = serve(GatewayScript::default());
    let (_host, _log, _) = fake_host(false);
    let mut h = harness(&gw.url);
    h.with_reply("ready");
    // Defaults known first (the voice screen fetches them), then dictate.
    h.store.voice.defaults.set(abstractcode::voice::DefaultsState::Loaded(Box::new(abstractcode::voice::VoiceDefaults::from_json(
        &json!({"stt": {"configured": true, "provider": "faster-whisper", "model": "large-v3"}}),
    ))));
    h.store
        .voice
        .dictation
        .set(abstractcode::voice::Dictation::Transcribing {
            since: Instant::now() - Duration::from_secs(4),
            route: "faster-whisper / large-v3".into(),
        });
    let s = h.turn();
    assert!(
        s.contains("Transcribing… 4 s · faster-whisper / large-v3"),
        "{s}"
    );
}

#[test]
fn a_silent_recording_is_never_sent_and_says_why() {
    let _g = serial();
    let gw = serve(GatewayScript::default());
    let (_host, _log, _) = fake_host(true);
    let mut h = harness(&gw.url);
    h.with_reply("ready");
    h.term.push_input(&[0x12]);
    h.until("recording", |s| s.contains("● Recording…"));
    h.term.push_input(&[0x12]);
    h.until("the silence sentence", |s| {
        s.contains(
            "Nothing was heard. Check the microphone in Settings → Voice (Test), then try again.",
        )
    });
    assert!(
        gw.requests("/audio/transcribe").is_empty(),
        "silence is not uploaded"
    );
}

#[test]
fn esc_cancels_a_recording_and_nothing_is_transcribed() {
    let _g = serial();
    let gw = serve(GatewayScript::default());
    let (_host, _log, _) = fake_host(false);
    let mut h = harness(&gw.url);
    h.with_reply("ready");
    h.term.push_input(&[0x12]);
    h.until("recording", |s| s.contains("● Recording…"));
    h.escape();
    h.until("cancelled", |s| {
        s.contains("Recording cancelled.") && !s.contains("● Recording…")
    });
    std::thread::sleep(Duration::from_millis(200));
    h.turn();
    assert!(gw.requests("/audio/transcribe").is_empty());
}

#[test]
fn dictation_without_a_conversation_says_what_to_do() {
    let _g = serial();
    let gw = serve(GatewayScript::default());
    let (_host, _log, spawns) = fake_host(false);
    let mut h = harness(&gw.url);
    h.store.fold.update(|f| {
        f.push_item(Item::User {
            text: "settle".into(),
        })
    });
    h.turn();
    h.term.push_input(&[0x12]);
    h.until("the sentence", |s| {
        s.contains("Start a conversation to enable dictation.")
    });
    assert_eq!(
        spawns.load(Ordering::SeqCst),
        0,
        "no bridge for a refused dictation"
    );
}

/// Time to first audio against a REAL gateway, through the real bridge in
/// `--null-output` mode (the first segment reaching the player; no sound on
/// the machine). Read-only apart from the gateway's own TTS child run.
/// `ACODE_TTFA_URL=… ACODE_TTFA_TOKEN=… ACODE_TTFA_RUN=… ACODE_TEST_PYTHON=…
///  cargo test --test voice_tui ttfa_probe -- --ignored --nocapture`
#[test]
#[ignore = "probes a real gateway"]
fn ttfa_probe() {
    let _g = serial();
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is required"));
    let dir = std::env::temp_dir().join(format!("acode-ttfa-{}", std::process::id()));
    let dir_s = dir.to_string_lossy().to_string();
    let py = std::path::PathBuf::from(var("ACODE_TEST_PYTHON"));
    let host = Arc::new(Host::new(Box::new(move |sink| {
        voice_host::spawn_bridge_with(&py, &["--null-output", &dir_s], sink)
    })));
    host.ensure().expect("bridge"); // warm, as in a session that already spoke once
    let client = abstractcode::gateway::GatewayClient::new(
        &var("ACODE_TTFA_URL"),
        Some(&var("ACODE_TTFA_TOKEN")),
    );
    let vg = VoiceGateway::new(&client);
    let text = "Hello, this is a short voice probe from the terminal. It has a second sentence, so the stream has more than one segment.";
    let gen = host.next_gen();
    host.claim_speech(gen);
    let t0 = Instant::now();
    let reply = voice::speak_blocking(
        &host,
        &vg,
        gen,
        &var("ACODE_TTFA_RUN"),
        text,
        &VoicePrefs::default(),
        &mut |_| {},
    )
    .expect("spoken");
    let segments = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
    println!(
        "TTFA {:.3} s · total {:.3} s · segments {segments} · metrics {}",
        reply.first_audio.map(|d| d.as_secs_f64()).unwrap_or(-1.0),
        t0.elapsed().as_secs_f64(),
        reply.metrics
    );
    host.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

// -- R17.1: read aloud inside an automation --------------------------------------

/// The recorded automation detail (`tests/fixtures/automations`), applied
/// as the automations lane posts it.
fn answer_inbox_detail(h: &mut Harness) -> String {
    use abstractcode::automations as auto;
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/automations");
    let read = |n: &str| -> Value {
        serde_json::from_slice(&std::fs::read(format!("{dir}/{n}")).unwrap()).unwrap()
    };
    let list = auto::parse_list_page(&read("list.json")).unwrap();
    let summary = list.items.into_iter().next().unwrap();
    let id = summary.id.clone();
    let definition = auto::Definition {
        revision: 1,
        workflow_id: "inbox@1.0.0:triage".into(),
        tool_approval: "ask".into(),
        growing: Default::default(),
        max_attempts: Some(3),
        workspace_root: summary.workspace_root.clone().unwrap_or_default(),
        target: json!({"bundle_ref": "inbox@1.0.0", "flow_id": "triage", "input_data": {}}),
        notify: serde_json::Value::Null,
    };
    let page = auto::parse_occurrence_page(&read("occurrences.json")).unwrap();
    let id2 = id.clone();
    h.store
        .automations
        .update(|v| v.apply_detail(&id2, definition, summary, page));
    h.turn();
    id
}

#[test]
fn ctrl_p_in_an_automation_reads_the_selected_runs_reply() {
    let _g = serial();
    let gw = serve(GatewayScript {
        segment_delay: Duration::from_millis(300),
        segments: vec![1, 2, 3, 4, 5, 6],
        ..Default::default()
    });
    let (_host, log, _) = fake_host(false);
    let mut h = harness(&gw.url);
    h.with_reply("The conversation's own reply.");
    let id = "53443dd0-25c4-5fa8-bdad-e1ac3fdfff8e";
    h.type_text(&format!("/automations {id}\r"));
    h.turn();
    assert_eq!(answer_inbox_detail(&mut h), id);
    // At rest the cursor is on the wait (not a run): it says so, nothing spoken.
    h.term.push_input(&[0x10]);
    h.until("the select-a-run notice", |s| {
        s.contains("select a run (↑↓) to read its reply aloud")
    });
    assert!(gw.requests("/voice/tts/stream").is_empty());
    // The newest run (#7, still waiting) has no reply yet.
    for _ in 0..30 {
        h.term.push_input(b"\x1b[B");
        h.turn();
    }
    h.term.push_input(&[0x10]);
    h.until("no reply yet", |s| {
        s.contains("No reply to read aloud yet.")
    });
    assert!(gw.requests("/voice/tts/stream").is_empty());
    // Run #6: its reply, through the AUTOMATION's run (the web's runId).
    h.term.push_input(b"\x1b[A");
    h.turn();
    h.term.push_input(&[0x10]);
    h.until("reading", |s| {
        s.contains("Reading the reply aloud — Ctrl+P stops.")
    });
    let reqs = h_wait_requests(&gw, &format!("/runs/{id}/voice/tts/stream"));
    let body: Value = serde_json::from_str(&reqs[0].2).unwrap();
    assert_eq!(
        body.get("text"),
        Some(&json!("1 new newsletter; nothing needs a reply.")),
        "the selected run's reply, never the conversation's"
    );
    assert!(gw.requests("/runs/run-ui/").is_empty());
    // Ctrl+P again stops.
    h.until("playing", |_| !played(&log).is_empty());
    h.term.push_input(&[0x10]);
    h.turn();
    h.until("stopped", |_| ops(&log).contains(&"stop".to_string()));
}

/// The requests matching `needle`, waiting for the voice thread to send one.
fn h_wait_requests(gw: &FakeGateway, needle: &str) -> Vec<(String, String, String)> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let r = gw.requests(needle);
        if !r.is_empty() {
            return r;
        }
        assert!(Instant::now() < deadline, "no request to {needle}");
        std::thread::sleep(Duration::from_millis(15));
    }
}

#[test]
fn voice_screen_shows_the_gateways_served_speech_input_hint() {
    // Round 16: the gateway serves a one-line hint for a stored speech-input route (on Apple
    // silicon, mlx-whisper runs the model on the GPU); the screen shows it verbatim.
    let _g = serial();
    let sentence = "Runs on the processor: faster-whisper has no Apple GPU backend. mlx-whisper runs large-v3 on this Mac's GPU.";
    let gw = serve(GatewayScript {
        defaults: json!({
            "tts": {"route": "output.voice", "configured": true, "provider": "supertonic", "model": "supertonic-3", "voice": "M3"},
            "stt": {"route": "input.voice", "configured": true, "provider": "faster-whisper", "model": "large-v3",
                    "hint": {"code": "apple_gpu_engine", "sentence": sentence,
                             "route": {"key": "input.voice", "provider": "mlx-whisper", "model": "large-v3"}}}
        }),
        ..GatewayScript::default()
    });
    let (_host, _log, _) = fake_host(false);
    let mut h = harness(&gw.url);
    h.with_reply("ok");
    h.type_text("/voice\r");
    let s = h.until("the served hint on the voice screen", |s| {
        s.contains("Apple GPU backend")
    });
    assert!(
        s.contains("Speech → text     Gateway default · faster-whisper / large-v3"),
        "{s}"
    );
    // Wrapped, never cut: every word of the sentence is on screen, in order.
    for line in abstracttui::text::wrap(sentence, 80) {
        assert!(s.contains(line.trim_end()), "missing {line:?}:\n{s}");
    }
}

// -- round 18: the spoken language is the ACCOUNT's (gateway-served block) --------

const PREFS_GET: &str = include_str!("fixtures/account_prefs/get_default.json");
const PREFS_PUT_FR: &str = include_str!("fixtures/account_prefs/put_spoken_fr.json");

fn spoken(fixture: &str) -> abstractcode::account_prefs::SpokenLanguagePref {
    abstractcode::account_prefs::spoken_language(&serde_json::from_str(fixture).unwrap()).unwrap()
}

#[test]
fn voice_screen_spoken_language_is_the_accounts_served_block() {
    let _g = serial();
    let gw = serve(GatewayScript::default());
    let (_host, _log, _) = fake_host(false);
    let mut h = harness(&gw.url);
    h.with_reply("ok");
    // Unknown block (an older gateway): the row says the seam sentence.
    h.store.account_workflow.update(|v| {
        v.spoken_language = Err(abstractcode::account_prefs::SPOKEN_LANGUAGE_MISSING.to_string())
    });
    h.type_text("/voice\r");
    let s = h.until("the voice screen", |s| s.contains("Spoken language"));
    assert!(
        s.contains("Spoken language   The gateway's account preferences answer has no spoken_language block."),
        "{s}"
    );
    h.escape();
    // Served block: the row shows the served label of the account's value.
    h.store
        .account_workflow
        .update(|v| v.spoken_language = Ok(spoken(PREFS_GET)));
    h.type_text("/voice\r");
    let s = h.until("the served label", |s| {
        s.contains("Spoken language   Auto (detected)")
    });
    assert!(!s.contains("Detect automatically"), "no client list:\n{s}");
    // Rows: 1 TTS … 10 Spoken language (0 Engines, 3 Output, 4 Output device,
    // 5 Test speaker, 6 Reply volume, 7 Microphone, 8 Input device, 9 Test microphone).
    h.term.push_input("\x1b[B".repeat(9).as_bytes());
    let s = h.until("the served help on the row", |s| {
        s.contains("The language spoken to the microphone.")
    });
    assert!(s.contains("Spoken language   Auto (detected)"), "{s}");
    while h._rx.try_recv().is_ok() {}
    h.type_text("\r");
    let s = h.until("the served choices", |s| {
        s.contains("Spoken language · Enter selects") && s.contains("French")
    });
    assert!(
        s.contains("Auto (detected)") && s.contains("English"),
        "{s}"
    );
    // Pick French: Auto(0) Arabic Chinese Dutch English French = 5 downs.
    let fr_ix = spoken(PREFS_GET)
        .choices
        .iter()
        .position(|(v, _)| v == "fr")
        .unwrap();
    h.term.push_input("\x1b[B".repeat(fr_ix).as_bytes());
    h.turn();
    h.type_text("\r");
    h.turn();
    let sent: Vec<Cmd> = std::iter::from_fn(|| h._rx.try_recv().ok()).collect();
    assert!(
        sent.iter().any(|c| matches!(c, Cmd::AccountPrefs(abstractcode::gateway::preferences::PrefCmd::SaveSpokenLanguage { value }) if value == "fr")),
        "one PUT of the pick: {sent:?}"
    );
    // The lane answers: the row follows the gateway, with the note.
    h.store
        .account_workflow
        .update(|v| v.apply_spoken_save(Ok(spoken(PREFS_PUT_FR))));
    let s = h.until("French saved", |s| {
        s.contains("Spoken language   French") && s.contains("Saved.")
    });
    assert!(s.contains("Voice ·"), "{s}");
    // The config never holds a language.
    let saved = h.prefs.borrow().voice.clone().unwrap_or(json!({}));
    assert!(saved.get("stt_language").is_none(), "{saved}");
}

#[test]
fn the_dictation_line_names_the_accounts_spoken_language() {
    let _g = serial();
    let gw = serve(GatewayScript::default());
    let (_host, _log, _) = fake_host(false);
    let mut h = harness(&gw.url);
    h.with_reply("ready");
    h.store
        .account_workflow
        .update(|v| v.spoken_language = Ok(spoken(PREFS_PUT_FR)));
    h.store
        .voice
        .dictation
        .set(abstractcode::voice::Dictation::Transcribing {
            since: Instant::now() - Duration::from_secs(4),
            route: "faster-whisper / large-v3".into(),
        });
    let s = h.turn();
    assert!(
        s.contains("Transcribing… 4 s · faster-whisper / large-v3 · Spoken language: French"),
        "{s}"
    );
    h.store
        .voice
        .dictation
        .set(abstractcode::voice::Dictation::Recording {
            since: Instant::now(),
            gen: 0,
        });
    let s = h.turn();
    assert!(
        s.contains("● Recording… 0 s · Spoken language: French · Ctrl+R transcribes"),
        "{s}"
    );
}
