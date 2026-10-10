//! Voice — speak replies and dictate, through the gateway's voice routes.
//!
//! The SAME contract as Code web and every kit app (ui-kit 0.7.1,
//! `AfVoiceSection` / `useGatewayVoice`):
//!
//! - "Gateway default · supertonic / supertonic-3" comes from ONE answer,
//!   `GET /api/gateway/voice/defaults` (`output.voice` / `input.voice`
//!   capability routes) — never from the voice catalog's engine-side
//!   fields (that is how "Gateway default · openai" appeared, round 6).
//! - Speaking a reply = `POST /runs/{id}/voice/tts/stream` (JSON Lines;
//!   the gateway splits the text at sentence boundaries and sends each WAV
//!   segment as soon as it is synthesised). Each segment goes straight to
//!   the host speaker while the next one is synthesised.
//! - Dictation = record on this computer → upload the WAV as a session
//!   attachment → `POST /runs/{id}/audio/transcribe` with the speech-to-
//!   text override only when the user set one (empty = the gateway's
//!   default route). The response names the route that ran.
//! - Preferences use the kit's `VoiceClientPreferences` keys, so a value
//!   means the same thing in every app; an empty field = the gateway
//!   default.
//! - The spoken language is the ACCOUNT's (round 18): the gateway applies
//!   its `spoken_language` preference ([`crate::account_prefs`]) to every
//!   transcription. This app keeps no language and never sends one (an old
//!   `stt_language` key in the config is ignored).
//!
//! Host audio (speaker, microphone, device lists) is AbstractVoice's, run
//! through [`crate::voice_host`]; this module never synthesises or
//! recognises anything itself. Every failure reaches the user as one
//! sentence with the kit's wording.

use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

use abstracttui::reactive::{Scope, Signal};
use serde_json::{json, Map, Value};

use crate::gateway::{url_encode, GatewayClient};
use crate::voice_host::{Host, HostEvent};

/// A recording shorter than this carries no speech (the kit's `MIN_RECORDING_MS`).
pub const MIN_RECORDING_MS: u64 = 350;
/// Peak level under which a recording is silence (the kit's `SILENT_LEVEL`).
pub const SILENT_LEVEL: f64 = 0.02;
/// How long a transcription may take before it is reported as failed (Code web's value).
pub const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(180);
/// A dictation stops by itself after this long (a forgotten recording must end).
pub const MAX_RECORDING_S: u64 = 120;

/// The latency choices (`quality_preset`, the kit's `VOICE_LATENCY_OPTIONS`).
pub const LATENCY: &[(&str, &str)] = &[
    ("", "Gateway default"),
    ("standard", "Balanced"),
    ("low", "Faster (lower quality)"),
    ("high", "Higher quality (slower)"),
];

/// Client voice preferences — the kit's `VoiceClientPreferences`. Empty =
/// the gateway default; devices are host device ids ("" = system default).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VoicePrefs {
    pub provider: String,
    pub model: String,
    pub voice: String,
    pub profile: String,
    pub speed: Option<f64>,
    pub quality_preset: String,
    pub instructions: String,
    pub stt_provider: String,
    pub stt_model: String,
    pub output_device: String,
    pub input_device: String,
    pub input_gain: Option<f64>,
    pub reply_volume: Option<f64>,
    pub read_aloud: bool,
}

const TTS_KEYS: [&str; 7] = [
    "provider",
    "model",
    "voice",
    "profile",
    "speed",
    "quality_preset",
    "instructions",
];

