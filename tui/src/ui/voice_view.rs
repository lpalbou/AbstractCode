//! Voice in the terminal — speak replies, dictate, and the `/voice` screen.
//!
//! Same contract and wording as the kit's VoiceSettings (Code web, Observer,
//! Entity): engines read "Gateway default · supertonic / supertonic-3" from
//! `GET /voice/defaults`; a reply is spoken from the gateway's streaming
//! route, sentence by sentence; dictation shows "Transcribing… 4 s ·
//! faster-whisper / large-v3" and lands in the composer. Audio plays and
//! records on this computer through AbstractVoice (`crate::voice_host`).
//!
//! Keys: Ctrl+P reads the latest reply aloud (playing = pause, paused =
//! resume), Ctrl+R starts a dictation (again = transcribe), a held Space on
//! an empty prompt talks (`ui::push_to_talk`), Esc stops speech / cancels a
//! recording. Each message's `▶` reads it; Read aloud narrates the cycles
//! as they land. Commands: `/voice`, `/speak [stop]`, `/dictate`.
//!
//! Threads: every gateway call and every bridge wait runs on a named voice
//! thread; results come back as posted closures. The UI thread only flips
//! signals and sends one-line commands to an already running bridge.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use abstracttui::prelude::*;
use abstracttui::text;
use serde_json::{json, Value};

use super::modals::{modal_size, open_picker, Picker};
use super::UiCtx;
use crate::store::{Phase, Store};
use crate::transcript::Item;
use crate::voice::{
    self, default_summary, error_sentence, stt_route_text, transcribing_line, DefaultsState,
    Devices, Dictation, Speaking, Tone, VoiceDefaults, VoiceGateway, VoiceKind, VoicePrefs,
};
use crate::voice_host;

/// The dictation allowed to deliver its text (0 = none; Esc cancels).
static DICTATION: AtomicU64 = AtomicU64::new(0);

// -- preferences ---------------------------------------------------------------

pub fn prefs(ctx: &UiCtx) -> VoicePrefs {
    VoicePrefs::from_json(ctx.prefs.borrow().voice.as_ref())
}

fn save_prefs(store: Store, ctx: &UiCtx, p: &VoicePrefs) {
    {
        let mut prefs = ctx.prefs.borrow_mut();
        let v = p.to_json();
        prefs.voice = if v.as_object().is_some_and(|m| m.is_empty()) {
            None
        } else {
            Some(v)
        };
        let _ = prefs.save();
    }
    store.voice.tick.update(|t| *t += 1);
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new().name(name.into()).spawn(f);
}

fn post_error(store: Store, text: String) {
    store
        .fold
        .update(|f| f.push_item(Item::Error { text: text.clone() }));
    store.notify(text);
}

/// The gateway's default voice routes, fetched once per app (and again when
/// the voice screen opens after a failure).
pub fn load_defaults(store: Store, ctx: &UiCtx) {
    if matches!(
        store.voice.defaults.get_untracked(),
        DefaultsState::Loaded(_)
    ) {
        return;
    }
    let gw = VoiceGateway::new(&ctx.client);
    let wake = abstracttui::reactive::wake_handle();
    spawn("voice-defaults", move || {
        let state = match gw.defaults() {
            Ok(d) => DefaultsState::Loaded(Box::new(d)),
            Err(e) => DefaultsState::Failed(e),
        };
        wake.post(move || store.voice.defaults.set(state));
    });
}

// -- speaking replies ----------------------------------------------------------

/// The latest reply: the last final answer after the last user message.
pub fn latest_reply(store: Store) -> Option<String> {
    store.fold.with_untracked(|f| {
        for item in f.items.iter().rev() {
            match item {
                Item::Assistant { text, .. } if !text.trim().is_empty() => {
                    return Some(text.clone())
                }
                Item::User { .. } => return None,
                _ => {}
            }
        }
        None
    })
}

/// Ctrl+P / `/speak`: read the latest reply aloud; while one is playing,
/// PAUSE it; while paused, RESUME it. (Esc stops.)
pub fn toggle_speak(store: Store, ctx: &UiCtx) {
    match store.voice.speaking.get_untracked() {
        Speaking::Playing => {
            pause_speaking(store);
            return;
        }
        Speaking::Paused => {
            resume_speaking(store);
            return;
        }
        Speaking::Preparing => {
            stop_speaking(store);
            return;
        }
        Speaking::Idle => {}
    }
    match latest_reply(store) {
        Some(text) => speak_text(store, ctx, text),
        None => store.notify("No reply to read aloud yet."),
    }
}

/// The play/pause icon of ONE message (operator ask 2026-10-10): idle →
/// read it; this message playing → pause; paused → resume; another text
/// speaking or queued → that stops and this one is read.
pub fn toggle_message(store: Store, ctx: &UiCtx, text: &str) {
    let key = voice::text_key(text);
    if store.voice.speaking_text.get_untracked() == Some(key) {
        match store.voice.speaking.get_untracked() {
            Speaking::Playing => return pause_speaking(store),
            Speaking::Paused => return resume_speaking(store),
            Speaking::Preparing => {
                stop_speaking(store);
                return;
            }
            Speaking::Idle => {}
        }
    }
    stop_speaking(store);
    speak_text(store, ctx, text.to_string());
}

/// Pause the reply being read (the player holds its place).
pub fn pause_speaking(store: Store) {
    if store.voice.speaking.get_untracked() == Speaking::Playing
        && voice_host::host().speech_op("pause")
    {
        store.voice.speaking.set(Speaking::Paused);
    }
}

/// Resume a paused reply where it stopped.
pub fn resume_speaking(store: Store) {
    if store.voice.speaking.get_untracked() == Speaking::Paused
        && voice_host::host().speech_op("resume")
    {
        store.voice.speaking.set(Speaking::Playing);
    }
}

thread_local! {
    /// Read-aloud narration waiting for the speaker (UI thread only): the
    /// cycles' words as they land, then the answer — one at a time, in
    /// order, never interrupting each other.
    static NARRATION: RefCell<std::collections::VecDeque<String>> =
        const { RefCell::new(std::collections::VecDeque::new()) };
}

/// Queue `text` for read-aloud narration; it starts at once when nothing
/// speaks.
pub fn narrate(store: Store, ctx: &UiCtx, text: String) {
    NARRATION.with(|q| q.borrow_mut().push_back(text));
    if store.voice.speaking.get_untracked() == Speaking::Idle {
        speak_next(store, ctx);
    }
}

