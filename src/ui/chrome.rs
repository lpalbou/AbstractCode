//! Fixed chrome: header, activity strip, composer, status bar.

use abstracttui::prelude::*;
use abstracttui::text;
use abstracttui::widgets::{Sparkline, Spinner};

use crate::store::{Conn, Phase, Store};
use crate::ui::UiCtx;

/// Header: wordmark · workflow · provider/model · session · connection orb.
pub fn header(t: &TokenSet, store: Store) -> View {
    let tokens = *t;
    dyn_view(LayoutStyle::line(1), move || {
        let t = tokens;
        let workflow = store.workflow.with(|w| {
            if w.flow_id.is_empty() {
                "no workflow yet".to_string()
            } else {
                w.label()
            }
        });
        let provider = store.provider.get();
        let model = store.model.get();
        let session = store.session_id.get();
        let conn = store.conn.get();
        let route = match (provider.is_empty(), model.is_empty()) {
            (true, true) => {
                // Honesty upgrade: name what "gateway defaults" resolves to.
                // Best truth first: the model a run actually used; else the
                // gateway's configured text route; else the bare label.
                // One FORMAT either way — `provider · model` (the provider
                // silently vanishing after the first run read as data loss;
                // adversary P3, 2026-07-22).
                let served = store.fold.with(|f| f.stats.effective_model.clone());
                let (dp, dm) = store.default_route.get();
                if !served.is_empty() && !dp.is_empty() {
                    format!("gateway defaults ({dp} · {served})")
                } else if !served.is_empty() {
                    format!("gateway defaults ({served})")
                } else if !dm.is_empty() {
                    format!("gateway defaults ({dp} · {dm})")
                } else {
                    "gateway defaults".to_string()
                }
            }
            (false, true) => provider.clone(),
            (true, false) => model.clone(),
            (false, false) => format!("{provider} · {model}"),
        };
        Element::new()
            .style(LayoutStyle::line(1))
            .draw(move |canvas, rect| {
                canvas.fill(rect, ' ', t.text, t.surface);
                // Right side measured FIRST so the left run clips under it
                // (overprint at narrow widths was a live finding).
                // Distinct glyphs, not just color (color-blind honesty).
                let (orb, orb_ink) = match &conn {
                    Conn::Ok => ("●", t.ok),
                    Conn::Unknown => ("◌", t.text_faint),
                    Conn::Down(_) => ("✗", t.error),
                };
                // Char-safe tail truncation: session ids are user-supplied
                // (--session, /session, prefs) — a byte slice paniced on
                // multibyte ids every frame (adversary finding 3).
                let sid = {
                    let chars: Vec<char> = session.chars().collect();
                    if chars.len() > 18 {
                        let tail: String = chars[chars.len() - 15..].iter().collect();
                        format!("…{tail}")
                    } else {
                        session.clone()
                    }
                };
                let right = format!("{sid} ");
                let right_w = text::width(&right) + 2;
                let rx = (rect.right() - right_w).max(rect.x);

                let mut x = rect.x + 1;
                let clip_to = (rx - 1).max(rect.x);
                let print_clipped =
                    |canvas: &mut dyn abstracttui::ui::Canvas, x: &mut i32, s: &str, ink| {
                        let avail = (clip_to - *x).max(0);
                        if avail <= 0 {
                            return;
                        }
                        let fitted = text::truncate_ellipsis(s, avail);
                        *x += canvas.print(Point::new(*x, rect.y), &fitted, ink, t.surface);
                    };
                print_clipped(canvas, &mut x, "▲ AbstractCode", t.accent);
                print_clipped(canvas, &mut x, "  ", t.text);
                print_clipped(canvas, &mut x, &workflow, t.text);
                print_clipped(canvas, &mut x, "  ·  ", t.text_faint);
                print_clipped(canvas, &mut x, &route, t.text_muted);

                let mut x2 = rx;
                x2 += canvas.print(Point::new(x2, rect.y), &right, t.text_faint, t.surface);
                canvas.print(Point::new(x2, rect.y), orb, orb_ink, t.surface);
            })
            .build()
    })
}