impl VoicePrefs {
    pub fn from_json(v: Option<&Value>) -> VoicePrefs {
        let Some(v) = v.filter(|v| v.is_object()) else {
            return VoicePrefs::default();
        };
        let s = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let f = |k: &str| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
        VoicePrefs {
            provider: s("provider"),
            model: s("model"),
            voice: s("voice"),
            profile: s("profile"),
            speed: f("speed"),
            quality_preset: s("quality_preset"),
            instructions: s("instructions"),
            stt_provider: s("stt_provider"),
            stt_model: s("stt_model"),
            output_device: s("output_device"),
            input_device: s("input_device"),
            input_gain: f("input_gain"),
            reply_volume: f("reply_volume"),
            read_aloud: v
                .get("read_aloud")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }

    /// The stored form: only what is set (an unset key = the gateway default).
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        let mut put = |k: &str, v: &str| {
            if !v.is_empty() {
                m.insert(k.into(), json!(v));
            }
        };
        put("provider", &self.provider);
        put("model", &self.model);
        put("voice", &self.voice);
        put("profile", &self.profile);
        put("quality_preset", &self.quality_preset);
        put("instructions", &self.instructions);
        put("stt_provider", &self.stt_provider);
        put("stt_model", &self.stt_model);
        put("output_device", &self.output_device);
        put("input_device", &self.input_device);
        if let Some(x) = self.speed {
            m.insert("speed".into(), json!(x));
        }
        if let Some(x) = self.input_gain {
            m.insert("input_gain".into(), json!(x));
        }
        if let Some(x) = self.reply_volume {
            m.insert("reply_volume".into(), json!(x));
        }
        if self.read_aloud {
            m.insert("read_aloud".into(), json!(true));
        }
        Value::Object(m)
    }

    /// The fields a TTS request carries (the kit's `voiceTtsRequest`): only
    /// the speech fields, empty values dropped — read-aloud, devices and the
    /// transcription route are this app's, not the request's.
    pub fn tts_request(&self) -> Map<String, Value> {
        let mut m = Map::new();
        for key in TTS_KEYS {
            let v = match key {
                "provider" => json!(self.provider),
                "model" => json!(self.model),
                "voice" => json!(self.voice),
                "profile" => json!(self.profile),
                "quality_preset" => json!(self.quality_preset),
                "instructions" => json!(self.instructions),
                _ => match self.speed {
                    Some(x) => json!(x),
                    None => continue,
                },
            };
            if v.as_str() != Some("") {
                m.insert(key.into(), v);
            }
        }
        m
    }

    /// The fields a transcription request carries (the kit's
    /// `voiceSttRequest`): provider/model only when overridden (empty =
    /// gateway default). Never a language: the gateway applies the
    /// account's spoken language (round 18).
    pub fn stt_request(&self) -> Map<String, Value> {
        let mut m = Map::new();
        if !self.stt_provider.is_empty() {
            m.insert("provider".into(), json!(self.stt_provider));
            if !self.stt_model.is_empty() {
                m.insert("model".into(), json!(self.stt_model));
            }
        }
        m
    }

    /// Reply volume 0..1 (absent = 1).
    pub fn volume(&self) -> f64 {
        self.reply_volume.map(|v| v.clamp(0.0, 1.0)).unwrap_or(1.0)
    }

    /// Input gain 0.5..1.5 (absent = 1).
    pub fn gain(&self) -> f64 {
        self.input_gain.map(|v| v.clamp(0.5, 1.5)).unwrap_or(1.0)
    }

