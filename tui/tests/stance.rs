//! `/stance` — the conduct read over a real fold.
//!
//! The core's own arithmetic is unit-tested inside
//! `ui::stance::conduct`; what needs an integration test is the seam
//! this client owns: turning a transcript into per-turn FACTS, and
//! building the session baseline from the turns before the current one.
//!
//! Deliberately its own file (no shared harness): the feature is meant
//! to be removable in one directory plus three lines.

use abstractcode::store::Store;
use abstractcode::transcript::{CallCost, Item, ToolStatus};
use abstractcode::ui::stance::{self, conduct};
use abstracttui::base::{Rect, Size};
use abstracttui::render::{Cell, Surface};

fn thinking(gen_ms: f64, out: u64) -> Item {
    Item::Thinking {
        iteration: 1,
        content: String::new(),
        reasoning: String::new(),
        call: CallCost {
            gen_time_ms: Some(gen_ms),
            input_tokens: 1000,
            output_tokens: out,
            cached_tokens: 0,
        },
    }
}

fn tool(key: &str, name: &str, status: ToolStatus) -> Item {
    Item::Tool {
        key: key.into(),
        name: name.into(),
        args_preview: String::new(),
        args_full: String::new(),
        status,
        result: String::new(),
        error: String::new(),
    }
}

/// A turn is the stretch between user messages, and `tool_rounds` counts
/// CONTIGUOUS blocks of calls — one model cycle that called tools is one
/// round, however many calls it made.
#[test]
fn turns_split_at_user_messages_and_count_rounds_not_calls() {
    abstracttui::reactive::create_root(|cx| {
        let store = Store::create(cx);
        store.fold.update(|f| {
            f.push_item(Item::User { text: "one".into() });
            f.push_item(thinking(1000.0, 100));
            f.push_item(tool("a", "read_file", ToolStatus::Ok));
            f.push_item(tool("b", "read_file", ToolStatus::Ok));
            // Same round: two calls, no cycle between them.
            f.push_item(thinking(2000.0, 200));
            f.push_item(tool("c", "edit_file", ToolStatus::Failed));
            // Second round.
            f.push_item(Item::User { text: "two".into() });
            f.push_item(thinking(500.0, 50));
        });
        let turns = stance::turns(store);
        assert_eq!(turns.len(), 2, "one turn per user message");
        let first = &turns[0];
        assert_eq!(first.facts.tool_rounds, Some(2), "blocks, not calls");
        assert_eq!(first.tools.len(), 3);
        assert_eq!(first.facts.think_ms, Some(3000.0), "cycles sum");
        assert_eq!(first.facts.tokens_out, Some(300));
        assert_eq!(first.tools[2].ok, Some(false));
        let second = &turns[1];
        assert_eq!(second.facts.tool_rounds, None, "no calls, no rounds fact");
        assert!(second.tools.is_empty());
    });
}

/// The baseline is the session's OWN history — the turns before this
/// one. With no history there is no baseline, and the reads say so
/// instead of inventing a scale.
#[test]
fn the_baseline_comes_from_prior_turns_only() {
    abstracttui::reactive::create_root(|cx| {
        let store = Store::create(cx);
        store.fold.update(|f| {
            f.push_item(Item::User {
                text: "first".into(),
            });
            f.push_item(thinking(4000.0, 400));
        });
        let (axes, _) = stance::read(store);
        let eff = &axes[0];
        assert_eq!(eff.value, None, "no prior turns = no baseline");
        assert!(eff
            .reason
            .as_deref()
            .unwrap()
            .contains("no session baseline"));
        assert_eq!(eff.text, "4.0s · 400tk", "the fact is still printed");

        // Three more turns at the same size; the fourth now has a median.
        store.fold.update(|f| {
            for i in 0..3 {
                f.push_item(Item::User {
                    text: format!("turn {i}"),
                });
                f.push_item(thinking(4000.0, 400));
            }
            f.push_item(Item::User { text: "now".into() });
            f.push_item(thinking(4000.0, 400));
        });
        let (axes, _) = stance::read(store);
        let eff = &axes[0];
        assert!(
            (eff.value.unwrap() - 0.5).abs() < 1e-6,
            "a turn at the session median sits at half, got {:?}",
            eff.value
        );
    });
}

