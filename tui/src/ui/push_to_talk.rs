//! Hold Space to talk (operator ask 2026-10-10: "we press, it records, we
//! unpress, it sends").
//!
//! On an EMPTY composer, holding Space records from this computer's
//! microphone; releasing it stops the recording, transcribes it through
//! the gateway's speech-to-text route, and SENDS the words as the next
//! message — exactly what Enter would do with them (a task when idle,
//! guidance while a run streams, an entity message under entity focus).
//! A single tap still types a space; Space in a non-empty draft is
//! ordinary typing. Ctrl+R stays the hands-free toggle that only fills
//! the draft. `/voice space off` turns the gesture off.
//!
//! ## How a hold is seen
//!
//! Recording starts on the FIRST press, on every wire — nothing is typed
//! and the first syllable is never lost (operator report 2026-10-10: the
//! first version typed a space and waited for the key-repeat warmup
//! before listening, a visible lag). What differs per wire is how the
//! detector learns that the key is HELD and when it goes UP. The engine's
//! key-state service (`abstracttui::app::keys`) says which wire this is:
//!
//! - **Full fidelity** (the kitty keyboard protocol with event types:
//!   kitty, Ghostty, WezTerm, foot, iTerm2 ≥ 3.5 after the probe): the
//!   terminal reports the key-UP itself. Press records, key-up sends. A
//!   tap is a very short recording, which the voice lane drops as too
//!   short.
//! - **Legacy wire** (Terminal.app and every terminal without the
//!   protocol): only presses arrive. A HELD key auto-repeats after the
//!   operating system's "Delay until repeat"; if no repeat arrives within
//!   that delay plus a margin ([`tap_window`]), the press was a TAP and
//!   the recording is DISCARDED, unsent. Once repeats flow, the RELEASE is
//!   the moment they stop — the approach Claude Code's `/voice` takes on
//!   the same terminals. The engine deliberately ships no such
//!   approximation (`keys.rs`: "a dropped repeat would fabricate a
//!   release"), so the app owns the trade: a stall in the repeat stream
//!   longer than [`release_after`] ends the recording early and sends what
//!   was said so far. It fails toward "stop listening", never toward a
//!   stuck microphone; [`crate::voice::MAX_RECORDING_S`] bounds it
//!   regardless.
//!
//! The detector below is pure (time is a parameter) so both paths are
//! unit-tested without a terminal.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The operating system's "Delay until repeat" when it could not be read
/// (macOS ships 375–500 ms, X11 and Windows ~500–660 ms).
pub const DEFAULT_REPEAT_DELAY: Duration = Duration::from_millis(600);

/// Margin past the repeat delay before a silent press counts as a tap
/// (terminals deliver the first repeat a little late under load).
pub const TAP_MARGIN: Duration = Duration::from_millis(250);

/// The calibrated repeat delay in ms (0 = not read yet / unreadable).
static REPEAT_DELAY_MS: AtomicU64 = AtomicU64::new(0);

/// Set the "Delay until repeat" explicitly. An explicit value wins over
/// [`calibrate`], which only fills it in when nothing was set (tests pin
/// it so their timing does not depend on the machine's setting).
pub fn set_repeat_delay(d: Duration) {
    REPEAT_DELAY_MS.store(d.as_millis() as u64, Ordering::Relaxed);
}

/// How long a legacy-wire press waits for its first auto-repeat before it
/// counts as a TAP: the OS repeat delay plus [`TAP_MARGIN`].
pub fn tap_window() -> Duration {
    let ms = REPEAT_DELAY_MS.load(Ordering::Relaxed);
    let delay = if ms == 0 {
        DEFAULT_REPEAT_DELAY
    } else {
        Duration::from_millis(ms)
    };
    delay + TAP_MARGIN
}

/// macOS stores "Delay until repeat" as `InitialKeyRepeat` in units of
/// 15 ms (absent = the system default). Parses `defaults read` output.
pub fn parse_initial_key_repeat(out: &str) -> Option<Duration> {
    let units: u64 = out.trim().parse().ok()?;
    (units > 0).then(|| Duration::from_millis(units * 15))
}

/// Read the OS repeat delay once, off the UI thread (a `defaults`
/// subprocess on macOS; nothing elsewhere — the default stands). Read-only;
/// never overrides a value set with [`set_repeat_delay`].
pub fn calibrate() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("ptt-calibrate".into())
        .spawn(|| {
            let out = std::process::Command::new("defaults")
                .args(["read", "-g", "InitialKeyRepeat"])
                .stderr(std::process::Stdio::null())
                .output();
            if let Ok(o) = out {
                if let Some(d) = parse_initial_key_repeat(&String::from_utf8_lossy(&o.stdout)) {
                    let _ = REPEAT_DELAY_MS.compare_exchange(
                        0,
                        d.as_millis() as u64,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    );
                }
            }
        });
}