fn speak_next(store: Store, ctx: &UiCtx) {
    if let Some(next) = NARRATION.with(|q| q.borrow_mut().pop_front()) {
        speak_text(store, ctx, next);
    }
}

/// Speak `text` through the gateway's streaming voice route.
pub fn speak_text(store: Store, ctx: &UiCtx, text: String) {
    // Before the first run, the session's media run carries the voice
    // route (the run dictation uses — see `transcription_run_id`).
    let session_id = store.session_id.get_untracked();
    let run_id = transcription_run_id(&store.run_id.get_untracked(), &session_id)
        .filter(|_| !session_id.is_empty())
        .unwrap_or_default();
    speak_text_for(store, ctx, run_id, text)
}

/// Speak `text` through the voice route of `run_id` (the conversation's
/// run — before its first run, the session's media run — or an
/// automation's: the Code web reads an automation's messages through the
/// automation's own run, `automations_view.tsx:284-286`).
pub fn speak_text_for(store: Store, ctx: &UiCtx, run_id: String, text: String) {
    if run_id.is_empty() {
        post_error(
            store,
            "Reading aloud failed: start a conversation first.".into(),
        );
        return;
    }
    let host = voice_host::host();
    host.stop_speech();
    let gen = host.next_gen();
    host.claim_speech(gen);
    store.voice.speaking.set(Speaking::Preparing);
    store.voice.speaking_text.set(Some(voice::text_key(&text)));
    store.voice.speak_levels.set(Vec::new());
    let p = prefs(ctx);
    let gw = VoiceGateway::new(&ctx.client);
    let wake = abstracttui::reactive::wake_handle();
    spawn("voice-speak", move || {
        let started_wake = wake.clone();
        let level_wake = wake.clone();
        let host2 = host.clone();
        let host3 = host.clone();
        let result = voice::speak_blocking_levels(
            &host,
            &gw,
            gen,
            &run_id,
            &text,
            &p,
            &mut |_d| {
                if host2.is_current_speech(gen) {
                    started_wake.post(move || {
                        if store.voice.speaking.get_untracked() == Speaking::Preparing {
                            store.voice.speaking.set(Speaking::Playing);
                        }
                    });
                }
            },
            &mut |rms, bands| {
                if host3.is_current_speech(gen) {
                    let bands = voice::meter_bands(rms, bands);
                    level_wake.post(move || store.voice.speak_levels.set(bands));
                }
            },
        );
        let current = host.is_current_speech(gen);
        host.release_speech(gen);
        wake.post(move || {
            if current {
                store.voice.speaking.set(Speaking::Idle);
                store.voice.speaking_text.set(None);
            }
            match result {
                Ok(reply) => {
                    if !reply.stopped {
                        let line = voice::reply_line(&reply);
                        if !line.is_empty() {
                            store.voice.last_reply.set(Some(line));
                        }
                    }
                }
                Err(e) => {
                    if current {
                        post_error(store, error_sentence("Reading aloud failed", &e));
                    }
                }
            }
        });
    });
}

/// Stop the spoken reply at once and drop the queued narration. Returns
/// false when nothing was speaking.
pub fn stop_speaking(store: Store) -> bool {
    NARRATION.with(|q| q.borrow_mut().clear());
    let was = store.voice.speaking.get_untracked() != Speaking::Idle;
    voice_host::host().stop_speech();
    if was {
        store.voice.speaking.set(Speaking::Idle);
    }
    store.voice.speaking_text.set(None);
    was
}

// -- dictation -------------------------------------------------------------------

/// Ctrl+R / `/dictate`: start recording, or stop and transcribe.
pub fn toggle_dictation(store: Store, ctx: &UiCtx) {
    match store.voice.dictation.get_untracked() {
        Dictation::Idle => {
            start_dictation(store, ctx, false);
        }
        Dictation::Recording { gen, .. } => {
            voice_host::host().send_if_running(&json!({"op": "record_stop", "gen": gen}));
        }
        Dictation::Transcribing { .. } => store.notify("Still transcribing the last recording."),
    }
}

/// Hold Space (`ui::push_to_talk`): start a recording whose transcript is
/// SENT. False when it could not start (another dictation is running, or
/// no session) — the caller forgets the hold.
pub fn start_hold(store: Store, ctx: &UiCtx) -> bool {
    match store.voice.dictation.get_untracked() {
        Dictation::Idle => {}
        Dictation::Recording { .. } => {
            store.notify("A dictation is already recording (Ctrl+R stops it).");
            return false;
        }
        Dictation::Transcribing { .. } => {
            store.notify("Still transcribing the last recording.");
            return false;
        }
    }
    // Talking over the agent: silence it first (and drop its queued
    // narration), or the microphone would record the agent's own voice.
    stop_speaking(store);
    start_dictation(store, ctx, true)
}

/// The held Space went up: stop the hold's recording (it then transcribes
/// and sends). A no-op when Esc already cancelled it.
pub fn release_hold(store: Store) {
    if !store.voice.hold.get_untracked() {
        return;
    }
    if let Dictation::Recording { gen, .. } = store.voice.dictation.get_untracked() {
        voice_host::host().send_if_running(&json!({"op": "record_stop", "gen": gen}));
    }
}

/// A press that turned out to be a TAP (no hold): drop its recording
/// silently — nothing is transcribed or sent, and no notice, since a tap
/// on an empty prompt is not a mistake worth a sentence.
pub fn discard_hold(store: Store) {
    if !store.voice.hold.get_untracked() {
        return;
    }
    if let Dictation::Recording { gen, .. } = store.voice.dictation.get_untracked() {
        DICTATION.store(0, Ordering::SeqCst);
        voice_host::host().send_if_running(&json!({"op": "record_stop", "gen": gen}));
        store.voice.dictation.set(Dictation::Idle);
        store.voice.level.set(0.0);
        store.voice.hold.set(false);
    }
}

/// The run a transcription rides. The current run when there is one;
/// otherwise the SESSION's media run, `session_memory_<session id>`, which
/// the gateway creates on first use (`_load_or_create_session_memory_owner_run`
/// in abstractgateway routes/gateway.py) — the run Code web
/// (`session_memory_owner_run_id`) and Observer already transcribe on. It
/// lets the very first message of a session be spoken. `None` for a
/// session id outside the gateway's safe run-id alphabet, whose hashed
/// form this client does not compute.
pub fn transcription_run_id(run_id: &str, session_id: &str) -> Option<String> {
    if !run_id.is_empty() {
        return Some(run_id.to_string());
    }
    let safe = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    safe(session_id).then(|| format!("session_memory_{session_id}"))
}