/// This client has no memory-recall fact on the agent lane, so ATT must
/// read ABSENT with its reason — never a fabricated zero.
#[test]
fn attention_is_absent_on_the_agent_lane_and_says_why() {
    abstracttui::reactive::create_root(|cx| {
        let store = Store::create(cx);
        store.fold.update(|f| {
            f.push_item(Item::User { text: "go".into() });
            f.push_item(thinking(1000.0, 100));
            f.push_item(tool("a", "read_file", ToolStatus::Ok));
        });
        let (axes, _) = stance::read(store);
        let att = axes
            .iter()
            .find(|a| a.id == conduct::AxisId::Attention)
            .unwrap();
        assert_eq!(att.value, None);
        assert_eq!(att.short, "—");
        assert_eq!(att.reason.as_deref(), Some("no recall fact this turn"));
    });
}

/// Both renderers survive hostile geometry (the block sits in a live
/// layout, so it meets every terminal size) and the line always prints
/// all four codes when it has the room.
#[test]
fn the_views_render_at_every_size() {
    let facts = conduct::Facts {
        think_ms: Some(4200.0),
        tokens_out: Some(380),
        tool_rounds: Some(2),
        ..conduct::Facts::default()
    };
    let turn = stance::Turn {
        facts,
        tools: vec![
            conduct::ToolCall {
                name: "read_file".into(),
                ok: Some(true),
            },
            conduct::ToolCall {
                name: "edit_file".into(),
                ok: Some(false),
            },
        ],
    };
    let axes = conduct::axes(&turn.facts, &turn.tools, &conduct::Baseline::default());
    let theme = abstracttui::theme::default_theme();
    for size in [
        Size::new(0, 0),
        Size::new(1, 1),
        Size::new(20, 1),
        Size::new(40, 3),
        Size::new(96, 14),
        Size::new(200, 40),
    ] {
        let mut s = Surface::new(size, Cell::EMPTY);
        let rect = Rect::new(0, 0, size.w, size.h);
        stance::draw_line(&mut s, rect, &theme.tokens, &axes);
        stance::draw_figure(&mut s, rect, &theme.tokens, &axes, &turn, 3, true);
        assert_eq!(s.size(), size);
    }
    // At a normal width the line carries every read.
    let size = Size::new(96, 1);
    let mut s = Surface::new(size, Cell::EMPTY);
    stance::draw_line(&mut s, Rect::new(0, 0, 96, 1), &theme.tokens, &axes);
    let row: String = (0..96)
        .map(|x| {
            s.get(x, 0)
                .map(|c| s.glyph_str(c))
                .filter(|t| !t.is_empty())
                .and_then(|t| t.chars().next())
                .unwrap_or(' ')
        })
        .collect();
    for code in ["EFF", "ACT", "ATT", "RIG"] {
        assert!(row.contains(code), "{code} missing from {row:?}");
    }
    assert!(
        row.contains("1✕"),
        "a failed call shows in the line: {row:?}"
    );
}

