//! The ambient pane: what the run looks like while it works, in place of
//! the transcript, until Esc brings the words back.
//!
//! **PARKED (2026-08-21): nothing can launch this.** The `/animation`
//! command and its help entry were removed — operator verdict, "for the
//! moment, they are mostly terrible and uninteresting". The code stays
//! compiled and under test because the parts under it are worth keeping
//! (the truncation-proof run feed, the honest run-state verdict, the
//! tool-family classifier, the charset gate for run-derived text, and
//! the Esc rung that exits a full-pane view before the cancel ladder).
//! The design record — what was built, why it did not clear the bar,
//! which shapes are worth reviving, and the two lines that re-open it —
//! is `docs/backlog/proposed/ambient-run-animations.md`.
//!
//! ## The rules this is built under
//!
//! An ambient visual in a coding tool earns its place or gets switched
//! off in a week, so every variant here answers to the same contract:
//!
//! - **Opt-in, always.** Nothing enters this pane by itself — today
//!   nothing enters it at all. Esc and a click leave, and no preference
//!   persists unless the user sets one.
//! - **Never fabricate.** Every moving thing is a function of a signal
//!   the run actually produced. A stalled run must LOOK stalled — the
//!   shared [`State`] is computed once, here, and every variant is
//!   obliged to render it. A visual that keeps dancing while the gateway
//!   is down is worse than no visual.
//! - **No text the run wrote.** Labels come from tool NAMES and file
//!   BASENAMES only, sanitized to a safe charset and capped — never from
//!   tool results, error bodies or model output. A rarity-weighted word
//!   picked out of a repository is a data-exfiltration surface pointed at
//!   a screen-share.
//! - **Truncation-proof.** The fold drops items past
//!   `transcript::MAX_ITEMS`; an animation that re-derives its history
//!   from `fold.items` would silently lose the start of a long run. The
//!   [`Feed`] here is append-fed from tool KEYS and monotonic counters,
//!   so it remembers what the transcript has already forgotten.
//! - **Idle costs nothing.** The ticker is armed only while the pane is
//!   showing, and its cadence follows the run: fast while something is in
//!   flight, slow when nothing is.
//!
//! ## The variants
//!
//! `/animation 1` [`pulse`] · the run as a live strip chart — cycles,
//! tool outcomes and context in three lanes, the newest second at the
//! right edge. The one that is still useful in a year.
//!
//! `/animation 2` [`desk`] · the intern: a character piece where every
//! prop is a real number (the paper is your context window, the bin is
//! your failures, the typing speed is your tokens per second).
//!
//! `/animation 3` [`drift`] · the work's own vocabulary as a field of
//! terms, sized by how hot they are and colored by where they came
//! from — your brief in the house red, the agent's own trail in blue.

pub mod desk;
pub mod drift;
pub mod pulse;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use abstracttui::base::{Point, Rect, Rgba};
use abstracttui::prelude::*;
use abstracttui::text;
use abstracttui::ui::StyledCanvas;

use crate::store::{Conn, Phase, Store};
use crate::transcript::{Item, ToolStatus};

/// The variants, in `/animation N` order. Index 0 is unused: 0 means OFF.
pub const VARIANTS: [&str; 3] = ["pulse", "desk", "drift"];

/// The variant `/animation` turns on when no number is given.
pub const DEFAULT_VARIANT: u8 = 1;

/// Longest label the pane will ever render from run-derived text.
const LABEL_MAX: usize = 20;

/// Events kept per run. At one event per tool call plus one per model
/// call, this covers a very long run; beyond it the oldest drop (the
/// chart compresses, it does not lie).
const EVENT_CAP: usize = 4096;

// ---------------------------------------------------------------------------
// What the run is doing, honestly
// ---------------------------------------------------------------------------