fn start_dictation(store: Store, ctx: &UiCtx, hold: bool) -> bool {
    let session_id = store.session_id.get_untracked();
    let run_id = match transcription_run_id(&store.run_id.get_untracked(), &session_id) {
        Some(r) if !session_id.is_empty() => r,
        _ => {
            post_error(store, "Start a conversation to enable dictation.".into());
            return false;
        }
    };
    store.voice.hold.set(hold);
    store.voice.levels.set(Vec::new());
    load_defaults(store, ctx);
    let host = voice_host::host();
    let gen = host.next_gen();
    DICTATION.store(gen, Ordering::SeqCst);
    store.voice.dictation.set(Dictation::Recording {
        since: Instant::now(),
        gen,
    });
    store.voice.level.set(0.0);
    let p = prefs(ctx);
    let gw = VoiceGateway::new(&ctx.client);
    let wake = abstracttui::reactive::wake_handle();
    // The route a transcription will use, read on the UI thread now and
    // again after the recording (the defaults may land meanwhile).
    spawn("voice-dictate", move || {
        let level_wake = wake.clone();
        let recorded =
            voice::record_blocking(&host, gen, &p, voice::MAX_RECORDING_S, &mut |rms, bands| {
                let bands = voice::meter_bands(rms, bands);
                level_wake.post(move || {
                    store.voice.level.set(rms);
                    store.voice.levels.set(bands);
                });
            });
        let live = || DICTATION.load(Ordering::SeqCst) == gen;
        let finish = move |wake: &abstracttui::reactive::WakeHandle,
                           err: Option<String>,
                           text: Option<String>| {
            wake.post(move || {
                if DICTATION.load(Ordering::SeqCst) != gen {
                    return; // cancelled (Esc) or superseded
                }
                DICTATION.store(0, Ordering::SeqCst);
                store.voice.dictation.set(Dictation::Idle);
                store.voice.level.set(0.0);
                store.voice.hold.set(false);
                if let Some(e) = err {
                    post_error(store, e);
                }
                if let Some(t) = text {
                    if hold {
                        store.voice.hold_send.set(Some(t));
                    } else {
                        store.voice.transcript.set(Some(t));
                    }
                }
            });
        };
        let rec = match recorded {
            Ok(r) => r,
            Err(e) => {
                let msg = if e.ends_with('.') {
                    e
                } else {
                    error_sentence("The microphone did not start", &e)
                };
                return finish(&wake, Some(msg), None);
            }
        };
        let bytes = std::fs::read(&rec.path);
        let _ = std::fs::remove_file(&rec.path);
        if !live() {
            return;
        }
        if rec.duration_ms < voice::MIN_RECORDING_MS {
            let msg = if hold {
                "Too short to hear: hold Space while you speak, release to send."
            } else {
                "The recording was too short. Press Ctrl+R, speak, then press Ctrl+R again to transcribe."
            };
            return finish(&wake, Some(msg.into()), None);
        }
        if rec.peak < voice::SILENT_LEVEL {
            return finish(
                &wake,
                Some("Nothing was heard. Check the microphone in Settings → Voice (Test), then try again.".into()),
                None,
            );
        }
        let bytes = match bytes {
            Ok(b) => b,
            Err(e) => {
                return finish(
                    &wake,
                    Some(error_sentence(
                        "The recording could not be prepared",
                        &e.to_string(),
                    )),
                    None,
                )
            }
        };
        let since = Instant::now();
        let p2 = p.clone();
        wake.post(move || {
            if DICTATION.load(Ordering::SeqCst) == gen {
                let route = store
                    .voice
                    .defaults
                    .with_untracked(|d| stt_route_text(&p2, d.value()));
                store
                    .voice
                    .dictation
                    .set(Dictation::Transcribing { since, route });
            }
        });
        match gw.transcribe(&session_id, &run_id, &bytes, &p) {
            Ok(t) if !t.text.is_empty() => finish(&wake, None, Some(t.text)),
            Ok(_) => finish(
                &wake,
                Some("Nothing was heard. Check the microphone in Settings → Voice (Test), then try again.".into()),
                None,
            ),
            Err(e) => finish(&wake, Some(error_sentence("Transcription failed", &e)), None),
        }
    });
    true
}

/// Esc: cancel a recording, else stop the spoken reply. Consumes the press
/// when it did something (the next Esc keeps its usual meaning).
pub fn escape(store: Store) -> bool {
    if let Dictation::Recording { gen, .. } = store.voice.dictation.get_untracked() {
        DICTATION.store(0, Ordering::SeqCst);
        voice_host::host().send_if_running(&json!({"op": "record_stop", "gen": gen}));
        store.voice.dictation.set(Dictation::Idle);
        store.voice.level.set(0.0);
        store.voice.hold.set(false);
        store.notify("Recording cancelled.");
        return true;
    }
    stop_speaking(store)
}

// -- wiring ------------------------------------------------------------------------

/// Root wiring: read-aloud on each new reply, transcripts into the composer,
/// the one-second tick while something runs.
pub fn wire(cx: Scope, store: Store, ctx: &UiCtx, composer: abstracttui::widgets::TextAreaState) {
    // A finished transcription lands at the end of the draft.
    cx.effect(move || {
        let Some(text) = store.voice.transcript.get() else {
            return;
        };
        store.voice.transcript.set(None);
        let draft = composer.text();
        let joined = if draft.trim().is_empty() {
            text
        } else if draft.ends_with(char::is_whitespace) {
            format!("{draft}{text}")
        } else {
            format!("{draft} {text}")
        };
        composer.set_text(joined);
    });
    // READ ALOUD narrates the turn as it happens (operator ask
    // 2026-10-10: "the ai explaining what it does as it works"): each
    // reasoning cycle's own words when its record folds, then the final
    // answer — queued, one after the other. Only items that land while
    // a run is live in the agent lane are read (a restored or replayed
    // history is never narrated), and the answer is skipped when it
    // repeats the last cycle's words verbatim (the last cycle usually IS
    // the answer). The reasoning channel is never read.
    let seen = Rc::new(Cell::new(store.fold.with_untracked(|f| f.items.len())));
    let last_read: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let rctx = ctx.clone();
    cx.effect(move || {
        let len = store.fold.with(|f| f.items.len());
        let before = seen.replace(len);
        let live = store.phase.get_untracked() != Phase::Idle
            && matches!(store.focus.get_untracked(), crate::convo::Focus::Agent);
        if len <= before || !live || !prefs(&rctx).read_aloud {
            return;
        }
        let fresh: Vec<String> = store.fold.with_untracked(|f| {
            f.items[before..]
                .iter()
                .filter_map(|i| match i {
                    Item::Thinking { content, .. } if !content.trim().is_empty() => {
                        Some(content.trim().to_string())
                    }
                    Item::Assistant {
                        text,
                        final_answer: true,
                    } if !text.trim().is_empty() => Some(text.trim().to_string()),
                    _ => None,
                })
                .collect()
        });
        for text in fresh {
            if *last_read.borrow() == text {
                continue;
            }
            *last_read.borrow_mut() = text.clone();
            narrate(store, &rctx, text);
        }
    });
    // The next narration starts when the speaker goes quiet.
    let qctx = ctx.clone();
    cx.effect(move || {
        if store.voice.speaking.get() == Speaking::Idle {
            speak_next(store, &qctx);
        }
    });
    // Elapsed seconds for "Recording… 3 s" / "Transcribing… 4 s".
    let _ = abstracttui::reactive::interval(cx, Duration::from_millis(500), move || {
        if store
            .voice
            .dictation
            .with_untracked(|d| !matches!(d, Dictation::Idle))
        {
            store.voice.tick.update(|t| *t += 1);
        }
    });
}