/// The window before the cadence of a hold is known (only the first
/// repeat seen): long enough for every supported repeat rate.
pub const FIRST_RELEASE_AFTER: Duration = Duration::from_millis(400);

/// How long after the last repeat a legacy-wire hold counts as released:
/// three repeat intervals, never under 300 ms (terminals deliver repeats
/// in small bursts, and a release must not fire inside one). macOS's
/// "Key repeat rate" runs from 30 ms (fastest) through 90 ms to 180 ms;
/// its slowest notches (450 ms+) release late but still release.
pub fn release_after(repeat_gap: Duration) -> Duration {
    (repeat_gap * 3).max(Duration::from_millis(300))
}

/// What the caller does with one Space press or one timer tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Not ours: the key types a space as usual.
    PassThrough,
    /// Ours: swallow the key (a repeat of a hold in progress).
    Consume,
    /// Start recording now and swallow the key.
    Start,
    /// It was a tap, not a hold: drop the recording, send nothing.
    Discard,
    /// The key went up: stop recording and send.
    Release,
    /// Nothing to do.
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Idle,
    /// Legacy wire: recording since the press, no repeat seen yet.
    Pending {
        at: Instant,
    },
    Holding {
        /// When the recording started (the first press).
        started: Instant,
        last: Instant,
        release_after: Duration,
        /// The wire reports the key-up itself (no timeout).
        full: bool,
    },
}

/// A key-up this soon after the press was a TAP (Full fidelity): the
/// recording is discarded instead of reported as too short.
pub const MIN_HOLD: Duration = Duration::from_millis(crate::voice::MIN_RECORDING_MS);

/// The hold detector. One per app; driven by the root key handler and a
/// short timer (see `arm_hold_release` in `ui::mod`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoldDetector {
    state: State,
}

impl Default for HoldDetector {
    fn default() -> Self {
        HoldDetector { state: State::Idle }
    }
}

impl HoldDetector {
    /// A recording this detector started is in progress (pending or held).
    pub fn active(&self) -> bool {
        !matches!(self.state, State::Idle)
    }

    /// A Space press arrived. `blank_draft`: the composer holds nothing
    /// but whitespace. `full`: the wire reports releases (kitty event
    /// types live).
    pub fn press(&mut self, now: Instant, blank_draft: bool, full: bool) -> Action {
        match self.state {
            State::Pending { at } => {
                // The first auto-repeat: a hold. Its cadence is not known
                // yet (this gap is the OS delay, not the repeat interval).
                self.state = State::Holding {
                    started: at,
                    last: now,
                    release_after: FIRST_RELEASE_AFTER,
                    full: false,
                };
                Action::Consume
            }
            State::Holding {
                started,
                last,
                release_after: _,
                full,
            } => {
                let gap = now.saturating_duration_since(last);
                self.state = State::Holding {
                    started,
                    last: now,
                    release_after: if full {
                        Duration::ZERO
                    } else {
                        release_after(gap)
                    },
                    full,
                };
                Action::Consume
            }
            State::Idle if !blank_draft => Action::PassThrough,
            State::Idle if full => {
                self.state = State::Holding {
                    started: now,
                    last: now,
                    release_after: Duration::ZERO,
                    full: true,
                };
                Action::Start
            }
            State::Idle => {
                self.state = State::Pending { at: now };
                Action::Start
            }
        }
    }

    /// The timer (legacy wire): no first repeat within the tap window =
    /// a TAP, discarded; repeats that stopped = RELEASED.
    pub fn tick(&mut self, now: Instant, tap_window: Duration) -> Action {
        match self.state {
            State::Pending { at } if now.saturating_duration_since(at) > tap_window => {
                self.state = State::Idle;
                Action::Discard
            }
            State::Holding {
                last,
                release_after,
                full: false,
                ..
            } if now.saturating_duration_since(last) > release_after => {
                self.state = State::Idle;
                Action::Release
            }
            _ => Action::Idle,
        }
    }

    /// The wire reported the key-up (Full fidelity), or the terminal lost
    /// focus / was suspended while the key was down (both wires: a hold
    /// that can no longer be observed ends — the engine's mic rule). A
    /// pending press that ends this way was never a hold: discarded, and
    /// so is a key-up within [`MIN_HOLD`] of the press (a tap).
    pub fn key_up(&mut self, now: Instant) -> Action {
        match self.state {
            State::Holding { started, .. } => {
                self.state = State::Idle;
                if now.saturating_duration_since(started) < MIN_HOLD {
                    Action::Discard
                } else {
                    Action::Release
                }
            }
            State::Pending { .. } => {
                self.state = State::Idle;
                Action::Discard
            }
            State::Idle => Action::Idle,
        }
    }