    /// "provider · model · voice" of the TTS override, "" when the gateway default applies.
    pub fn tts_override_summary(&self) -> String {
        let voice = if self.profile.is_empty() {
            &self.voice
        } else {
            &self.profile
        };
        [&self.provider, &self.model, voice]
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    pub fn stt_override_summary(&self) -> String {
        [&self.stt_provider, &self.stt_model]
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// One route of `GET /api/gateway/voice/defaults`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RouteDefault {
    pub configured: bool,
    pub provider: String,
    pub model: String,
    pub voice: String,
    pub note: String,
    /// The gateway's served one-line hint for this route (`hint.sentence`, e.g. speech input
    /// on Apple silicon: mlx-whisper runs the model on the GPU). Shown verbatim.
    pub hint: String,
}

/// The body of `GET /api/gateway/voice/defaults`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VoiceDefaults {
    pub tts: Option<RouteDefault>,
    pub stt: Option<RouteDefault>,
}

impl VoiceDefaults {
    pub fn from_json(v: &Value) -> VoiceDefaults {
        let route = |k: &str| {
            v.get(k).filter(|r| r.is_object()).map(|r| {
                let s = |f: &str| {
                    r.get(f)
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string()
                };
                RouteDefault {
                    configured: r
                        .get("configured")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    provider: s("provider"),
                    model: s("model"),
                    voice: s("voice"),
                    note: s("note"),
                    hint: r
                        .get("hint")
                        .and_then(|h| h.get("sentence"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                }
            })
        };
        VoiceDefaults {
            tts: route("tts"),
            stt: route("stt"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceKind {
    Tts,
    Stt,
}

/// "provider / model" of a route ("" when nothing is known).
pub fn route_text(provider: &str, model: &str) -> String {
    [provider.trim(), model.trim()]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" / ")
}

/// What "Gateway default · …" names for `kind` (the kit's
/// `voiceDefaultSummary`): the configured route, "not set" when the
/// administrator set none, "unknown" when the gateway could not be asked,
/// "" while loading.
pub fn default_summary(defaults: Option<&VoiceDefaults>, kind: VoiceKind, failed: bool) -> String {
    if failed {
        return "unknown".into();
    }
    let Some(d) = defaults else {
        return String::new();
    };
    let entry = match kind {
        VoiceKind::Tts => d.tts.as_ref(),
        VoiceKind::Stt => d.stt.as_ref(),
    };
    match entry {
        Some(e) if e.configured => {
            let t = route_text(&e.provider, &e.model);
            if t.is_empty() {
                "not set".into()
            } else {
                t
            }
        }
        _ => "not set".into(),
    }
}

/// The route a transcription will use (the kit's `sttRouteText`): the
/// override, else the gateway default, else "".
pub fn stt_route_text(prefs: &VoicePrefs, defaults: Option<&VoiceDefaults>) -> String {
    if !prefs.stt_provider.is_empty() {
        return route_text(&prefs.stt_provider, &prefs.stt_model);
    }
    match defaults.and_then(|d| d.stt.as_ref()) {
        Some(e) if e.configured => route_text(&e.provider, &e.model),
        _ => String::new(),
    }
}

/// Whole seconds ("12 s").
pub fn elapsed_seconds(since: Instant, now: Instant) -> String {
    format!("{} s", now.saturating_duration_since(since).as_secs())
}

/// "Transcribing… 12 s · faster-whisper / large-v3" (the kit's `transcribingLine`).
pub fn transcribing_line(since: Instant, now: Instant, route: &str) -> String {
    let route = if route.is_empty() {
        String::new()
    } else {
        format!(" · {route}")
    };
    format!("Transcribing… {}{route}", elapsed_seconds(since, now))
}

/// "Prefix: detail." — every voice failure is one sentence (the kit's `voiceErrorSentence`).
pub fn error_sentence(prefix: &str, detail: &str) -> String {
    let d = detail.trim().trim_end_matches('.');
    if d.is_empty() {
        format!("{prefix}.")
    } else {
        format!("{prefix}: {d}.")
    }
}

/// What a dictation is doing (UI state).
#[derive(Debug, Clone, PartialEq)]
pub enum Dictation {
    Idle,
    Recording { since: Instant, gen: u64 },
    Transcribing { since: Instant, route: String },
}

/// What speaking a reply is doing (UI state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaking {
    Idle,
    /// Asked the gateway; no audio yet.
    Preparing,
    Playing,
}

/// Host device lists (from AbstractVoice through the bridge).
#[derive(Debug, Clone, PartialEq)]
pub enum Devices {
    Unknown,
    Loading,
    Loaded {
        output: Vec<(String, String)>,
        input: Vec<(String, String)>,
    },
    Failed(String),
}

/// The gateway's default voice routes, as known to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum DefaultsState {
    Unknown,
    /// Boxed: the served routes (with their hints) dwarf the other variants.
    Loaded(Box<VoiceDefaults>),
    Failed(String),
}

impl DefaultsState {
    pub fn value(&self) -> Option<&VoiceDefaults> {
        match self {
            DefaultsState::Loaded(d) => Some(d.as_ref()),
            _ => None,
        }
    }
    pub fn failed(&self) -> bool {
        matches!(self, DefaultsState::Failed(_))
    }
}

/// A note under a settings row (speaker / microphone test results).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Ok,
    Error,
}

/// The voice lane's UI signals (one per app, on the [`crate::store::Store`]).
#[derive(Clone, Copy)]
pub struct VoiceSignals {
    pub speaking: Signal<Speaking>,
    pub dictation: Signal<Dictation>,
    /// Microphone level 0..1 while recording or testing.
    pub level: Signal<f32>,
    pub devices: Signal<Devices>,
    pub defaults: Signal<DefaultsState>,
    /// Output (speaker test) note.
    pub speaker_note: Signal<Option<(String, Tone)>>,
    /// Microphone test note.
    pub mic_note: Signal<Option<(String, Tone)>>,
    /// The last spoken reply: "first audio 0.84 s · mps".
    pub last_reply: Signal<Option<String>>,
    /// A finished transcription waiting to land in the composer.
    pub transcript: Signal<Option<String>>,
    /// Ticks once a second while something runs (elapsed seconds).
    pub tick: Signal<u64>,
    /// The microphone test is running.
    pub mic_testing: Signal<bool>,
}

impl VoiceSignals {
    pub fn create(cx: Scope) -> VoiceSignals {
        VoiceSignals {
            speaking: cx.signal(Speaking::Idle),
            dictation: cx.signal(Dictation::Idle),
            level: cx.signal(0.0f32),
            devices: cx.signal(Devices::Unknown),
            defaults: cx.signal(DefaultsState::Unknown),
            speaker_note: cx.signal(None),
            mic_note: cx.signal(None),
            last_reply: cx.signal(None),
            transcript: cx.signal(None),
            tick: cx.signal(0u64),
            mic_testing: cx.signal(false),
        }
    }
}

// -- gateway lane ------------------------------------------------------------

/// A finished transcription: the text and the route that ran.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcription {
    pub text: String,
    pub provider: String,
    pub model: String,
    pub duration_ms: Option<u64>,
    /// What reached the engine (round 18): the account's spoken language or
    /// "auto" ("" from a gateway older than round 18).
    pub language: String,
    /// What the engine reported ("" when it did not say).
    pub detected_language: String,
}

/// The gateway's voice routes over this client's connection. Built from the
/// app's [`GatewayClient`] (same base URL + token); its own agents because a
/// stream of spoken segments must never queue behind the worker's loop.
#[derive(Clone)]
pub struct VoiceGateway {
    client: GatewayClient,
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
    stream: ureq::Agent,
}

fn detail_of(resp: ureq::Response) -> String {
    let status = resp.status();
    let body = resp.into_string().unwrap_or_default();
    serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            v.get("detail").map(|d| match d {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
        })
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("HTTP {status}"))
}

fn ureq_detail(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(_, resp) => detail_of(resp),
        ureq::Error::Transport(t) => format!("the gateway could not be reached ({t})"),
    }
}