/// The status line shown above the composer while voice is busy (the
/// RECORDING states render as the live meter instead — see
/// [`recording_parts`]): "♪ Speaking… · Esc stops", "Transcribing… 4 s ·
/// faster-whisper / large-v3" (a hold adds "· sends when ready").
pub fn status_text(store: Store, now: Instant) -> Option<String> {
    match store.voice.dictation.get() {
        Dictation::Recording { .. } => {
            return recording_parts(store, now).map(|(l, h)| format!("{l} · {h}"))
        }
        Dictation::Transcribing { since, route } => {
            let line = transcribing_line(since, now, &route);
            return Some(if store.voice.hold.get() {
                format!("{line} · sends when ready")
            } else {
                line
            });
        }
        Dictation::Idle => {}
    }
    match store.voice.speaking.get() {
        Speaking::Preparing => Some("♪ Preparing speech… · Esc stops".into()),
        Speaking::Playing | Speaking::Paused => {
            speaking_parts(store).map(|(l, h)| format!("{l} · {h}"))
        }
        Speaking::Idle => None,
    }
}

/// While a reply is READ: ("♪ Speaking…", "Ctrl+P pauses · Esc stops") or,
/// paused, ("‖ Paused", "Ctrl+P resumes · Esc stops") — rendered as the
/// meter row over the speaker's levels. `None` otherwise.
pub fn speaking_parts(store: Store) -> Option<(String, &'static str)> {
    match store.voice.speaking.get() {
        Speaking::Playing => Some(("♪ Speaking…".into(), "Ctrl+P pauses · Esc stops")),
        Speaking::Paused => Some(("‖ Paused".into(), "Ctrl+P resumes · Esc stops")),
        _ => None,
    }
}

/// A recording's label and hint: ("● Listening… 3 s", "release Space to
/// send · Esc cancels") for a hold, ("● Recording… 3 s", "Ctrl+R
/// transcribes · Esc cancels") for Ctrl+R. `None` when not recording.
pub fn recording_parts(store: Store, now: Instant) -> Option<(String, &'static str)> {
    let Dictation::Recording { since, .. } = store.voice.dictation.get() else {
        return None;
    };
    let secs = voice::elapsed_seconds(since, now);
    Some(if store.voice.hold.get() {
        (
            format!("● Listening… {secs}"),
            "release Space to send · Esc cancels",
        )
    } else {
        (
            format!("● Recording… {secs}"),
            "Ctrl+R transcribes · Esc cancels",
        )
    })
}

/// Meter width in cells: wide enough to read as a voice trace (~3 s at
/// the bridge's 10 levels a second), narrow enough for 80 columns.
pub const METER_CELLS: usize = 28;

/// One voice meter row: `<glyph><label>  ▂▃▅▇▅▃  <hint>`. The glyph (the
/// label's first char) takes `glyph_ink` — red for a live microphone,
/// accent for the speaker. The bars are a LIVE HISTOGRAM of the sound
/// right now — its spectrum, low frequencies left (`voice::meter_cells`),
/// updated ~20 times a second (operator ask 2026-10-10: current values,
/// not a trace through time) — in the thinking wave's block glyphs and
/// inks (trough → floored crest). The meter yields to the hint's 12-column
/// floor, then shrinks; the hint ellipsizes last.
fn meter_row(
    t: &TokenSet,
    label: String,
    glyph_ink: Rgba,
    levels: Vec<f32>,
    hint: &'static str,
) -> View {
    let label_ink = t.text;
    let hint_ink = t.text_faint;
    let trough = t.text_faint;
    let crest = crate::ui::logo::floored_highlights(t).1;
    Element::new()
        .style(LayoutStyle::line(1).shrink(0.0))
        .draw(move |canvas, rect| {
            if rect.w <= 2 {
                return;
            }
            let mut x = rect.x + 1;
            let split = label.char_indices().nth(1).map_or(label.len(), |(i, _)| i);
            let (glyph, rest) = label.split_at(split);
            x += canvas.print(Point::new(x, rect.y), glyph, glyph_ink, Rgba::TRANSPARENT);
            x += canvas.print(Point::new(x, rect.y), rest, label_ink, Rgba::TRANSPARENT);
            x += 2;
            let room = (rect.right() - 1 - x - 2 - 12).max(0) as usize;
            let n = METER_CELLS.min(room);
            for (i, w) in voice::meter_cells(&levels, n).into_iter().enumerate() {
                canvas.put(
                    Point::new(x + i as i32, rect.y),
                    crate::ui::thinking::glyph_for(w),
                    crate::ui::logo::lerp_ink(trough, crest, w),
                    Rgba::TRANSPARENT,
                );
            }
            if n > 0 {
                x += n as i32 + 2;
            }
            let avail = rect.right() - 1 - x;
            if avail >= 4 {
                let fitted = text::truncate_ellipsis(hint, avail);
                canvas.print(Point::new(x, rect.y), &fitted, hint_ink, Rgba::TRANSPARENT);
            }
        })
        .build()
}

