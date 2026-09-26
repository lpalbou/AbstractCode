//! The "Stream replies" setting (contract S): whether new runs ask the
//! gateway to stream the model's reply token by token.
//!
//! Three states, one wire key. `gateway_default` (the default) sends
//! NOTHING — the gateway applies its own `agents.streaming_default`.
//! `off` ALWAYS sends `_runtime.stream: false` — the user's "off" must never
//! be left to a gateway default (REVIEW/15). `on` sends `true` ONLY to a
//! gateway whose `GET /discovery/capabilities` advertises
//! `streaming: {deltas: true, default: bool}` (CONTRACTS.md S-2 §6).
//!
//! Why the capability gate: an older gateway does not reject an unknown
//! `_runtime` key (it passes `input_data._runtime` through unvalidated —
//! abstractgateway `routes/gateway.py` validates only `speculation`), but
//! the agent layer has honoured `_runtime.stream: true` since 2026-07
//! (`abstractagent/adapters/generation_params.py`): the provider call
//! streams and is re-aggregated server-side with no live frames to show
//! for it, and some OpenAI-compatible servers then report no usage. So
//! "on" against such a gateway would change how the call runs while
//! showing nothing — `true` is withheld and the header/picker (and one
//! transcript notice per session) say why. `false` is harmless there and
//! always rides.

/// The stored preference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StreamReplies {
    /// Send nothing; the gateway's `agents.streaming_default` decides.
    #[default]
    GatewayDefault,
    On,
    Off,
}

impl StreamReplies {
    /// Parse the prefs word or a command/flag argument. `None` = not a
    /// word this setting knows (callers refuse it out loud).
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "gateway_default" | "gateway-default" | "default" | "gateway" | "inherit" => {
                Some(Self::GatewayDefault)
            }
            "on" | "true" | "yes" => Some(Self::On),
            "off" | "false" | "no" => Some(Self::Off),
            _ => None,
        }
    }

    /// The prefs.json spelling.
    pub fn word(self) -> &'static str {
        match self {
            Self::GatewayDefault => "gateway_default",
            Self::On => "on",
            Self::Off => "off",
        }
    }

    /// What people read (picker rows, notices).
    pub fn label(self) -> &'static str {
        match self {
            Self::GatewayDefault => "Gateway default",
            Self::On => "On",
            Self::Off => "Off",
        }
    }

    pub const ALL: [StreamReplies; 3] = ALL_ROWS;
}

/// The picker's row order.
pub const ALL_ROWS: [StreamReplies; 3] = [
    StreamReplies::GatewayDefault,
    StreamReplies::On,
    StreamReplies::Off,
];

/// The `_runtime.stream` value a new run carries. `deltas` is the
/// gateway's `streaming.deltas` capability: `Some(true)` advertised,
/// `Some(false)` known absent, `None` not known yet (capabilities not
/// loaded). `Off` always carries `false`; `On` carries `true` only for an
/// advertised capability; the default carries nothing.
pub fn run_input_value(pref: StreamReplies, deltas: Option<bool>) -> Option<bool> {
    match pref {
        StreamReplies::GatewayDefault => None,
        StreamReplies::On => (deltas == Some(true)).then_some(true),
        StreamReplies::Off => Some(false),
    }
}

/// True when "on" was chosen but this gateway is KNOWN not to stream —
/// the case that earns the once-per-session transcript notice.
pub fn on_but_unsupported(pref: StreamReplies, deltas: Option<bool>) -> bool {
    pref == StreamReplies::On && deltas == Some(false)
}

/// The once-per-session transcript notice for [`on_but_unsupported`].
pub const UNSUPPORTED_NOTICE: &str = "Stream replies is on, but this gateway does not support streaming (no \"streaming.deltas\" capability) — answers appear when each call completes";