impl VoiceGateway {
    pub fn new(client: &GatewayClient) -> VoiceGateway {
        let (base, token) = client.connection();
        VoiceGateway {
            client: client.clone(),
            base,
            token,
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(5))
                .timeout_read(TRANSCRIBE_TIMEOUT)
                .timeout_write(Duration::from_secs(60))
                .build(),
            // Idle watchdog between spoken segments (the gateway has its own
            // synthesis watchdog and ends the stream with an error event).
            stream: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(5))
                .timeout_read(Duration::from_secs(120))
                .build(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/gateway{}", self.base, path)
    }

    fn auth(&self, req: ureq::Request) -> ureq::Request {
        match &self.token {
            Some(t) => req.set("Authorization", &format!("Bearer {t}")),
            None => req,
        }
    }

    fn get(&self, path: &str) -> Result<Value, String> {
        let resp = self
            .auth(
                self.agent
                    .get(&self.url(path))
                    .set("Accept", "application/json"),
            )
            .call()
            .map_err(ureq_detail)?;
        let body = resp.into_string().map_err(|e| e.to_string())?;
        serde_json::from_str(&body).map_err(|e| format!("invalid JSON from {path}: {e}"))
    }

    /// `GET /voice/defaults` — the routes "Gateway default" names.
    pub fn defaults(&self) -> Result<VoiceDefaults, String> {
        self.get("/voice/defaults")
            .map(|v| VoiceDefaults::from_json(&v))
    }

    /// `GET /voice/voices?compact=true[&provider&model]` — engines, models and
    /// voices to pick an override from.
    pub fn catalog(&self, provider: &str, model: &str) -> Result<Value, String> {
        let mut path = "/voice/voices?compact=true".to_string();
        if !provider.is_empty() {
            path.push_str(&format!("&provider={}", url_encode(provider)));
        }
        if !model.is_empty() {
            path.push_str(&format!("&model={}", url_encode(model)));
        }
        self.get(&path)
    }