/// The shared verdict every variant must render. Computed once per frame
/// from the same signals the activity strip uses, so the pane can never
/// disagree with the chrome above it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// No run. The pane is a still life.
    Idle,
    /// Work is landing: events are arriving.
    Working,
    /// A run is live but nothing has arrived for a while — a long model
    /// call, or something wrong. The pane says so and shows the wait.
    Quiet,
    /// Blocked on a human: approval or an ask.
    Waiting,
    /// The recent tool history is mostly failures.
    Failing,
    /// The gateway is not answering.
    Down,
}

impl State {
    /// The word the pane prints. Deliberately flat: the visual carries
    /// the feeling, the label carries the fact.
    pub fn label(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Working => "working",
            State::Quiet => "waiting on the model",
            State::Waiting => "waiting on you",
            State::Failing => "tools failing",
            State::Down => "gateway not answering",
        }
    }

    /// The ink for this state, from theme tokens only.
    pub fn ink(self, t: &TokenSet) -> Rgba {
        match self {
            State::Idle => t.text_faint,
            State::Working => t.ok,
            State::Quiet => t.text_muted,
            State::Waiting => t.warn,
            State::Failing | State::Down => t.error,
        }
    }

    /// Motion multiplier: how alive the scene is allowed to look. The
    /// honesty rule in one number — nothing moves when nothing moves.
    pub fn motion(self) -> f32 {
        match self {
            State::Working => 1.0,
            State::Failing => 0.7,
            State::Waiting => 0.25,
            State::Quiet => 0.08,
            State::Idle | State::Down => 0.0,
        }
    }
}

/// Everything a variant needs about RIGHT NOW (the feed carries history).
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub state: State,
    pub phase: Phase,
    /// Seconds the current run has been going.
    pub elapsed_secs: u64,
    /// Milliseconds since the last event the feed recorded.
    pub since_event_ms: u64,
    /// Output tokens per second of the last model call, when known.
    pub tok_per_s: Option<f64>,
    /// Context used / declared window, when the operator declared one.
    pub ctx_frac: Option<f32>,
    pub llm_calls: u64,
    pub tool_calls: u64,
    pub tool_failures: u64,
    /// The app's own activity line (already app-authored — safe text).
    pub activity: String,
}

impl Snapshot {
    /// `since_event_ms` rendered the way a person reads a wait.
    pub fn since_label(&self) -> String {
        let s = self.since_event_ms / 1000;
        if s < 60 {
            format!("{s}s")
        } else {
            format!("{}m{:02}s", s / 60, s % 60)
        }
    }
}

// ---------------------------------------------------------------------------
// The feed: append-only history, immune to transcript truncation
// ---------------------------------------------------------------------------

/// What a tool call is, coarsely. Five families the eye can learn in a
/// day, derived from the tool NAME only — no arguments, no results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Read,
    Write,
    Exec,
    Search,
    Net,
    Other,
    /// A model call, not a tool.
    Think,
}

impl Family {
    /// The one classifier. Every variant colors and places by this, so
    /// they agree about what a tool IS.
    pub fn of(tool: &str) -> Family {
        let n = tool.to_ascii_lowercase();
        let has = |k: &str| n.contains(k);
        if has("write") || has("edit") || has("patch") || has("apply") || has("create") {
            Family::Write
        } else if has("read") || has("open") || has("cat") || has("view") || has("list") {
            Family::Read
        } else if has("exec") || has("bash") || has("shell") || has("command") || has("run") {
            Family::Exec
        } else if has("search") || has("grep") || has("find") || has("glob") {
            Family::Search
        } else if has("fetch") || has("http") || has("url") || has("web") || has("browse") {
            Family::Net
        } else {
            Family::Other
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Family::Read => "read",
            Family::Write => "edit",
            Family::Exec => "exec",
            Family::Search => "find",
            Family::Net => "net",
            Family::Other => "tool",
            Family::Think => "think",
        }
    }

    /// Lane index for the chart variants (Think rides its own lane).
    pub fn lane(self) -> i32 {
        match self {
            Family::Read => 0,
            Family::Search => 1,
            Family::Write => 2,
            Family::Exec => 3,
            Family::Net => 4,
            Family::Other => 5,
            Family::Think => 6,
        }
    }