pub fn status_row(store: Store) -> View {
    dyn_view(LayoutStyle::default().shrink(0.0), move || {
        let _ = store.voice.tick.get();
        let t = abstracttui::app::current_theme().tokens;
        // RECORDING: the live voice meter (operator ask 2026-10-10: "a
        // widget, like for thinking, but based on the voice being
        // recorded") — `● Listening… 3 s ▂▃▅▇▅▃▂ release Space to send ·
        // Esc cancels`. The bars are the microphone's own levels, newest
        // at the right, in the thinking wave's block glyphs and inks
        // (trough → floored crest), so a quiet room reads flat and
        // speech visibly moves. Re-renders per level (~10/s) only while
        // recording.
        if let Some((label, hint)) = recording_parts(store, Instant::now()) {
            let levels = store.voice.levels.get();
            return meter_row(&t, label, t.error, levels, hint);
        }
        // SPEAKING: the same meter over the SPEAKER's levels (operator ask
        // 2026-10-10: "when the ai speaks, show that histogram of voice").
        if let Some((label, hint)) = speaking_parts(store) {
            let levels = store.voice.speak_levels.get();
            return meter_row(&t, label, t.accent, levels, hint);
        }
        let Some(line) = status_text(store, Instant::now()) else {
            return Element::new().build();
        };
        let ink = t.accent;
        Element::new()
            .style(LayoutStyle::line(1).shrink(0.0))
            .draw(move |canvas, rect| {
                let fitted = text::truncate_ellipsis(&line, (rect.w - 2).max(4));
                canvas.print(
                    Point::new(rect.x + 1, rect.y),
                    &fitted,
                    ink,
                    Rgba::TRANSPARENT,
                );
            })
            .build()
    })
}

// -- commands ------------------------------------------------------------------------

pub fn command_voice(cx: Scope, store: Store, ctx: &UiCtx, arg: Option<String>) {
    let Some(arg) = arg else {
        open_voice_settings(cx, store, ctx);
        return;
    };
    let words: Vec<String> = arg
        .split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .collect();
    match words
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["read-aloud", v] | ["read", "aloud", v] if matches!(*v, "on" | "off") => {
            let mut p = prefs(ctx);
            p.read_aloud = *v == "on";
            save_prefs(store, ctx, &p);
            store.notify(format!(
                "Read aloud {}",
                if p.read_aloud { "[x]" } else { "[ ]" }
            ));
        }
        ["space", v] | ["hold-space", v] | ["space-to-talk", v] if matches!(*v, "on" | "off") => {
            let mut p = prefs(ctx);
            p.space_to_talk_off = *v == "off";
            save_prefs(store, ctx, &p);
            store.notify(if p.space_to_talk_off {
                "Hold Space to talk: off (Ctrl+R still dictates into the draft)"
            } else {
                "Hold Space to talk: on — on an empty prompt, hold Space, speak, release to send"
            });
        }
        _ => post_error(
            store,
            format!("/voice takes no argument, read-aloud on|off, or space on|off (got {arg:?})."),
        ),
    }
}

pub fn command_speak(store: Store, ctx: &UiCtx, arg: Option<String>) {
    match arg.as_deref().map(str::trim) {
        Some("stop") => {
            stop_speaking(store);
        }
        Some(other) if !other.is_empty() => post_error(
            store,
            format!("/speak takes no argument or stop (got {other:?})."),
        ),
        _ => toggle_speak(store, ctx),
    }
}

// -- the /voice screen ---------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Row {
    None,
    Tts,
    Stt,
    Output,
    TestSpeaker,
    Volume,
    Input,
    TestMic,
    Language,
    Gain,
    ReadAloud,
    Latency,
}

fn percent(v: f64) -> String {
    format!("{} %", (v * 100.0).round() as i64)
}

fn meter(level: f32) -> String {
    let n = (level.clamp(0.0, 1.0) * 10.0).round() as usize;
    format!("{}{}", "▮".repeat(n), "▯".repeat(10 - n))
}

fn device_name(list: &[(String, String)], id: &str, saved_missing: &str) -> String {
    if id.is_empty() {
        return "System default".into();
    }
    list.iter()
        .find(|(i, _)| i == id)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| saved_missing.to_string())
}

fn engine_value(summary: &str, over: &str) -> String {
    if !over.is_empty() {
        format!("{over} (this app)")
    } else if summary.is_empty() {
        "Gateway default".into()
    } else {
        format!("Gateway default · {summary}")
    }
}