    /// `POST /runs/{run_id}/voice/tts/stream`: calls `on_audio` with each
    /// base64 WAV segment as soon as it arrives; returns the terminal `done`
    /// event's `metrics` (`ttfb_s`, `rtf`, `device`, …). `cancelled` is
    /// checked between segments: a stopped reply drops the connection, which
    /// ends the synthesis on the gateway.
    pub fn tts_stream(
        &self,
        run_id: &str,
        text: &str,
        prefs: &VoicePrefs,
        cancelled: &dyn Fn() -> bool,
        on_audio: &mut dyn FnMut(&str) -> Result<(), String>,
    ) -> Result<Value, String> {
        let mut body = prefs.tts_request();
        body.insert("text".into(), json!(text));
        body.insert(
            "request_id".into(),
            json!(crate::gateway::mint_command_id()),
        );
        let path = format!("/runs/{}/voice/tts/stream", url_encode(run_id));
        let resp = self
            .auth(
                self.stream
                    .post(&self.url(&path))
                    .set("Accept", "application/x-ndjson")
                    .set("Content-Type", "application/json"),
            )
            .send_string(&Value::Object(body).to_string())
            .map_err(ureq_detail)?;
        let reader = BufReader::new(resp.into_reader());
        let mut segments = 0usize;
        for line in reader.lines() {
            if cancelled() {
                return Ok(Value::Null);
            }
            let line = line.map_err(|e| format!("the speech stream broke ({e})"))?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let evt: Value = serde_json::from_str(line)
                .map_err(|_| "the speech stream returned invalid JSON".to_string())?;
            let typ = evt.get("type").and_then(Value::as_str).unwrap_or("");
            if typ == "error" {
                return Err(evt
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("TTS stream error")
                    .to_string());
            }
            if let Some(b64) = evt
                .get("audio_b64")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                segments += 1;
                on_audio(b64)?;
            }
            if typ == "done" || typ == "cancelled" {
                if segments == 0 && typ == "done" {
                    return Err("the voice engine returned no audio".into());
                }
                return Ok(evt.get("metrics").cloned().unwrap_or(Value::Null));
            }
        }
        if cancelled() {
            return Ok(Value::Null);
        }
        Err("the speech stream ended before synthesis completed".into())
    }

    /// Upload `wav` as a session attachment, then `POST
    /// /runs/{run_id}/audio/transcribe` (override route + language only when
    /// set). Returns the text and the route that ran.
    pub fn transcribe(
        &self,
        session_id: &str,
        run_id: &str,
        wav: &[u8],
        prefs: &VoicePrefs,
    ) -> Result<Transcription, String> {
        let artifact = self
            .client
            .upload_attachment(session_id, "recording.wav", wav)
            .map_err(|e| e.compact_reason())?;
        let mut body = prefs.stt_request();
        body.insert("audio_artifact".into(), artifact);
        body.insert(
            "request_id".into(),
            json!(crate::gateway::mint_command_id()),
        );
        let path = format!("/runs/{}/audio/transcribe", url_encode(run_id));
        let resp = self
            .auth(
                self.agent
                    .post(&self.url(&path))
                    .set("Accept", "application/json")
                    .set("Content-Type", "application/json"),
            )
            .send_string(&Value::Object(body).to_string())
            .map_err(|e| match e {
                ureq::Error::Transport(t) if t.kind() == ureq::ErrorKind::Io => format!(
                    "the gateway did not answer within {} s",
                    TRANSCRIBE_TIMEOUT.as_secs()
                ),
                other => ureq_detail(other),
            })?;
        let v: Value = serde_json::from_str(&resp.into_string().map_err(|e| e.to_string())?)
            .map_err(|e| format!("invalid JSON from the transcription route: {e}"))?;
        let s = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        Ok(Transcription {
            text: s("text"),
            provider: s("provider"),
            model: s("model"),
            duration_ms: v.get("duration_ms").and_then(Value::as_u64),
            language: s("language"),
            detected_language: s("detected_language"),
        })
    }
}

// -- operations (blocking; run on voice threads, never the UI thread) -------

/// The result of one spoken reply.
#[derive(Debug, Clone, PartialEq)]
pub struct SpokenReply {
    /// From sending the request to the first audio reaching the speaker.
    pub first_audio: Option<Duration>,
    /// The gateway's `done` metrics (`ttfb_s`, `device`, …).
    pub metrics: Value,
    pub stopped: bool,
}

/// "first audio 0.84 s · mps" — what the settings screen shows for the last reply.
pub fn reply_line(r: &SpokenReply) -> String {
    let mut parts = Vec::new();
    if let Some(d) = r.first_audio {
        parts.push(format!("first audio {:.2} s", d.as_secs_f64()));
    }
    if let Some(dev) = r
        .metrics
        .get("device")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("engine on {dev}"));
    }
    parts.join(" · ")
}