/// A finished run does not breathe: with `moving = false` the figure is
/// identical across frames. The honesty gate, pinned.
#[test]
fn a_still_run_draws_a_still_figure() {
    let turn = stance::Turn {
        facts: conduct::Facts {
            think_ms: Some(9000.0),
            tokens_out: Some(700),
            tool_rounds: Some(1),
            ..conduct::Facts::default()
        },
        tools: vec![conduct::ToolCall {
            name: "read_file".into(),
            ok: Some(true),
        }],
    };
    let base = conduct::Baseline {
        think_ms: Some(4000.0),
        tokens_out: Some(400.0),
        tool_rounds: Some(1.0),
        ..conduct::Baseline::default()
    };
    let axes = conduct::axes(&turn.facts, &turn.tools, &base);
    let theme = abstracttui::theme::default_theme();
    let render = |frame: u64, moving: bool| -> String {
        let size = Size::new(80, 14);
        let mut s = Surface::new(size, Cell::EMPTY);
        stance::draw_figure(
            &mut s,
            Rect::new(0, 0, size.w, size.h),
            &theme.tokens,
            &axes,
            &turn,
            frame,
            moving,
        );
        (0..size.h)
            .map(|y| {
                (0..size.w)
                    .map(|x| {
                        s.get(x, y)
                            .map(|c| s.glyph_str(c))
                            .filter(|t| !t.is_empty())
                            .and_then(|t| t.chars().next())
                            .unwrap_or(' ')
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        render(0, false),
        render(11, false),
        "a run that is not producing must not move"
    );
    assert_ne!(
        render(0, true),
        render(6, true),
        "a live run breathes (or the breath is dead code)"
    );
}

/// The agent lane cannot produce a recall fact at all — memory belongs
/// to the entity visit endpoint — so the attention read is DROPPED
/// there, not dashed forever. A permanent blank is not an honest absent
/// reading; it is clutter that teaches the reader the instrument is
/// broken.
#[test]
fn the_agent_lane_drops_the_read_it_can_never_make() {
    abstracttui::reactive::create_root(|cx| {
        let store = Store::create(cx);
        store.fold.update(|f| {
            f.push_item(Item::User { text: "go".into() });
            f.push_item(thinking(1000.0, 100));
            f.push_item(tool("a", "read_file", ToolStatus::Ok));
        });
        // The core still reports all four — the kit's contract is intact.
        let (all, _) = stance::read(store);
        assert_eq!(all.len(), 4);
        assert!(all.iter().any(|a| a.id == conduct::AxisId::Attention));

        // The agent lane shows three.
        assert!(!stance::lane_has_recall(store));
        let shown = stance::visible(store, all.clone());
        assert_eq!(shown.len(), 3, "no attention read on the agent lane");
        assert!(!shown.iter().any(|a| a.id == conduct::AxisId::Attention));

        // An entity visit has the fact, so it keeps all four.
        store
            .focus
            .set(abstractcode::convo::Focus::Entity("castor".into()));
        assert!(stance::lane_has_recall(store));
        assert_eq!(stance::visible(store, all).len(), 4);
    });
}

/// The panel is its own click target: a press inside it toggles, a press
/// anywhere else must pass through to the transcript untouched. (The
/// layer routes no input by design — the root tree asks `hit` at capture
/// phase — so this predicate IS the affordance.)
#[test]
fn the_panel_is_clickable_and_nothing_else_is() {
    use abstracttui::base::Point;
    let view = Size::new(110, 30);
    for mode in [stance::LINE, stance::FIGURE] {
        let b = stance::panel_bounds(mode, view);
        // Every corner of the panel is a hit.
        for (x, y) in [
            (b.x, b.y),
            (b.x + b.w - 1, b.y),
            (b.x, b.y + b.h - 1),
            (b.x + b.w - 1, b.y + b.h - 1),
        ] {
            assert!(
                stance::hit(mode, view, Point::new(x, y)),
                "{mode}: ({x},{y})"
            );
        }
        // One cell outside, on every side, is not.
        for (x, y) in [
            (b.x - 1, b.y),
            (b.x + b.w, b.y),
            (b.x, b.y - 1),
            (b.x, b.y + b.h),
        ] {
            assert!(
                !stance::hit(mode, view, Point::new(x, y)),
                "{mode}: ({x},{y}) is outside the panel"
            );
        }
        // The composer and the transcript are never the panel.
        assert!(!stance::hit(mode, view, Point::new(2, 2)));
        assert!(!stance::hit(mode, view, Point::new(2, view.h - 2)));
    }
    // With the panel off, nothing on screen is a hit.
    assert!(!stance::hit(stance::OFF, view, Point::new(90, 24)));
}