/// The screen's rows: (label, action). Pure over the signals it reads, so
/// the picker rebuilds live as answers land.
fn settings_rows(store: Store, p: &VoicePrefs) -> Vec<(String, Row)> {
    let v = store.voice;
    let defaults = v.defaults.get();
    let d = defaults.value();
    let failed = defaults.failed();
    let tts = default_summary(d, VoiceKind::Tts, failed);
    let stt = default_summary(d, VoiceKind::Stt, failed);
    let devices = v.devices.get();
    let (outs, ins, dev_note) = match &devices {
        Devices::Loaded { output, input } => (output.clone(), input.clone(), None),
        Devices::Failed(e) => (Vec::new(), Vec::new(), Some(e.clone())),
        Devices::Loading | Devices::Unknown => (Vec::new(), Vec::new(), None),
    };
    let loading = matches!(devices, Devices::Loading | Devices::Unknown);
    // "Not connected" is a claim only a loaded list can make; when the list
    // failed, nothing is known about the saved device.
    let listed = matches!(devices, Devices::Loaded { .. });
    let (out_missing, in_missing) = if listed {
        (
            "System default · saved speaker not connected",
            "System default · saved microphone not connected",
        )
    } else {
        ("Saved speaker", "Saved microphone")
    };
    let label = |name: &str, value: String| format!("  {name:<17} {value}");
    let mut rows: Vec<(String, Row)> = Vec::new();
    rows.push((
        "Engines — which engines speak and listen.".into(),
        Row::None,
    ));
    rows.push((
        label(
            "Text → speech",
            engine_value(&tts, &p.tts_override_summary()),
        ),
        Row::Tts,
    ));
    rows.push((
        label(
            "Speech → text",
            engine_value(&stt, &p.stt_override_summary()),
        ),
        Row::Stt,
    ));
    if failed {
        rows.push((
            "  The gateway's default voice routes could not be read. Requests still use them."
                .into(),
            Row::None,
        ));
    }
    if let Some(d) = d {
        for e in [d.tts.as_ref(), d.stt.as_ref()].into_iter().flatten() {
            if !e.configured && !e.note.is_empty() {
                rows.push((format!("  {}", e.note), Row::None));
            }
            if e.configured && !e.hint.is_empty() {
                // The gateway's served sentence, verbatim (round 16), wrapped to the picker
                // so no part of it is cut off.
                for line in text::wrap(&e.hint, 80) {
                    rows.push((format!("  {line}"), Row::None));
                }
            }
        }
    }
    rows.push(("Output".into(), Row::None));
    let out_value = if loading && !p.output_device.is_empty() {
        "…".into()
    } else {
        device_name(&outs, &p.output_device, out_missing)
    };
    rows.push((label("Output device", out_value), Row::Output));
    let speaker = v
        .speaker_note
        .get()
        .map(|(t, _)| t)
        .unwrap_or_else(|| "Enter plays a short chime".into());
    rows.push((label("Test speaker", speaker), Row::TestSpeaker));
    rows.push((
        label("Reply volume", format!("{}  ←/→", percent(p.volume()))),
        Row::Volume,
    ));
    rows.push(("Microphone".into(), Row::None));
    let in_value = if loading && !p.input_device.is_empty() {
        "…".into()
    } else {
        device_name(&ins, &p.input_device, in_missing)
    };
    rows.push((label("Input device", in_value), Row::Input));
    let mic = match v.mic_note.get() {
        Some((t, _)) if v.mic_testing.get() => format!("{}  {t}", meter(v.level.get())),
        Some((t, _)) => t,
        None => "Enter records 3 s and plays it back".into(),
    };
    rows.push((label("Test microphone", mic), Row::TestMic));
    if let Some(e) = dev_note {
        rows.push((format!("  {e}"), Row::None));
    }
    let lang = voice::LANGUAGES
        .iter()
        .find(|(k, _)| *k == p.stt_language)
        .map(|(_, l)| l.to_string())
        .unwrap_or_else(|| p.stt_language.clone());
    rows.push((label("Spoken language", lang), Row::Language));
    rows.push((
        label("Input level", format!("{}  ←/→", percent(p.gain()))),
        Row::Gain,
    ));
    rows.push(("Replies".into(), Row::None));
    rows.push((
        format!(
            "  {} Read aloud — Narrate each cycle as the agent works, then the reply.",
            if p.read_aloud { "[x]" } else { "[ ]" }
        ),
        Row::ReadAloud,
    ));
    let latency = voice::LATENCY
        .iter()
        .find(|(k, _)| *k == p.quality_preset)
        .map(|(_, l)| l.to_string())
        .unwrap_or_else(|| p.quality_preset.clone());
    rows.push((label("Voice latency", latency), Row::Latency));
    if let Some(line) = v.last_reply.get() {
        rows.push((format!("Last reply: {line}"), Row::None));
    }
    rows
}

/// One line of help for the selected row (the kit's helper sentences).
fn row_help(row: Row) -> &'static str {
    match row {
        Row::Tts | Row::Stt => "Enter: Gateway default or an override for this app",
        Row::Output => "Enter picks the speaker (AbstractVoice on this computer)",
        Row::Volume => "←/→ changes the reply volume",
        Row::Gain => "Raises a quiet microphone · ←/→",
        Row::Language => "Naming it skips detection: transcription is faster.",
        Row::Latency => "Trades quality for a faster first word.",
        Row::ReadAloud => "Enter switches it",
        Row::TestSpeaker | Row::TestMic | Row::Input => "Enter",
        Row::None => "",
    }
}

fn load_devices(store: Store) {
    if matches!(store.voice.devices.get_untracked(), Devices::Loading) {
        return;
    }
    store.voice.devices.set(Devices::Loading);
    let wake = abstracttui::reactive::wake_handle();
    spawn("voice-devices", move || {
        let state = match voice::devices_blocking(&voice_host::host()) {
            Ok((output, input)) => Devices::Loaded { output, input },
            Err(e) => Devices::Failed(e),
        };
        wake.post(move || store.voice.devices.set(state));
    });
}

pub fn open_voice_settings(cx: Scope, store: Store, ctx: &UiCtx) {
    if store
        .voice
        .defaults
        .with_untracked(|d| !matches!(d, DefaultsState::Loaded(_)))
    {
        store.voice.defaults.set(DefaultsState::Unknown);
    }
    load_defaults(store, ctx);
    if !matches!(store.voice.devices.get_untracked(), Devices::Loaded { .. }) {
        load_devices(store);
    }
    let selected = Rc::new(Cell::new(1usize));
    let rows_ctx = ctx.clone();
    let rows_of = move || {
        let _ = store.voice.tick.get();
        settings_rows(store, &prefs(&rows_ctx))
            .into_iter()
            .map(|(l, _)| l)
            .collect::<Vec<_>>()
    };
    let n = rows_of().len() as i32;
    let choose_ctx = ctx.clone();
    let choose_cx = cx;
    let adjust = |delta: f64, store: Store, ctx: UiCtx, selected: Rc<Cell<usize>>| {
        move || {
            let rows = settings_rows(store, &prefs(&ctx));
            let Some((_, row)) = rows.get(selected.get()) else {
                return;
            };
            let mut p = prefs(&ctx);
            match row {
                Row::Volume => {
                    p.reply_volume = Some(((p.volume() + delta) * 20.0).round() / 20.0)
                        .map(|v| v.clamp(0.0, 1.0))
                }
                Row::Gain => {
                    p.input_gain =
                        Some(((p.gain() + delta) * 10.0).round() / 10.0).map(|v| v.clamp(0.5, 1.5))
                }
                _ => return,
            }
            save_prefs(store, &ctx, &p);
        }
    };
    let help_ctx = ctx.clone();
    let help_sel = selected.clone();
    let obs_sel = selected.clone();
    open_picker(
        cx,
        ctx,
        Picker {
            title: "Voice · Enter changes · ←/→ adjusts · Esc closes".into(),
            labels: rows_of(),
            live: Some(Rc::new(rows_of)),
            start: 1,
            size: modal_size(96, n + 8),
            hint: None,
            live_hint: Some(Rc::new(move || {
                let _ = store.voice.tick.get();
                let rows = settings_rows(store, &prefs(&help_ctx));
                rows.get(help_sel.get())
                    .map(|(_, r)| row_help(*r))
                    .unwrap_or("")
                    .to_string()
            })),
            keys: vec![
                (
                    KeyChord::plain(Key::Left),
                    Rc::new(adjust(-0.1, store, ctx.clone(), selected.clone())) as Rc<dyn Fn()>,
                ),
                (
                    KeyChord::plain(Key::Right),
                    Rc::new(adjust(0.1, store, ctx.clone(), selected.clone())) as Rc<dyn Fn()>,
                ),
            ],
            on_mount: None,
            on_selection: Some(Box::new(move |ix| {
                obs_sel.set(ix);
                store.voice.tick.update(|t| *t += 1);
            })),
            on_choose: Box::new(move |ix| {
                let rows = settings_rows(store, &prefs(&choose_ctx));
                let Some((_, row)) = rows.get(ix) else { return };
                activate(choose_cx, store, &choose_ctx, *row);
            }),
            on_cancel: None,
        },
    );
}