/// Speak `text` on the host speaker: stream the gateway's segments into the
/// bridge while the next ones are synthesised. `on_started` fires once,
/// when the first audio reaches the player — watched while the HTTP stream
/// is still waiting for the next segment (the stream is read on its own
/// scoped thread), so "first audio" is the real moment, not the next
/// segment's arrival.
pub fn speak_blocking(
    host: &Host,
    gw: &VoiceGateway,
    gen: u64,
    run_id: &str,
    text: &str,
    prefs: &VoicePrefs,
    on_started: &mut dyn FnMut(Duration),
) -> Result<SpokenReply, String> {
    enum Msg {
        Audio(String),
        Done(Value),
        Failed(String),
    }
    let events = host.subscribe(gen);
    let result = (|| {
        host.ensure().map_err(|e| e.to_string())?;
        let t0 = Instant::now();
        let mut first: Option<Duration> = None;
        let device = prefs.output_device.clone();
        let volume = prefs.volume();
        let cancelled = || !host.is_current_speech(gen);
        let (seg_tx, seg_rx) = std::sync::mpsc::channel::<Msg>();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let tx = seg_tx;
                let res = gw.tts_stream(run_id, text, prefs, &cancelled, &mut |b64| {
                    tx.send(Msg::Audio(b64.to_string()))
                        .map_err(|_| "stopped".to_string())
                });
                let _ = tx.send(match res {
                    Ok(m) => Msg::Done(m),
                    Err(e) => Msg::Failed(e),
                });
            });
            let mut metrics: Option<Value> = None;
            let mut ended = false;
            loop {
                // Player events first: Started is noted the moment it lands.
                loop {
                    match events.try_recv() {
                        Ok(HostEvent::Started { .. }) if first.is_none() => {
                            let d = t0.elapsed();
                            first = Some(d);
                            on_started(d);
                        }
                        Ok(HostEvent::Done { .. }) if ended => {
                            return Ok(SpokenReply {
                                first_audio: first,
                                metrics: metrics.unwrap_or(Value::Null),
                                stopped: false,
                            });
                        }
                        Ok(HostEvent::Stopped { .. }) => {
                            return Ok(SpokenReply {
                                first_audio: first,
                                metrics: metrics.unwrap_or(Value::Null),
                                stopped: true,
                            });
                        }
                        Ok(HostEvent::Error { message, .. }) => return Err(message),
                        Ok(HostEvent::Exited(why)) => return Err(why),
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                if cancelled() {
                    return Ok(SpokenReply {
                        first_audio: first,
                        metrics: metrics.unwrap_or(Value::Null),
                        stopped: true,
                    });
                }
                if ended {
                    std::thread::sleep(Duration::from_millis(15));
                    continue;
                }
                match seg_rx.recv_timeout(Duration::from_millis(15)) {
                    Ok(Msg::Audio(b64)) => {
                        if !cancelled() {
                            host.send(&json!({"op": "play", "gen": gen, "b64": b64, "device": device, "volume": volume}))?;
                        }
                    }
                    Ok(Msg::Done(m)) => {
                        metrics = Some(m);
                        if cancelled() {
                            continue;
                        }
                        host.send(&json!({"op": "end", "gen": gen}))?;
                        ended = true;
                    }
                    Ok(Msg::Failed(e)) => return Err(e),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        return Err("the speech stream stopped".into())
                    }
                }
            }
        })
    })();
    host.unsubscribe(gen);
    result
}

/// A finished host recording.
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    pub path: String,
    pub duration_ms: u64,
    pub peak: f64,
}