    /// Ink from the theme's categorical chart ramp — hue-separated and
    /// legible on every theme by construction.
    pub fn ink(self, t: &TokenSet) -> Rgba {
        match self {
            Family::Read => t.chart[1],
            Family::Search => t.chart[6],
            Family::Write => t.chart[0],
            Family::Exec => t.chart[7],
            Family::Net => t.chart[5],
            Family::Other => t.chart[3],
            Family::Think => t.chart[2],
        }
    }
}

/// How an event ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Running,
    Ok,
    Failed,
    Denied,
}

impl Outcome {
    fn of(status: ToolStatus) -> Outcome {
        match status {
            ToolStatus::Ok => Outcome::Ok,
            ToolStatus::Failed => Outcome::Failed,
            ToolStatus::Denied => Outcome::Denied,
            _ => Outcome::Running,
        }
    }
}

/// One thing that happened, with when it happened and how it ended.
#[derive(Clone, Debug)]
pub struct Ev {
    /// Milliseconds since the feed started.
    pub at_ms: u64,
    pub family: Family,
    pub outcome: Outcome,
    /// Model calls carry their output tokens; tools carry 0.
    pub tokens: u64,
    /// A SAFE label: a tool name or a file basename, sanitized and
    /// capped. Never model output, never a result body.
    pub label: String,
}

/// The run's history, accumulated as it happens.
#[derive(Debug)]
pub struct Feed {
    started: Instant,
    pub events: std::collections::VecDeque<Ev>,
    /// Tool keys already recorded, with the index of their event, so a
    /// call that RESOLVES updates its own tick instead of adding one.
    seen: HashMap<String, u64>,
    /// Monotonic counters last observed (the truncation-proof half).
    last_llm_calls: u64,
    /// Terms and their heat, for the `drift` variant.
    pub terms: Vec<Term>,
    /// The run this feed belongs to; a new run starts a new history.
    run_id: String,
}

/// One term in the drift field: a word the work is about.
#[derive(Clone, Debug)]
pub struct Term {
    pub word: String,
    /// Decayed weight — how hot this term is right now.
    pub heat: f32,
    /// 0 = the user's own brief, 1 = the agent's own trail. The axis
    /// that answers "is it still working on what I asked?".
    pub origin: f32,
    /// Stable position on the unit disc, hashed from the word.
    pub pos: (f32, f32),
}

impl Default for Feed {
    fn default() -> Self {
        Feed::new()
    }
}