/// Activity strip: spinner + status + cycle + elapsed + token sparkline.
pub fn activity_strip(t: &TokenSet, store: Store, spin: Signal<u64>) -> View {
    let tokens = *t;
    dyn_view(LayoutStyle::line(1), move || {
        let t = tokens;
        // A pending wait OWNS the strip, unconditionally: later records from
        // other subruns overwrite the activity text, and a deferred prompt
        // left NO visible trace (live finding: "awaiting approval" card with
        // no modal and no way back). This line cannot be overwritten while
        // the wait is pending.
        let waiting = store.fold.with(|f| {
            f.pending_wait.as_ref().map(|w| match &w.kind {
                crate::transcript::WaitKind::Approval { tool_calls } => {
                    format!("approval needed — {} tool call(s)", tool_calls.len())
                }
                crate::transcript::WaitKind::Ask { .. } => "the agent asked a question".into(),
            })
        });
        if let Some(what) = waiting {
            let warn = t.warn;
            let text = format!("⏸ {what} · press Enter to open the prompt");
            return Element::new()
                .style(LayoutStyle::line(1))
                .draw(move |canvas, rect| {
                    let fitted = abstracttui::text::truncate_ellipsis(&text, (rect.w - 2).max(4));
                    canvas.print(
                        Point::new(rect.x + 1, rect.y),
                        &fitted,
                        warn,
                        Rgba::TRANSPARENT,
                    );
                })
                .build();
        }
        // Durable pause owns the strip the same way (quit-safe: the run
        // stays paused on the gateway across restarts).
        if store.paused.get() {
            let warn = t.warn;
            return Element::new()
                .style(LayoutStyle::line(1))
                .draw(move |canvas, rect| {
                    let fitted = abstracttui::text::truncate_ellipsis(
                        "⏸ run paused durably on the gateway · /resume continues",
                        (rect.w - 2).max(4),
                    );
                    canvas.print(
                        Point::new(rect.x + 1, rect.y),
                        &fitted,
                        warn,
                        Rgba::TRANSPARENT,
                    );
                })
                .build();
        }
        let phase = store.phase.get();
        if phase == Phase::Idle {
            let totals = store.totals.get();
            if totals.runs == 0 {
                return Element::new().style(LayoutStyle::line(1)).build();
            }
            let last_ctx = store.fold.with(|f| f.stats.last_input_tokens);
            let ctx_part = if last_ctx > 0 {
                format!(" · ctx {} tk", fmt_tokens(last_ctx))
            } else {
                String::new()
            };
            let summary = format!(
                "session: {} runs · {} in / {} out tk{ctx_part} · Enter sends the next task",
                totals.runs,
                fmt_tokens(totals.input_tokens),
                fmt_tokens(totals.output_tokens)
            );
            return Element::new()
                .style(LayoutStyle::line(1))
                .draw(move |canvas, rect| {
                    canvas.print(
                        Point::new(rect.x + 1, rect.y),
                        &summary,
                        t.text_faint,
                        Rgba::TRANSPARENT,
                    );
                })
                .build();
        }

        let frame = spin.get();
        let (activity, cycle, stats) = store
            .fold
            .with(|f| (f.activity.clone(), f.cycle, f.stats.clone()));
        let elapsed = store.elapsed_secs.get();
        let label = {
            let base = if activity.is_empty() {
                match phase {
                    Phase::Starting => "starting run".to_string(),
                    _ => "working".to_string(),
                }
            } else {
                activity
            };
            let mut parts = vec![base];
            // Skip the cycle chip when the activity text already names it
            // ("thinking (cycle 1) · cycle 1" read twice; live review,
            // 2026-07-22).
            if cycle > 0 && !parts[0].contains(&format!("cycle {cycle}")) {
                parts.push(format!("cycle {cycle}"));
            }
            parts.push(format!("{}s", elapsed));
            parts.push(format!(
                "{}↑ {}↓ tk",
                fmt_tokens(stats.input_tokens),
                fmt_tokens(stats.output_tokens)
            ));
            if stats.last_input_tokens > 0 {
                // The context the model saw on its latest call — the live
                // "how full is the conversation" number.
                parts.push(format!("ctx {}", fmt_tokens(stats.last_input_tokens)));
            }
            if stats.cached_tokens > 0 {
                parts.push(format!("cache {}", fmt_tokens(stats.cached_tokens)));
            }
            if stats.tool_calls > 0 {
                parts.push(format!("{} tools", stats.tool_calls));
            }
            // A single model call past 60s names itself (live finding: an
            // MLX 27B at ~0.25 tok/s looked idle; the truth was slow
            // inference, and the strip should say so).
            if let Some(since) = store.fold.with(|f| f.llm_inflight_since) {
                let s = since.elapsed().as_secs();
                if s >= 60 {
                    parts.push(format!(
                        "model call {}m{:02}s — provider may be slow",
                        s / 60,
                        s % 60
                    ));
                }
            }
            parts.join("  ·  ")
        };

        let series = stats.output_series.clone();
        Element::new()
            .style(
                LayoutStyle::row()
                    .h(1)
                    .gap(1)
                    .padding(Edges::hv(1, 0))
                    // Full width: a content-hugging row starves the grow(1.0)
                    // spinner label (live finding: the strip never rendered).
                    .width(Dimension::Percent(1.0)),
            )
            .child(
                Spinner::new()
                    .frame(frame)
                    .label(label)
                    .layout(LayoutStyle::default().h(1).grow(1.0))
                    .element(&t)
                    .build(),
            )
            .child(if series.len() >= 2 {
                Sparkline::new(series)
                    .layout(LayoutStyle::default().h(1).width(Dimension::Cells(16)))
                    .element(&t)
                    .build()
            } else {
                Element::new()
                    .style(LayoutStyle::default().h(1).width(Dimension::Cells(0)))
                    .build()
            })
            .build()
    })
}