/// Record from the chosen microphone until `record_stop` for `gen` is sent
/// (or [`MAX_RECORDING_S`] elapses). `on_level` gets the live level.
pub fn record_blocking(
    host: &Host,
    gen: u64,
    prefs: &VoicePrefs,
    max_s: u64,
    on_level: &mut dyn FnMut(f32),
) -> Result<Recording, String> {
    let events = host.subscribe(gen);
    let result = (|| {
        host.ensure().map_err(|e| e.to_string())?;
        let path = std::env::temp_dir().join(format!(
            "abstractcode-voice-{}-{gen}.wav",
            std::process::id()
        ));
        host.send(&json!({
            "op": "record", "gen": gen, "device": prefs.input_device, "gain": prefs.gain(),
            "path": path.to_string_lossy(), "max_s": max_s,
        }))?;
        loop {
            match events.recv_timeout(Duration::from_secs(max_s + 15)) {
                Ok(HostEvent::Level { rms, .. }) => on_level(rms),
                Ok(HostEvent::Recorded {
                    path,
                    duration_ms,
                    peak,
                    ..
                }) => {
                    return Ok(Recording {
                        path,
                        duration_ms,
                        peak,
                    })
                }
                Ok(HostEvent::Error { message, .. }) => return Err(message),
                Ok(HostEvent::Exited(why)) => return Err(why),
                Ok(_) => {}
                Err(_) => return Err("the recording never finished".into()),
            }
        }
    })();
    host.unsubscribe(gen);
    result
}

/// Ask the bridge for the host's audio devices: `(id, name)` per output and input.
#[allow(clippy::type_complexity)]
pub fn devices_blocking(
    host: &Host,
) -> Result<(Vec<(String, String)>, Vec<(String, String)>), String> {
    let gen = host.next_gen();
    let events = host.subscribe(gen);
    let result = (|| {
        host.ensure().map_err(|e| e.to_string())?;
        host.send(&json!({"op": "devices", "gen": gen}))?;
        loop {
            match events.recv_timeout(Duration::from_secs(10)) {
                Ok(HostEvent::Devices { output, input, .. }) => return Ok((output, input)),
                Ok(HostEvent::Error { message, .. }) => return Err(message),
                Ok(HostEvent::Exited(why)) => return Err(why),
                Ok(_) => {}
                Err(_) => return Err("the audio devices could not be listed".into()),
            }
        }
    })();
    host.unsubscribe(gen);
    result
}

/// Play a short chime on the chosen output (the speaker Test).
pub fn tone_blocking(host: &Host, prefs: &VoicePrefs) -> Result<(), String> {
    let gen = host.next_gen();
    let events = host.subscribe(gen);
    let result = (|| {
        host.ensure().map_err(|e| e.to_string())?;
        host.send(&json!({"op": "tone", "gen": gen, "device": prefs.output_device, "volume": prefs.volume()}))?;
        loop {
            match events.recv_timeout(Duration::from_secs(10)) {
                Ok(HostEvent::ToneDone { .. }) => return Ok(()),
                Ok(HostEvent::Error { message, .. }) => return Err(message),
                Ok(HostEvent::Exited(why)) => return Err(why),
                Ok(_) => {}
                Err(_) => return Err("the chime never finished".into()),
            }
        }
    })();
    host.unsubscribe(gen);
    result
}