/// The header chip: empty for the default; otherwise `stream on|off`,
/// with the reason when the gateway cannot honour it.
pub fn chip(pref: StreamReplies, deltas: Option<bool>) -> String {
    let base = match pref {
        StreamReplies::GatewayDefault => return String::new(),
        StreamReplies::On => "stream on",
        // Off is honoured by every gateway (the key always rides).
        StreamReplies::Off => return "stream off".into(),
    };
    match deltas {
        Some(true) => base.to_string(),
        Some(false) => format!("{base} (gateway has no live replies)"),
        None => format!("{base} (gateway not checked yet)"),
    }
}

/// The "Gateway default" row names what the gateway's default IS when
/// the gateway says (`streaming.default`).
pub fn gateway_default_label(streaming_default: Option<bool>) -> String {
    match streaming_default {
        Some(true) => "Gateway default (currently on)".into(),
        Some(false) => "Gateway default (currently off)".into(),
        None => "Gateway default".into(),
    }
}

/// One line explaining what the choice does against THIS gateway —
/// the picker's note and the `/stream` confirmation.
pub fn effect_note(pref: StreamReplies, deltas: Option<bool>) -> String {
    match (pref, deltas) {
        (StreamReplies::Off, _) => "new runs show the reply only when it is complete".into(),
        (_, Some(false)) => {
            "this gateway does not advertise live replies (no \"deltas\" capability): \
             new runs send no stream setting and answers appear when complete"
                .into()
        }
        (_, None) => "the gateway's capabilities are not known yet: new runs send no stream \
                      setting until they are"
            .into(),
        (StreamReplies::GatewayDefault, Some(true)) => {
            "new runs follow the gateway's streaming default".into()
        }
        (StreamReplies::On, Some(true)) => "new runs stream the reply as it is written".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_round_trip_and_junk_is_refused() {
        for s in StreamReplies::ALL {
            assert_eq!(StreamReplies::parse(s.word()), Some(s));
        }
        assert_eq!(StreamReplies::parse(" ON "), Some(StreamReplies::On));
        assert_eq!(
            StreamReplies::parse("default"),
            Some(StreamReplies::GatewayDefault)
        );
        assert_eq!(StreamReplies::parse("maybe"), None);
        assert_eq!(StreamReplies::default(), StreamReplies::GatewayDefault);
    }

    #[test]
    fn the_key_rides_only_for_on_or_off_against_an_advertising_gateway() {
        use StreamReplies::*;
        assert_eq!(run_input_value(GatewayDefault, Some(true)), None);
        assert_eq!(run_input_value(On, Some(true)), Some(true));
        assert_eq!(run_input_value(Off, Some(true)), Some(false));
        for deltas in [Some(false), None] {
            assert_eq!(run_input_value(On, deltas), None, "{deltas:?}");
            assert_eq!(run_input_value(GatewayDefault, deltas), None, "{deltas:?}");
            // REVIEW/15 rule 1: "off" is never left to the gateway default.
            assert_eq!(run_input_value(Off, deltas), Some(false), "{deltas:?}");
        }
        assert!(on_but_unsupported(On, Some(false)));
        assert!(
            !on_but_unsupported(On, None),
            "unknown is not 'unsupported'"
        );
        assert!(!on_but_unsupported(Off, Some(false)));
    }

    #[test]
    fn the_chip_is_silent_for_the_default_and_names_an_unsupported_gateway() {
        use StreamReplies::*;
        assert_eq!(chip(GatewayDefault, Some(true)), "");
        assert_eq!(chip(GatewayDefault, Some(false)), "");
        assert_eq!(chip(On, Some(true)), "stream on");
        assert_eq!(chip(Off, Some(true)), "stream off");
        assert_eq!(
            chip(On, Some(false)),
            "stream on (gateway has no live replies)"
        );
        assert!(chip(On, None).contains("not checked"));
        assert_eq!(
            chip(Off, Some(false)),
            "stream off",
            "off is honoured everywhere"
        );
    }
}