/// Composer: multiline `TextArea` (grows 1..4 rows, Enter submits,
/// Alt+Enter — and Shift+Enter on kitty terminals — inserts a newline,
/// block paste inserts whole, ↑/↓ recall submitted history at the buffer
/// edges) with a `/`-command completion dropdown anchored at the caret.
/// Rebuilt only on THEME changes; the durable `TextAreaState` lives in
/// root scope, so drafts + caret + history survive the rebuild.
///
/// `.autofocus()` re-fires on every dyn regeneration (theme switches), so
/// boot focus and post-theme-switch focus need no app bookkeeping
/// (abstracttui 0.2.0; the 0.1.0 autofocus-in-dyn panic is fixed).
pub fn composer(
    cx: Scope,
    t: &TokenSet,
    store: Store,
    state: &abstracttui::widgets::TextAreaState,
    overlays: &abstracttui::app::Overlays,
    on_submit: impl FnMut(&str) + Clone + 'static,
) -> View {
    let _ = store;
    let mut submit = on_submit;
    let submit_state = state.clone();
    let area = abstracttui::widgets::TextArea::new()
        .state(state)
        .placeholder("describe a task, steer a running one — /help · Alt+Enter newline")
        .rows(1, 4)
        .on_change(|_| {
            // A lingering drag-selection swallows `c`/Enter as copy keys
            // (engine design: the region stays visible after the release
            // copy). Typing means the user moved on — clear it so the
            // composer gets every subsequent key. HONEST LIMIT (engine
            // backlog 0290): the selection layer consumes `c`/Enter
            // BEFORE tree dispatch, so this hook only fires for OTHER
            // keys — a leading `c` or a bare Enter after a drag is still
            // eaten; only the engine can fix that half.
            let sel = abstracttui::app::selection::selection();
            if sel.is_active() {
                sel.clear();
            }
        })
        .on_submit(move |text| {
            let owned = text.to_string();
            if !owned.trim().is_empty() {
                submit_state.push_history(owned.trim());
            }
            submit_state.clear();
            submit(&owned);
        })
        .element(cx, t)
        .autofocus()
        .build();
    // `/` command completion at the caret (engine anchored panel: never
    // takes focus, Esc dismisses, Enter/Tab accept, typing refilters).
    // Two provider rules keep Enter predictable (the dropdown intercepts
    // Enter while open):
    // 1. only when the caret token IS the whole draft (the command head
    //    being typed) — the engine arms the trigger on any whitespace-
    //    delimited "/token", but a prompt mentioning "/src" mid-sentence
    //    and a command ARGUMENT containing a slash token ("/steer fix
    //    /s") must submit, never complete (review finding: Enter
    //    rewrote the argument into "/steer fix /skills ");
    // 2. a query that already IS a command (canonical or alias — `parse`
    //    is the one authority) yields no candidates, so a fully-typed
    //    command submits on the first Enter.
    //
    // Known trade-off of rule 2 (deliberate): because ALIASES count as
    // fully-typed commands, the dropdown closes mid-word at `/q`,
    // `/skill`, `/detail`, `/session`, `/model` en route to the longer
    // spellings (`/quit`, `/skills`, `/details`, `/sessions`,
    // `/models`) and only reopens if the continuation stops being a
    // command. First-Enter-submits for every spelling `parse` accepts
    // is worth that flicker — an alias that kept the dropdown open
    // would swallow the Enter meant to run it.
    let dropdown_state = state.clone();
    let area = abstracttui::app::anchored::Completion::new()
        .trigger('/', move |query| {
            if dropdown_state.text().trim() != format!("/{query}") {
                return Vec::new();
            }
            if !matches!(
                crate::commands::parse(&format!("/{query}")),
                None | Some(crate::commands::Command::Unknown(_))
            ) {
                return Vec::new();
            }
            crate::commands::COMPLETIONS
                .iter()
                .filter(|(c, _)| c.starts_with(query))
                .map(|(c, hint)| {
                    abstracttui::app::anchored::CompletionCandidate::new(
                        format!("/{c}"),
                        format!("/{c} "),
                    )
                    .detail(*hint)
                })
                .collect()
        })
        .attach(cx, overlays, state, area);
    // No outer border: the TextArea draws its own `▐ ▌` side strokes —
    // `Block::new()` defaults to a Plain box, which double-framed the
    // composer AND stole a row (the caret line scrolled out of view at
    // 4 lines; adversary P1, 2026-07-22). The Block remains only for
    // the surface fill.
    abstracttui::widgets::Block::new()
        .border(abstracttui::widgets::BorderKind::None)
        .fill(t.surface)
        .layout(LayoutStyle::column())
        .child(area)
        .element(t)
        .build()
}