/// Play a recorded WAV file on the chosen output and wait until it ends
/// (the microphone Test's playback).
pub fn play_file_blocking(host: &Host, path: &str, prefs: &VoicePrefs) -> Result<(), String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("the recording could not be read ({e})"))?;
    let gen = host.next_gen();
    host.claim_speech(gen);
    let events = host.subscribe(gen);
    let result = (|| {
        host.ensure().map_err(|e| e.to_string())?;
        host.send(
            &json!({"op": "play", "gen": gen, "b64": crate::voice_host::b64_encode(&bytes),
            "device": prefs.output_device, "volume": prefs.volume()}),
        )?;
        host.send(&json!({"op": "end", "gen": gen}))?;
        loop {
            match events.recv_timeout(Duration::from_secs(30)) {
                Ok(HostEvent::Done { .. }) | Ok(HostEvent::Stopped { .. }) => return Ok(()),
                Ok(HostEvent::Error { message, .. }) => return Err(message),
                Ok(HostEvent::Exited(why)) => return Err(why),
                Ok(_) => {}
                Err(_) => return Err("the playback never finished".into()),
            }
        }
    })();
    host.unsubscribe(gen);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_served_route_hint_is_read_verbatim() {
        let d = VoiceDefaults::from_json(&json!({
            "stt": {"configured": true, "provider": "faster-whisper", "model": "large-v3",
                    "hint": {"code": "apple_gpu_engine", "sentence": " Runs on the processor: mlx-whisper runs large-v3 on this Mac's GPU. ", "route": null}}
        }));
        assert_eq!(
            d.stt.as_ref().unwrap().hint,
            "Runs on the processor: mlx-whisper runs large-v3 on this Mac's GPU."
        );
        let none = VoiceDefaults::from_json(
            &json!({"stt": {"configured": true, "provider": "faster-whisper"}}),
        );
        assert_eq!(none.stt.unwrap().hint, "");
    }

    #[test]
    fn default_summary_matches_the_kit_wording() {
        let d = VoiceDefaults::from_json(&json!({
            "tts": {"route": "output.voice", "configured": true, "provider": "supertonic", "model": "supertonic-3", "voice": "M3"},
            "stt": {"route": "input.voice", "configured": false, "provider": null, "model": null, "note": "No gateway default is set for speech to text."}
        }));
        assert_eq!(
            default_summary(Some(&d), VoiceKind::Tts, false),
            "supertonic / supertonic-3"
        );
        assert_eq!(default_summary(Some(&d), VoiceKind::Stt, false), "not set");
        assert_eq!(default_summary(None, VoiceKind::Tts, false), "");
        assert_eq!(default_summary(Some(&d), VoiceKind::Tts, true), "unknown");
    }

    #[test]
    fn requests_carry_only_what_the_user_overrode() {
        let p = VoicePrefs::default();
        assert!(
            p.tts_request().is_empty(),
            "gateway default = no provider/model/voice in the request"
        );
        assert!(p.stt_request().is_empty());
        let p = VoicePrefs {
            provider: "piper".into(),
            stt_provider: "faster-whisper".into(),
            stt_model: "small".into(),
            output_device: "BuiltIn".into(),
            read_aloud: true,
            ..Default::default()
        };
        let tts = p.tts_request();
        assert_eq!(tts.get("provider"), Some(&json!("piper")));
        assert!(!tts.contains_key("output_device") && !tts.contains_key("read_aloud"));
        let stt = p.stt_request();
        assert_eq!(stt.get("model"), Some(&json!("small")));
        assert_eq!(stt.len(), 2, "provider/model only: {stt:?}");
    }

    #[test]
    fn a_transcription_request_never_carries_a_language() {
        // Round 18: an old config's `stt_language` is ignored; the account's applies on the gateway.
        let p = VoicePrefs::from_json(Some(
            &json!({"stt_language": "fr", "stt_provider": "faster-whisper"}),
        ));
        let stt = p.stt_request();
        assert!(!stt.contains_key("language"), "{stt:?}");
        assert_eq!(Value::Object(stt), json!({"provider": "faster-whisper"}));
        assert!(
            p.to_json().get("stt_language").is_none(),
            "never written back"
        );
    }

    #[test]
    fn prefs_round_trip_and_clamp() {
        let p = VoicePrefs {
            voice: "M3".into(),
            reply_volume: Some(1.7),
            input_gain: Some(0.1),
            read_aloud: true,
            ..Default::default()
        };
        let back = VoicePrefs::from_json(Some(&p.to_json()));
        assert_eq!(back, p);
        assert_eq!(back.volume(), 1.0);
        assert_eq!(back.gain(), 0.5);
        assert_eq!(VoicePrefs::default().to_json(), json!({}));
    }

    #[test]
    fn transcribing_line_names_elapsed_time_and_route() {
        let t0 = Instant::now();
        let line = transcribing_line(
            t0,
            t0 + Duration::from_millis(12_400),
            "faster-whisper / large-v3",
        );
        assert_eq!(line, "Transcribing… 12 s · faster-whisper / large-v3");
        assert_eq!(transcribing_line(t0, t0, ""), "Transcribing… 0 s");
        let prefs = VoicePrefs::default();
        let d = VoiceDefaults::from_json(
            &json!({"stt": {"configured": true, "provider": "faster-whisper", "model": "large-v3"}}),
        );
        assert_eq!(
            stt_route_text(&prefs, Some(&d)),
            "faster-whisper / large-v3"
        );
        let o = VoicePrefs {
            stt_provider: "openai".into(),
            ..Default::default()
        };
        assert_eq!(stt_route_text(&o, Some(&d)), "openai");
    }

    #[test]
    fn error_sentences_are_one_sentence() {
        assert_eq!(
            error_sentence("Reading aloud failed", "HTTP 503."),
            "Reading aloud failed: HTTP 503."
        );
        assert_eq!(
            error_sentence("Transcription failed", ""),
            "Transcription failed."
        );
    }
}
