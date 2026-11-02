//! Explicit side-assistant audio destination over the shared host audio owner.
use super::{voice_view, UiCtx};
use crate::{docs_chat::ChatScope, store::Store, voice::VoicePrefs};

/// Start/stop this side scope's microphone. Results remain editable in that
/// conversation's draft; this function never submits a task or question.
pub fn toggle_microphone(store: Store, ctx: &UiCtx, scope: ChatScope) {
    voice_view::toggle_assistant_dictation(store, ctx, scope);
}
pub fn is_recording_for(store: Store, scope: ChatScope) -> bool {
    voice_view::is_assistant_recording(store, scope)
}
/// Speak with the companion's synthesis preset through the one existing player.
/// Terminal device and volume settings remain those of the terminal user.
pub fn speak(store: Store, ctx: &UiCtx, text: String, voice: &VoicePrefs) {
    voice_view::speak_text_with_prefs(store, ctx, text, voice);
}

/// Choose the companion's independent speaking preset without editing main voice.
pub fn open_voice_picker(cx: abstracttui::prelude::Scope, store: Store, ctx: &UiCtx) {
    voice_view::open_companion_voice_picker(cx, store, ctx);
}

/// Queue automatic companion speech behind current narration, with source guards.
pub fn narrate(store: Store, ctx: &UiCtx, scope: ChatScope, text: String, voice: &VoicePrefs) {
    voice_view::narrate_assistant(store, ctx, scope, text, voice);
}