/// Status bar: key legend + theme + gateway.
pub fn status_bar(t: &TokenSet, store: Store, ctx: &UiCtx) -> View {
    let tokens = *t;
    let gateway = ctx.gateway_label.clone();
    dyn_view(LayoutStyle::line(1), move || {
        let t = tokens;
        let conn = store.conn.get();
        let theme_label = abstracttui::app::current_theme().label;
        let gateway = gateway.clone();
        Element::new()
            .style(LayoutStyle::line(1))
            .draw(move |canvas, rect| {
                canvas.fill(rect, ' ', t.text, t.surface);
                // Right side measured FIRST so the key legend clips under
                // it instead of overprinting mid-word at 80 cols
                // (adversary P3, 2026-07-22 — same rule as the header).
                let right = match &conn {
                    Conn::Down(msg) => format!(
                        "{theme_label} · {gateway} · {}",
                        text::truncate_ellipsis(msg, 40)
                    ),
                    _ => format!("{theme_label} · {gateway}"),
                };
                let w = text::width(&right) + 1;
                let rx = (rect.right() - w).max(rect.x);
                let clip_to = (rx - 1).max(rect.x);
                let mut x = rect.x + 1;
                let print_clipped =
                    |canvas: &mut dyn abstracttui::ui::Canvas, x: &mut i32, s: &str, ink| {
                        let avail = (clip_to - *x).max(0);
                        if avail <= 0 {
                            return;
                        }
                        let fitted = text::truncate_ellipsis(s, avail);
                        *x += canvas.print(Point::new(*x, rect.y), &fitted, ink, t.surface);
                    };
                for (key, label) in [
                    ("enter", "send/steer"),
                    ("esc esc", "cancel"),
                    ("ctrl+d", "details"),
                    ("pgup/dn", "scroll"),
                    ("ctrl+t", "theme"),
                    ("/help", "commands"),
                ] {
                    print_clipped(canvas, &mut x, key, t.accent);
                    print_clipped(canvas, &mut x, " ", t.text);
                    print_clipped(canvas, &mut x, label, t.text_muted);
                    print_clipped(canvas, &mut x, "  ", t.text);
                }
                let ink = if matches!(conn, Conn::Down(_)) {
                    t.error
                } else {
                    t.text_faint
                };
                canvas.print(Point::new(rx, rect.y), &right, ink, t.surface);
            })
            .build()
    })
}

pub fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{:.0}k", n as f64 / 1_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