    /// Forget everything (Esc cancelled the recording, or it could not
    /// start).
    pub fn reset(&mut self) {
        self.state = State::Idle;
    }
}

/// The footer's voice hint for the current gesture setting.
pub fn hint(space_to_talk: bool) -> &'static str {
    if space_to_talk {
        "hold Space to talk"
    } else {
        "Ctrl+R speak"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Legacy wire: the FIRST press records at once (nothing typed); the
    /// first auto-repeat confirms the hold; when the repeats stop, it is
    /// released after three intervals (≥ 300 ms).
    #[test]
    fn legacy_hold_records_on_the_first_press_and_releases_when_repeats_stop() {
        let t0 = Instant::now();
        let win = ms(850);
        let mut d = HoldDetector::default();
        assert_eq!(d.press(t0, true, false), Action::Start, "instant");
        assert_eq!(
            d.tick(t0 + ms(400), win),
            Action::Idle,
            "within the OS delay"
        );
        assert_eq!(
            d.press(t0 + ms(500), true, false),
            Action::Consume,
            "first repeat"
        );
        let mut t = t0 + ms(500);
        for _ in 0..20 {
            t += ms(90);
            assert_eq!(d.press(t, true, false), Action::Consume);
            assert_eq!(d.tick(t + ms(50), win), Action::Idle, "mid-hold");
        }
        assert_eq!(d.tick(t + ms(290), win), Action::Idle);
        assert_eq!(d.tick(t + ms(310), win), Action::Release);
        assert!(!d.active());
        assert_eq!(d.tick(t + ms(900), win), Action::Idle, "one release only");
    }

    /// A tap: no repeat within the window — discarded, never sent.
    #[test]
    fn a_tap_is_discarded_after_the_tap_window() {
        let t0 = Instant::now();
        let mut d = HoldDetector::default();
        assert_eq!(d.press(t0, true, false), Action::Start);
        assert_eq!(d.tick(t0 + ms(800), ms(850)), Action::Idle);
        assert_eq!(d.tick(t0 + ms(900), ms(850)), Action::Discard);
        assert!(!d.active());
    }

    /// Space in a draft with words is typing, even at repeat speed.
    #[test]
    fn space_in_a_draft_is_typing() {
        let t0 = Instant::now();
        let mut d = HoldDetector::default();
        for i in 0..6 {
            assert_eq!(d.press(t0 + ms(40 * i), false, false), Action::PassThrough);
        }
        assert!(!d.active());
    }

    /// Full fidelity: press records, only the wire's key-up ends it —
    /// never a timeout.
    #[test]
    fn full_fidelity_starts_on_press_and_ends_on_the_wire_release() {
        let t0 = Instant::now();
        let mut d = HoldDetector::default();
        assert_eq!(d.press(t0, true, true), Action::Start);
        assert_eq!(d.tick(t0 + ms(10_000), ms(850)), Action::Idle, "no timeout");
        assert_eq!(d.press(t0 + ms(10_030), true, true), Action::Consume);
        assert_eq!(d.key_up(t0 + ms(10_100)), Action::Release);
        assert_eq!(d.key_up(t0 + ms(10_200)), Action::Idle);
        assert_eq!(d.press(t0, false, true), Action::PassThrough);
        // A tap on a full wire: discarded, not "too short".
        assert_eq!(d.press(t0, true, true), Action::Start);
        assert_eq!(d.key_up(t0 + ms(120)), Action::Discard);
    }

    /// A focus loss before the first repeat ends a pending press as a tap.
    #[test]
    fn focus_loss_while_pending_discards() {
        let mut d = HoldDetector::default();
        let t0 = Instant::now();
        d.press(t0, true, false);
        assert_eq!(d.key_up(t0 + ms(100)), Action::Discard);
        d.reset();
        assert!(!d.active());
    }

    /// The release window follows the observed repeat rate; the tap
    /// window follows the OS repeat delay.
    #[test]
    fn windows_follow_the_os_settings() {
        assert_eq!(release_after(ms(30)), ms(300));
        assert_eq!(release_after(ms(180)), ms(540));
        assert_eq!(parse_initial_key_repeat("25\n"), Some(ms(375)));
        assert_eq!(parse_initial_key_repeat("120"), Some(ms(1800)));
        assert_eq!(parse_initial_key_repeat(""), None);
        assert_eq!(parse_initial_key_repeat("0"), None);
    }
}