fn activate(cx: Scope, store: Store, ctx: &UiCtx, row: Row) {
    let mut p = prefs(ctx);
    match row {
        Row::ReadAloud => {
            p.read_aloud = !p.read_aloud;
            save_prefs(store, ctx, &p);
        }
        Row::Volume => {
            p.reply_volume = Some(if p.volume() <= 0.0 {
                1.0
            } else {
                ((p.volume() - 0.25) * 4.0).round() / 4.0
            });
            save_prefs(store, ctx, &p);
        }
        Row::Gain => {
            p.input_gain = Some(if p.gain() >= 1.5 {
                0.5
            } else {
                p.gain() + 0.25
            });
            save_prefs(store, ctx, &p);
        }
        Row::TestSpeaker => test_speaker(store, &p),
        Row::TestMic => test_microphone(store, &p),
        Row::Output | Row::Input => open_device_picker(cx, store, ctx, row == Row::Output),
        Row::Language => open_choice_picker(
            cx,
            store,
            ctx,
            "Spoken language",
            voice::LANGUAGES,
            p.stt_language.clone(),
            |p, v| p.stt_language = v,
        ),
        Row::Latency => open_choice_picker(
            cx,
            store,
            ctx,
            "Voice latency",
            voice::LATENCY,
            p.quality_preset.clone(),
            |p, v| p.quality_preset = v,
        ),
        Row::Tts | Row::Stt => open_engine_picker(cx, store, ctx, row == Row::Tts),
        Row::None => {}
    }
}

fn test_speaker(store: Store, p: &VoicePrefs) {
    store
        .voice
        .speaker_note
        .set(Some(("Playing a short chime…".into(), Tone::Info)));
    let p = p.clone();
    let wake = abstracttui::reactive::wake_handle();
    spawn("voice-test-speaker", move || {
        let note = match voice::tone_blocking(&voice_host::host(), &p) {
            Ok(()) => (
                "Chime played. Heard nothing? Pick another output or raise the volume.".to_string(),
                Tone::Ok,
            ),
            Err(e) => (error_sentence("The speaker test failed", &e), Tone::Error),
        };
        wake.post(move || store.voice.speaker_note.set(Some(note)));
    });
}

fn test_microphone(store: Store, p: &VoicePrefs) {
    if store.voice.mic_testing.get_untracked() {
        return;
    }
    store.voice.mic_testing.set(true);
    store.voice.mic_note.set(Some((
        "Recording 3 seconds — say something.".into(),
        Tone::Info,
    )));
    let p = p.clone();
    let wake = abstracttui::reactive::wake_handle();
    spawn("voice-test-microphone", move || {
        let host = voice_host::host();
        let gen = host.next_gen();
        let lw = wake.clone();
        let rec = voice::record_blocking(&host, gen, &p, 3, &mut |rms, _| {
            lw.post(move || store.voice.level.set(rms))
        });
        let note = match rec {
            Err(e) => {
                let msg = if e.ends_with('.') {
                    e
                } else {
                    error_sentence("The microphone did not start", &e)
                };
                (msg, Tone::Error)
            }
            Ok(r) if r.peak < voice::SILENT_LEVEL => {
                let _ = std::fs::remove_file(&r.path);
                (
                    "The microphone recorded silence. Pick another input, or raise its level in the system sound settings."
                        .to_string(),
                    Tone::Error,
                )
            }
            Ok(r) => {
                wake.post(move || {
                    store
                        .voice
                        .mic_note
                        .set(Some(("Playing it back…".into(), Tone::Info)))
                });
                let played = voice::play_file_blocking(&host, &r.path, &p);
                let _ = std::fs::remove_file(&r.path);
                match played {
                    Ok(()) => (
                        format!("The microphone works (peak level {}).", percent(r.peak)),
                        Tone::Ok,
                    ),
                    Err(e) => (error_sentence("The playback failed", &e), Tone::Error),
                }
            }
        };
        wake.post(move || {
            store.voice.mic_testing.set(false);
            store.voice.level.set(0.0);
            store.voice.mic_note.set(Some(note));
        });
    });
}

fn open_choice_picker(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    title: &str,
    options: &'static [(&'static str, &'static str)],
    current: String,
    apply: fn(&mut VoicePrefs, String),
) {
    let start = options.iter().position(|(k, _)| *k == current).unwrap_or(0);
    let pctx = ctx.clone();
    let back = ctx.clone();
    open_picker(
        cx,
        ctx,
        Picker {
            title: format!("{title} · Enter selects · Esc back"),
            labels: options.iter().map(|(_, l)| l.to_string()).collect(),
            live: None,
            start,
            size: modal_size(60, options.len() as i32 + 6),
            hint: None,
            live_hint: None,
            keys: Vec::new(),
            on_mount: None,
            on_selection: None,
            on_choose: Box::new(move |ix| {
                if let Some((k, _)) = options.get(ix) {
                    let mut p = prefs(&pctx);
                    apply(&mut p, k.to_string());
                    save_prefs(store, &pctx, &p);
                }
                open_voice_settings(cx, store, &pctx);
            }),
            on_cancel: Some(Box::new(move || open_voice_settings(cx, store, &back))),
        },
    );
}