impl Feed {
    pub fn new() -> Feed {
        Feed {
            started: Instant::now(),
            events: std::collections::VecDeque::new(),
            seen: HashMap::new(),
            last_llm_calls: 0,
            terms: Vec::new(),
            run_id: String::new(),
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// The time axis the charts draw: the whole run, always — the later
    /// of "how long this feed has been alive" and "when the newest event
    /// landed" (a restored session's history arrives with timestamps
    /// older than the feed itself), never less than 30 s so a young run
    /// grows left-to-right instead of re-scaling every second.
    pub fn span_ms(&self) -> u64 {
        let last = self.events.back().map(|e| e.at_ms).unwrap_or(0);
        self.elapsed_ms().max(last).max(30_000)
    }

    /// Milliseconds since the newest event (or since the feed started).
    pub fn since_last_ms(&self) -> u64 {
        let last = self.events.back().map(|e| e.at_ms).unwrap_or(0);
        self.elapsed_ms().saturating_sub(last)
    }

    fn push(&mut self, ev: Ev) {
        if self.events.len() >= EVENT_CAP {
            self.events.pop_front();
        }
        self.events.push_back(ev);
    }

    /// Fold this run's current state into the history. Called from an
    /// effect on every fold change; cheap and idempotent — the tool KEY
    /// is the identity, so re-observing the same batch adds nothing.
    pub fn absorb(&mut self, store: Store) {
        let run = store.run_id.get_untracked();
        if !run.is_empty() && run != self.run_id {
            // A new run is a new history: the chart must never splice two
            // runs into one timeline.
            *self = Feed::new();
            self.run_id = run;
        }
        let now = self.elapsed_ms();
        store.fold.with_untracked(|f| {
            // Model calls: counted, not keyed (they carry no id).
            if f.stats.llm_calls > self.last_llm_calls {
                let new = f.stats.llm_calls - self.last_llm_calls;
                let tokens = f.stats.output_tokens;
                self.last_llm_calls = f.stats.llm_calls;
                for _ in 0..new.min(8) {
                    self.push(Ev {
                        at_ms: now,
                        family: Family::Think,
                        outcome: Outcome::Ok,
                        tokens,
                        label: String::new(),
                    });
                }
            }
            for item in f.items.iter() {
                let Item::Tool {
                    key,
                    name,
                    args_preview,
                    status,
                    ..
                } = item
                else {
                    continue;
                };
                let outcome = Outcome::of(*status);
                match self.seen.get(key).copied() {
                    Some(at) => {
                        // Resolve in place: the tick that was running now
                        // carries its outcome.
                        if let Some(ev) = self.events.iter_mut().find(|e| e.at_ms == at) {
                            if ev.outcome == Outcome::Running {
                                ev.outcome = outcome;
                            }
                        }
                    }
                    None => {
                        self.seen.insert(key.clone(), now);
                        let family = Family::of(name);
                        let label = safe_label(args_preview).unwrap_or_else(|| safe_word(name));
                        drift::absorb_term(&mut self.terms, &label, 1.0, 1.0);
                        drift::absorb_term(&mut self.terms, &safe_word(name), 0.6, 1.0);
                        self.push(Ev {
                            at_ms: now,
                            family,
                            outcome,
                            tokens: 0,
                            label,
                        });
                    }
                }
            }
            // The brief: the user's own words anchor the drift field and
            // are the only free text on this pane the RUN did not write.
            for item in f.items.iter().take(4) {
                if let Item::User { text } = item {
                    for word in text.split_whitespace().take(40) {
                        if let Some(w) = safe_label(word) {
                            drift::absorb_term(&mut self.terms, &w, 2.5, 0.0);
                        }
                    }
                }
            }
        });
        drift::decay(&mut self.terms, 0.985);
    }

    /// The run's state, honestly. One place, so every variant agrees.
    pub fn snapshot(&self, store: Store) -> Snapshot {
        let phase = store.phase.get();
        let conn = store.conn.get();
        let since_event_ms = self.since_last_ms();
        let (llm_calls, tool_calls, tool_failures, ctx_frac, activity, waiting) =
            store.fold.with(|f| {
                let window = store.context_window.get_untracked();
                let frac = if window > 0 {
                    Some((f.stats.last_input_tokens as f32 / window as f32).clamp(0.0, 1.5))
                } else {
                    None
                };
                (
                    f.stats.llm_calls,
                    f.stats.tool_calls,
                    f.stats.tool_failures,
                    frac,
                    f.activity.clone(),
                    f.pending_wait.is_some(),
                )
            });
        // Recent failure pressure: the last five tool events, not the
        // lifetime count — "it failed twice an hour ago" is not failing.
        let recent_failed = self
            .events
            .iter()
            .rev()
            .filter(|e| e.family != Family::Think)
            .take(5)
            .filter(|e| e.outcome == Outcome::Failed)
            .count();
        let state = if matches!(conn, Conn::Down(..)) {
            State::Down
        } else if waiting {
            State::Waiting
        } else if phase == Phase::Idle {
            State::Idle
        } else if recent_failed >= 3 {
            State::Failing
        } else if since_event_ms > 25_000 {
            State::Quiet
        } else {
            State::Working
        };
        Snapshot {
            state,
            phase,
            elapsed_secs: store.elapsed_secs.get(),
            since_event_ms,
            tok_per_s: store.last_call_rate.get(),
            ctx_frac,
            llm_calls,
            tool_calls,
            tool_failures,
            activity,
        }
    }
}

/// A shared feed plus the version counter that wakes the pane. The feed
/// itself is not a signal: it is appended to, and cloning a run's whole
/// history once per frame would be the most expensive thing on screen.
#[derive(Clone)]
pub struct FeedHandle {
    pub feed: Rc<RefCell<Feed>>,
    pub version: Signal<u64>,
}

/// Sanitize run-derived text down to something safe to PRINT: a file
/// basename or a bare word, ASCII-ish, bounded. Control characters,
/// escape sequences and anything exotic never reach the terminal — a
/// tool result carrying `ESC[2J` must not be able to clear the screen,
/// and a 40 MB blob must not be able to slow a frame.
pub fn safe_label(raw: &str) -> Option<String> {
    let head: String = raw.chars().take(240).collect();
    // Prefer the last path segment: `src/ui/mod.rs` reads as `mod.rs`.
    let token = head
        .split_whitespace()
        .find(|w| w.contains('/') || w.contains('.'))
        .unwrap_or_else(|| head.split_whitespace().next().unwrap_or(""));
    let base = token.rsplit('/').next().unwrap_or(token);
    let word = safe_word(base);
    (word.len() >= 2).then_some(word)
}

/// The charset gate: letters, digits, and the three punctuation marks a
/// filename needs. Everything else becomes nothing.
pub fn safe_word(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .take(LABEL_MAX)
        .collect()
}

// ---------------------------------------------------------------------------
// The pane
// ---------------------------------------------------------------------------

/// The animation pane: one full-pane draw plus a click that returns to
/// the transcript. Reads the version signal (so appends repaint) and the
/// frame signal (so motion advances).
pub fn pane(_cx: Scope, store: Store, handle: FeedHandle, frame: Signal<u64>) -> View {
    let feed = handle.feed.clone();
    let version = handle.version;
    Element::new()
        .style(LayoutStyle::default().grow(1.0))
        .role(abstracttui::ui::Role::Button)
        .access_label("run animation — Esc returns to the transcript")
        .on(abstracttui::ui::Phase::Bubble, move |ectx, ev| {
            let abstracttui::ui::UiEvent::Mouse(m) = ev else {
                return;
            };
            if let abstracttui::ui::MouseKind::Down(abstracttui::ui::MouseButton::Left) = m.kind {
                ectx.stop_propagation();
                // A click is an exit, not a trick: the fastest way back
                // to the words is always one gesture away.
                store.animation.set(0);
            }
        })
        .child(dyn_view(
            LayoutStyle::default()
                .grow(1.0)
                .width(Dimension::Percent(1.0)),
            move || {
                let t = abstracttui::app::current_theme().tokens;
                let variant = store.animation.get().max(1);
                let f = frame.get();
                let _ = version.get(); // repaint when the feed grows
                let feed = feed.clone();
                let snap = feed.borrow().snapshot(store);
                Element::new()
                    .style(LayoutStyle::default().grow(1.0))
                    .draw(move |canvas, rect| {
                        let feed = feed.borrow();
                        canvas.fill(rect, ' ', t.bg, t.bg);
                        if rect.h < 3 || rect.w < 20 {
                            return;
                        }
                        let stage = Rect::new(rect.x, rect.y, rect.w, rect.h - 2);
                        match variant {
                            2 => desk::render(canvas, stage, &t, &feed, &snap, f),
                            3 => drift::render(canvas, stage, &t, &feed, &snap, f),
                            _ => pulse::render(canvas, stage, &t, &feed, &snap, f),
                        }
                        status_row(canvas, rect, &t, &snap, variant);
                    })
                    .build()
            },
        ))
        .build()
}

/// The two chrome rows every variant carries: the honest state line, and
/// the way out. The animation is never the only carrier of a fact — this
/// row is the fact.
fn status_row(canvas: &mut dyn StyledCanvas, rect: Rect, t: &TokenSet, snap: &Snapshot, v: u8) {
    let y = rect.y + rect.h - 2;
    let mut left = format!("{} · {}", snap.state.label(), snap.since_label());
    if snap.state == State::Quiet || snap.state == State::Working {
        if let Some(rate) = snap.tok_per_s {
            left.push_str(&format!(" · {rate:.0} tok/s"));
        }
    }
    if snap.tool_failures > 0 {
        left.push_str(&format!(" · {} failed", snap.tool_failures));
    }
    let fitted = text::truncate_ellipsis(&left, (rect.w - 2).max(4));
    canvas.print(
        Point::new(rect.x + 1, y),
        &fitted,
        snap.state.ink(t),
        Rgba::TRANSPARENT,
    );
    let name = VARIANTS
        .get(v.saturating_sub(1) as usize)
        .unwrap_or(&"pulse");
    let hint = format!("/animation {v} · {name} — Esc or click returns to the transcript");
    let hint = text::truncate_ellipsis(&hint, (rect.w - 2).max(4));
    canvas.print(
        Point::new(rect.x + 1, rect.y + rect.h - 1),
        &hint,
        t.text_faint,
        Rgba::TRANSPARENT,
    );
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

/// Create the feed and keep it fed. The absorb effect tracks the fold,
/// which is where every event this pane draws comes from; it runs
/// whether or not the animation is on, so opening `/animation` mid-run
/// shows the run's whole history rather than starting from blank.
pub fn wire_feed(cx: Scope, store: Store) -> FeedHandle {
    let handle = FeedHandle {
        feed: Rc::new(RefCell::new(Feed::new())),
        version: cx.signal(0u64),
    };
    let feed = handle.feed.clone();
    let version = handle.version;
    cx.effect(move || {
        // Tracked: any fold change (an item, a stat) re-runs this.
        let _ = store.fold.with(|f| f.items.len());
        feed.borrow_mut().absorb(store);
        version.update(|v| *v = v.wrapping_add(1));
    });
    handle
}

/// The animation ticker: armed ONLY while the pane is showing, at a
/// cadence that follows the run — the engine's zero-wakeup idle
/// guarantee holds everywhere else, and a paused or finished run costs
/// one slow tick instead of thirty fast ones.
pub fn wire_ticker(cx: Scope, store: Store, frame: Signal<u64>) {
    let handle: Rc<RefCell<Option<abstracttui::reactive::IntervalHandle>>> =
        Rc::new(RefCell::new(None));
    let armed: Rc<std::cell::Cell<u64>> = Rc::new(std::cell::Cell::new(0));
    cx.effect(move || {
        let on = store.animation.get() > 0;
        let running = store.phase.get() != Phase::Idle;
        let period_ms: u64 = if !on {
            0
        } else if running {
            120
        } else {
            600
        };
        let mut slot = handle.borrow_mut();
        if period_ms == 0 {
            if let Some(h) = slot.take() {
                h.cancel();
            }
            armed.set(0);
            return;
        }
        if slot.is_some() && armed.get() == period_ms {
            return;
        }
        if let Some(h) = slot.take() {
            h.cancel();
        } else {
            frame.set(0); // a fresh entrance always starts at frame 0
        }
        armed.set(period_ms);
        *slot = Some(abstracttui::reactive::interval(
            cx,
            Duration::from_millis(period_ms),
            move || frame.update(|f| *f = f.wrapping_add(1)),
        ));
    });
}

/// `/animation [N|off|on]`. Returns the line to echo.
pub fn command(store: Store, arg: Option<&str>) -> String {
    let current = store.animation.get_untracked();
    let arg = arg.map(str::trim).unwrap_or("");
    let pick = match arg.to_ascii_lowercase().as_str() {
        "" => {
            // Bare toggles: off if showing, else the last/default pick.
            if current > 0 {
                0
            } else {
                DEFAULT_VARIANT
            }
        }
        "off" | "none" | "stop" | "0" => 0,
        "on" => DEFAULT_VARIANT,
        other => match other.parse::<u8>() {
            Ok(n) if (1..=VARIANTS.len() as u8).contains(&n) => n,
            _ => match VARIANTS.iter().position(|v| *v == other) {
                Some(i) => i as u8 + 1,
                None => {
                    return format!(
                        "/animation: unknown variant {other:?} — try 1..{} ({})",
                        VARIANTS.len(),
                        VARIANTS.join(", ")
                    )
                }
            },
        },
    };
    store.animation.set(pick);
    if pick == 0 {
        "animation off — the transcript is back".to_string()
    } else {
        let name = VARIANTS[pick as usize - 1];
        format!("animation {pick} · {name} — Esc or click returns to the transcript")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The label vocabulary is the safety boundary: what reaches the
    /// terminal from run-derived text is a bounded, charset-gated word.
    /// A tool result carrying escape sequences must not be able to clear
    /// the screen, set the title, or write the clipboard.
    #[test]
    fn labels_cannot_carry_control_sequences() {
        let hostile = "\u{1b}[2J\u{1b}]52;c;cGFyYQ==\u{7}/etc/passwd";
        let out = safe_word(hostile);
        assert!(!out.contains('\u{1b}'), "no escape byte survives: {out:?}");
        assert!(!out.contains('\u{7}'), "no BEL survives: {out:?}");
        assert!(out.chars().count() <= LABEL_MAX);
        // A 40 MB blob costs a bounded scan, not a frame.
        let huge = "x".repeat(40 * 1024 * 1024);
        assert!(safe_label(&huge).unwrap().chars().count() <= LABEL_MAX);
        // Right-to-left overrides and zero-width joiners never render.
        assert_eq!(safe_word("a\u{202e}b\u{200d}c"), "abc");
        // A real argument line reduces to the file it names.
        assert_eq!(
            safe_label("src/ui/mosaic.rs start_line=1 end_line=90").as_deref(),
            Some("mosaic.rs")
        );
    }

    /// `/animation` picks, toggles, and REFUSES junk by name rather than
    /// silently landing on a variant the user did not ask for.
    #[test]
    fn the_command_toggles_picks_and_refuses() {
        abstracttui::reactive::create_root(|cx| {
            let store = Store::create(cx);
            assert_eq!(store.animation.get_untracked(), 0, "opt-in: off at boot");
            command(store, None);
            assert_eq!(store.animation.get_untracked(), DEFAULT_VARIANT);
            command(store, None);
            assert_eq!(store.animation.get_untracked(), 0, "bare toggles back off");
            command(store, Some("3"));
            assert_eq!(store.animation.get_untracked(), 3);
            command(store, Some("desk"));
            assert_eq!(store.animation.get_untracked(), 2, "names work too");
            let msg = command(store, Some("9"));
            assert!(msg.contains("unknown variant"), "{msg}");
            assert_eq!(
                store.animation.get_untracked(),
                2,
                "a refusal changes nothing"
            );
            command(store, Some("off"));
            assert_eq!(store.animation.get_untracked(), 0);
        });
    }

    /// The tool-family classifier is shared vocabulary: every variant
    /// colors and places by it, so it is pinned in one place.
    #[test]
    fn families_classify_the_tools_this_client_actually_sees() {
        for (tool, want) in [
            ("read_file", Family::Read),
            ("list_files", Family::Read),
            ("write_file", Family::Write),
            ("edit_file", Family::Write),
            ("execute_command", Family::Exec),
            ("search_files", Family::Search),
            ("web_search", Family::Search),
            ("fetch_url", Family::Net),
            ("summon_entity", Family::Other),
        ] {
            assert_eq!(Family::of(tool), want, "{tool}");
        }
    }
}