fn open_device_picker(cx: Scope, store: Store, ctx: &UiCtx, output: bool) {
    let p = prefs(ctx);
    let saved = if output {
        p.output_device.clone()
    } else {
        p.input_device.clone()
    };
    let list = match store.voice.devices.get_untracked() {
        Devices::Loaded {
            output: o,
            input: i,
        } => {
            if output {
                o
            } else {
                i
            }
        }
        Devices::Failed(e) => {
            post_error(store, e);
            load_devices(store);
            return;
        }
        _ => {
            store.notify("Listing this computer's audio devices…");
            return;
        }
    };
    let mut choices: Vec<(String, String)> = vec![(String::new(), "System default".into())];
    choices.extend(list);
    if !saved.is_empty() && !choices.iter().any(|(id, _)| *id == saved) {
        choices.push((
            saved.clone(),
            if output {
                "Saved speaker (not connected)"
            } else {
                "Saved microphone (not connected)"
            }
            .into(),
        ));
    }
    let start = choices.iter().position(|(id, _)| *id == saved).unwrap_or(0);
    let labels = choices.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>();
    let pctx = ctx.clone();
    let back = ctx.clone();
    open_picker(
        cx,
        ctx,
        Picker {
            title: format!(
                "{} · Enter selects · Esc back",
                if output {
                    "Output device"
                } else {
                    "Input device"
                }
            ),
            size: modal_size(72, labels.len() as i32 + 6),
            labels,
            live: None,
            start,
            hint: None,
            live_hint: None,
            keys: Vec::new(),
            on_mount: None,
            on_selection: None,
            on_choose: Box::new(move |ix| {
                if let Some((id, _)) = choices.get(ix) {
                    let mut p = prefs(&pctx);
                    if output {
                        p.output_device = id.clone();
                        store.voice.speaker_note.set(None);
                    } else {
                        p.input_device = id.clone();
                        store.voice.mic_note.set(None);
                    }
                    save_prefs(store, &pctx, &p);
                }
                open_voice_settings(cx, store, &pctx);
            }),
            on_cancel: Some(Box::new(move || open_voice_settings(cx, store, &back))),
        },
    );
}

/// Engine choices from the voice catalog: TTS = the catalog's voices
/// (provider/model/voice), STT = providers × models. First row = Gateway default.
pub fn engine_choices(
    catalog: &Value,
    tts: bool,
    defaults: Option<&VoiceDefaults>,
) -> Vec<(String, VoicePrefs)> {
    let names = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| match x {
                        Value::String(s) => Some(s.clone()),
                        o => o
                            .get("id")
                            .or_else(|| o.get("name"))
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let kind = if tts { VoiceKind::Tts } else { VoiceKind::Stt };
    let summary = default_summary(defaults, kind, false);
    let mut out = vec![(engine_value(&summary, ""), VoicePrefs::default())];
    if tts {
        let items = catalog
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for it in items {
            let id = it
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                continue;
            }
            let provider = it
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let model = it
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let label = it
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string();
            let profile = it.get("voice_kind").and_then(Value::as_str) == Some("profile");
            let route = voice::route_text(&provider, &model);
            let mut p = VoicePrefs {
                provider,
                model,
                ..Default::default()
            };
            if profile {
                p.profile = id;
            } else {
                p.voice = id;
            }
            out.push((
                if route.is_empty() {
                    label
                } else {
                    format!("{label} · {route}")
                },
                p,
            ));
        }
    } else {
        for provider in names(catalog.get("stt_providers")) {
            let models = names(
                catalog
                    .get("stt_models_by_provider")
                    .and_then(|m| m.get(&provider))
                    .or_else(|| catalog.get("stt_models")),
            );
            if models.is_empty() {
                out.push((
                    provider.clone(),
                    VoicePrefs {
                        stt_provider: provider.clone(),
                        ..Default::default()
                    },
                ));
            }
            for m in models {
                out.push((
                    voice::route_text(&provider, &m),
                    VoicePrefs {
                        stt_provider: provider.clone(),
                        stt_model: m,
                        ..Default::default()
                    },
                ));
            }
        }
    }
    out
}

thread_local! {
    /// The context an engine picker opens with once the catalog answers
    /// (`UiCtx` holds `Rc`s: it stays on the UI thread; the voice thread
    /// carries only the answer).
    static ENGINE_PICKER: RefCell<Option<(Scope, UiCtx, bool)>> = const { RefCell::new(None) };
}

fn open_engine_picker(cx: Scope, store: Store, ctx: &UiCtx, tts: bool) {
    store.notify("Asking the gateway for its voice engines…");
    ENGINE_PICKER.with(|slot| *slot.borrow_mut() = Some((cx, ctx.clone(), tts)));
    let gw = VoiceGateway::new(&ctx.client);
    let wake = abstracttui::reactive::wake_handle();
    spawn("voice-catalog", move || {
        let res = gw.catalog("", "");
        wake.post(move || {
            let Some((cx, ctx, tts)) = ENGINE_PICKER.with(|slot| slot.borrow_mut().take()) else {
                return;
            };
            match res {
                Err(e) => post_error(
                    store,
                    error_sentence("The voice engines could not be listed", &e),
                ),
                Ok(catalog) => show_engine_picker(cx, store, &ctx, tts, &catalog),
            }
        });
    });
}

fn show_engine_picker(cx: Scope, store: Store, ctx: &UiCtx, tts: bool, catalog: &Value) {
    let defaults = store.voice.defaults.with_untracked(|d| d.value().cloned());
    let choices = engine_choices(catalog, tts, defaults.as_ref());
    let p = prefs(ctx);
    let start = choices
        .iter()
        .position(|(_, c)| {
            if tts {
                c.provider == p.provider
                    && c.model == p.model
                    && c.voice == p.voice
                    && c.profile == p.profile
            } else {
                c.stt_provider == p.stt_provider && c.stt_model == p.stt_model
            }
        })
        .unwrap_or(0);
    let labels = choices.iter().map(|(l, _)| l.clone()).collect::<Vec<_>>();
    let cctx = ctx.clone();
    let back = ctx.clone();
    open_picker(
        cx,
        ctx,
        Picker {
            title: format!(
                "{} · Enter selects · Esc back",
                if tts {
                    "Text → speech"
                } else {
                    "Speech → text"
                }
            ),
            size: modal_size(84, labels.len() as i32 + 6),
            labels,
            live: None,
            start,
            hint: Some(
                "An override applies to this app only; Gateway default follows the gateway.".into(),
            ),
            live_hint: None,
            keys: Vec::new(),
            on_mount: None,
            on_selection: None,
            on_choose: Box::new(move |ix| {
                if let Some((_, c)) = choices.get(ix) {
                    let mut p = prefs(&cctx);
                    if tts {
                        p.provider = c.provider.clone();
                        p.model = c.model.clone();
                        p.voice = c.voice.clone();
                        p.profile = c.profile.clone();
                    } else {
                        p.stt_provider = c.stt_provider.clone();
                        p.stt_model = c.stt_model.clone();
                    }
                    save_prefs(store, &cctx, &p);
                }
                open_voice_settings(cx, store, &cctx);
            }),
            on_cancel: Some(Box::new(move || open_voice_settings(cx, store, &back))),
        },
    );
}
