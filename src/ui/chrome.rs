//! Fixed chrome: header, activity strip, composer, status bar.

use abstracttui::prelude::*;
use abstracttui::text;
use abstracttui::widgets::Sparkline;

use crate::store::{Conn, Phase, Store};
use crate::ui::UiCtx;

/// The effective provider/model label — the honesty-upgraded "gateway
/// defaults" resolution shared by the header and the idle fact card
/// (IDLE-1). Reads signals: call inside a reactive scope.
pub fn route_label(store: Store) -> String {
    let provider = store.provider.get();
    let model = store.model.get();
    let base = match (provider.is_empty(), model.is_empty()) {
        (true, true) => {
            // Honesty upgrade: name what "gateway defaults" resolves to.
            // Best truth first: the model a run actually used; else the
            // gateway's configured text route; else the bare label.
            // One FORMAT either way — `provider · model` (the provider
            // silently vanishing after the first run read as data loss;
            // adversary P3, 2026-07-22).
            let served = store.fold.with(|f| f.stats.effective_model.clone());
            let (dp, dm) = store.default_route.get();
            // Join the non-empty halves: a model-only capability route
            // must never render a dangling "( · model)" pair (cycle-2
            // review P2-E — the join is the format, absence is omission).
            let pair = |provider: &str, model: &str| -> String {
                [provider, model]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" · ")
            };
            let resolved = if !served.is_empty() {
                pair(&dp, &served)
            } else {
                pair(&dp, &dm)
            };
            if resolved.is_empty() {
                "gateway defaults".to_string()
            } else {
                format!("gateway defaults ({resolved})")
            }
        }
        (false, true) => provider,
        (true, false) => model,
        (false, false) => format!("{provider} · {model}"),
    };
    // The third axis (first-citizen directive): the reasoning override
    // joins the route label — absence is omission (no override = the
    // server default, unlabeled), the P2-E join rule.
    let reasoning = store.reasoning.get();
    if reasoning.is_empty() {
        base
    } else {
        format!("{base} · {reasoning}")
    }
}

/// The header's middle-span facts (HDR-1): cwd basename, workspace mode,
/// `skills N · mcp N` when nonzero, session token total at rest. Pure
/// over its inputs so the fill rule is testable; the ~180 blank columns
/// at wide widths were the literal screenshot complaint.
pub fn header_facts(
    cwd_base: &str,
    workspace_mode: &str,
    skills: usize,
    mcp: usize,
    idle: bool,
    session_tokens: u64,
) -> Vec<String> {
    let mut facts: Vec<String> = Vec::new();
    if !cwd_base.is_empty() {
        facts.push(format!("⌂ {cwd_base}"));
    }
    facts.push(if workspace_mode.trim().is_empty() {
        "server-managed".to_string()
    } else {
        workspace_mode.trim().to_string()
    });
    if skills > 0 {
        facts.push(format!("skills {skills}"));
    }
    if mcp > 0 {
        facts.push(format!("mcp {mcp}"));
    }
    // Session token total AT REST only: during a run the activity strip
    // carries the live numbers — a second live counter would just race it.
    if idle && session_tokens > 0 {
        facts.push(format!("{} tk", fmt_tokens(session_tokens)));
    }
    facts
}

/// Header: wordmark · workflow · provider/model · entity chips · facts ·
/// session · connection orb. Chips render only when ≥1 conversation
/// exists; the focused chip is highlighted; a running turn's chip
/// carries elapsed.
pub fn header(t: &TokenSet, store: Store, spin: Signal<u64>, cwd_base: String) -> View {
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
        let session = store.session_id.get();
        let conn = store.conn.get();
        // Entity chips (inside this same line — no CHROME_ROWS change).
        // spin is read ONLY while a turn/run is active, so the elapsed
        // repaints without waking the header every frame otherwise.
        // Reading it during AGENT runs too is HDR-2(b): the header's
        // dyn re-renders every tick while anything runs, so its row is
        // freshly damaged in the model — a damage gap (toast vacating
        // over it, layer churn) can never leave stale header pixels for
        // the rest of a long run. Idle stays zero-wakeup (the ticker is
        // off), and re-renders that change nothing emit zero bytes.
        let focus = store.focus.get();
        let any_turn = store.convos.with(|cs| {
            cs.iter()
                .any(|c| c.status == crate::convo::ConvoStatus::TurnRunning)
        });
        let phase = store.phase.get();
        if any_turn || phase != Phase::Idle {
            let _ = spin.get();
        }
        let chips: Vec<(String, bool)> = store.convos.with(|cs| {
            cs.iter()
                .map(|c| {
                    let focused = matches!(&focus, crate::convo::Focus::Entity(n) if *n == c.name);
                    (c.chip(), focused)
                })
                .collect()
        });
        let route = route_label(store);
        // HDR-1: cockpit facts fill the middle (dim, clipped whole-run,
        // painted AFTER chips so chips keep priority when space is tight).
        let facts = header_facts(
            &cwd_base,
            &store.workspace_mode.get(),
            store.selected_skills.with(|s| s.len()),
            store.mcp_servers.with(|m| m.len()),
            phase == Phase::Idle,
            store.totals.with(|s| s.total_tokens),
        );
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
                    Conn::Down(..) => ("✗", t.error),
                };
                // Char-safe TAIL truncation (session ids differ at the
                // end): one shared helper — see `tail_ellipsis`.
                let right = format!("{} ", tail_ellipsis(&session, 18));
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
                // Chips render WHOLE or not at all; the tail collapses to
                // an honest "+N" instead of a mangled fragment ("◆eph…" —
                // cycle-2 UX review at 100 cols with 3 conversations).
                // The FOCUSED chip always paints (first, when it would
                // otherwise fall into the tail) — PAINT order only: the
                // Alt+E cycle order is the convos vec order and never
                // follows this reordering (see `convo::chip_paint_plan`).
                let widths: Vec<usize> = chips
                    .iter()
                    .map(|(c, _)| text::width(c).max(0) as usize)
                    .collect();
                let focused_ix = chips.iter().position(|(_, f)| *f);
                let avail = (clip_to - x).max(0) as usize;
                let (paint, hidden) = crate::convo::chip_paint_plan(&widths, focused_ix, avail);
                for ix in paint {
                    let (chip, focused) = &chips[ix];
                    print_clipped(canvas, &mut x, "  ", t.text);
                    print_clipped(
                        canvas,
                        &mut x,
                        chip,
                        if *focused { t.accent } else { t.text_muted },
                    );
                }
                if hidden > 0 {
                    print_clipped(canvas, &mut x, &format!("  +{hidden}"), t.text_faint);
                }
                // Facts fill the middle (HDR-1): dim values, faint
                // separators — information-carrying, so muted ink (the
                // faint tier is decoration-only by the theme contract).
                // Facts drop WHOLE from the right when space runs out
                // (POLISH-1: instruments never self-truncate into `…`
                // fragments — ellipsis is for prose); paint order
                // already gives workflow/route/chips priority over them.
                let fact_widths: Vec<usize> = facts
                    .iter()
                    .map(|f| text::width(f).max(0) as usize)
                    .collect();
                let fit = prefix_fit(&fact_widths, 5, 3, (clip_to - x).max(0) as usize);
                for (i, fact) in facts.iter().take(fit).enumerate() {
                    let sep = if i == 0 { "  ·  " } else { " · " };
                    x += canvas.print(Point::new(x, rect.y), sep, t.text_faint, t.surface);
                    x += canvas.print(Point::new(x, rect.y), fact, t.text_muted, t.surface);
                }

                let mut x2 = rx;
                // Session id in MUTED ink: the faint tier measured 2.77:1
                // on the default theme — below the 3:1 UI floor; "the
                // brightest thing in the right two-thirds of that bar"
                // must clear it (review-current-state §3).
                x2 += canvas.print(Point::new(x2, rect.y), &right, t.text_muted, t.surface);
                canvas.print(Point::new(x2, rect.y), orb, orb_ink, t.surface);
            })
            .build()
    })
}

/// Activity strip: spinner + status + cycle + elapsed + token sparkline.
/// `follow` is the transcript's tail-pin signal: while the user is
/// scrolled UP into history the strip says so — the run appending (or
/// concluding) below a reading user was otherwise invisible
/// (visibility review P1-2: the signal was written five times and
/// rendered nowhere; the engine contract explicitly invites rendering
/// it).
pub fn activity_strip(t: &TokenSet, store: Store, spin: Signal<u64>, follow: Signal<bool>) -> View {
    let tokens = *t;
    dyn_view(LayoutStyle::line(1), move || {
        let t = tokens;
        let scrolled_up = !follow.get();
        // A pending wait OWNS the strip, unconditionally: later records from
        // other subruns overwrite the activity text, and a deferred prompt
        // left NO visible trace (live finding: "awaiting approval" card with
        // no modal and no way back). This line cannot be overwritten while
        // the wait is pending.
        let focus = store.focus.get();
        let entity_focus = matches!(focus, crate::convo::Focus::Entity(_));
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
            // DELIBERATE exception: a pending AGENT wait owns the strip in
            // ANY focus (an approval is urgent); the prefix names the lane.
            let prefix = if entity_focus { "agent: " } else { "" };
            // A forgotten approval visibly AGES (visibility review
            // P2-4): the run clock is the honest anchor we have — how
            // long the run has been alive while parked on you.
            let age = crate::convo::fmt_elapsed(store.elapsed_secs.get());
            let text = format!("⏸ {prefix}{what} · run {age} · press Enter to open the prompt");
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
        // stays paused on the gateway across restarts). Same lane-prefix
        // rule as the wait above: in entity focus the line must say WHICH
        // conversation is paused — entity turns are non-interruptible and
        // never pause, so an unprefixed "run paused" read as the visit
        // (cycle-2 review P2-D).
        if store.paused.get() {
            let warn = t.warn;
            let prefix = if entity_focus { "agent: " } else { "" };
            let age = crate::convo::fmt_elapsed(store.elapsed_secs.get());
            let text = format!(
                "⏸ {prefix}run paused durably on the gateway · run {age} · /resume continues"
            );
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
        // Entity focus: the strip mirrors the FOCUSED conversation
        // (status word · turn elapsed · held-draft marker · spend delta
        // from /cognition when polled — never a fabricated count).
        if let crate::convo::Focus::Entity(name) = &focus {
            let frame = spin.get();
            let view = store.convos.with(|cs| {
                cs.iter().find(|c| c.name == *name).map(|c| {
                    (
                        c.status,
                        c.turn_started,
                        c.held_draft.clone(),
                        c.spend_tokens,
                    )
                })
            });
            let Some((status, started, held, spend)) = view else {
                return Element::new().style(LayoutStyle::line(1)).build();
            };
            let mut parts = vec![match status {
                crate::convo::ConvoStatus::Opening => format!("opening a visit with {name}…"),
                crate::convo::ConvoStatus::Ready => format!("{name} is present — Enter sends"),
                crate::convo::ConvoStatus::TurnRunning => format!(
                    "{name} is working ({}) — non-interruptible",
                    crate::convo::fmt_elapsed(started.map(|s| s.elapsed().as_secs()).unwrap_or(0))
                ),
                crate::convo::ConvoStatus::Parked => {
                    format!("parked — {name} waits for your next message")
                }
                crate::convo::ConvoStatus::Closed => format!("visit closed — @{name} reopens"),
                crate::convo::ConvoStatus::Refused => {
                    "visit refused — see the notice above".to_string()
                }
            }];
            // Held-draft marker ONLY while a hold can exist (Opening/
            // TurnRunning): next to "parked — …" the old unconditional
            // marker promised a send that already happened or never would
            // (cycle-2 UX review). Wording names the actual boundary.
            if !held.trim().is_empty() {
                match status {
                    crate::convo::ConvoStatus::Opening => {
                        parts.push("draft held (sends when the visit opens)".into());
                    }
                    crate::convo::ConvoStatus::TurnRunning => {
                        parts.push("draft held (sends when the turn parks)".into());
                    }
                    _ => {}
                }
            }
            if let Some(tk) = spend {
                parts.push(format!("visit spend {} tk", fmt_tokens(tk)));
            }
            let label = parts.join("  ·  ");
            let busy = matches!(
                status,
                crate::convo::ConvoStatus::Opening | crate::convo::ConvoStatus::TurnRunning
            );
            if busy {
                return Element::new()
                    .style(
                        LayoutStyle::row()
                            .h(1)
                            .gap(1)
                            .padding(Edges::hv(1, 0))
                            .width(Dimension::Percent(1.0)),
                    )
                    .child(crate::ui::thinking::element(&t, frame, label).build())
                    .build();
            }
            let faint = t.text_faint;
            return Element::new()
                .style(LayoutStyle::line(1))
                .draw(move |canvas, rect| {
                    let fitted = abstracttui::text::truncate_ellipsis(&label, (rect.w - 2).max(4));
                    canvas.print(
                        Point::new(rect.x + 1, rect.y),
                        &fitted,
                        faint,
                        Rgba::TRANSPARENT,
                    );
                })
                .build();
        }

        let phase = store.phase.get();
        // Queue visibility rides the strip in EVERY phase (plan item 1):
        // a held queue with no trace is invisible work.
        let queued = store.queue.with(|q| q.len());
        let queue_paused = store.queue_paused.get();
        let queue_part = if queued > 0 && queue_paused {
            format!("{queued} queued (paused — /queue resumes)")
        } else if queued > 0 {
            format!("{queued} queued")
        } else {
            String::new()
        };
        if phase == Phase::Idle {
            let totals = store.totals.get();
            // A restore that failed and may yet succeed: shown WHILE
            // the condition holds and gone the moment it resolves
            // (operator, 2026-08-31 — an ephemeral fault does not
            // belong in the transcript, which is a permanent record).
            // The reconnect retries it, so the words promise the
            // recovery this client actually performs rather than
            // asking the operator to re-select by hand.
            if let Some(reason) = store.restore_failed.get() {
                let summary = format!(
                    "session history not restored ({reason}) — retrying when the gateway answers"
                );
                return Element::new()
                    .style(LayoutStyle::line(1))
                    .draw(move |canvas, rect| {
                        let fitted =
                            abstracttui::text::truncate_ellipsis(&summary, (rect.w - 2).max(4));
                        canvas.print(
                            Point::new(rect.x + 1, rect.y),
                            &fitted,
                            t.warn,
                            Rgba::TRANSPARENT,
                        );
                    })
                    .build();
            }
            // Boot/switch rehydration window says what it is doing —
            // "no runs yet" during up-to-21 bundle fetches was a lie
            // about a session with history in flight (review P2-7).
            if store.restoring.get() || store.history_loading.get() {
                // Same surface, two windows: the boot rehydration and a
                // history-bloc stream (scroll-top auto-load / /history).
                let summary = if store.restoring.get() {
                    // The loading screen above carries the bar; this row
                    // repeats the counter so the fact survives even at
                    // pane heights where the caption rows clip away.
                    match store.restore_progress.get() {
                        Some((done, total)) if total > 0 => format!(
                            "restoring session history from the gateway… (turn {done} of {total})"
                        ),
                        _ => "restoring session history from the gateway…".to_string(),
                    }
                } else {
                    format!(
                        "streaming earlier history from the gateway… ({} turn(s) remain)",
                        store.older_turns.get()
                    )
                };
                return Element::new()
                    .style(LayoutStyle::line(1))
                    .draw(move |canvas, rect| {
                        let fitted =
                            abstracttui::text::truncate_ellipsis(&summary, (rect.w - 2).max(4));
                        canvas.print(
                            Point::new(rect.x + 1, rect.y),
                            &fitted,
                            t.text_faint,
                            Rgba::TRANSPARENT,
                        );
                    })
                    .build();
            }
            if totals.runs == 0 && queued == 0 {
                // REST-1: a fresh session shows the session card line —
                // the reserved strip row was a permanently blank line on
                // first launch (review-current-state §4.1).
                let session = store.session_id.get();
                let sid = tail_ellipsis(&session, 24);
                let summary = format!("session {sid} · no runs yet — Enter sends the first task");
                return Element::new()
                    .style(LayoutStyle::line(1))
                    .draw(move |canvas, rect| {
                        let fitted =
                            abstracttui::text::truncate_ellipsis(&summary, (rect.w - 2).max(4));
                        canvas.print(
                            Point::new(rect.x + 1, rect.y),
                            &fitted,
                            t.text_faint,
                            Rgba::TRANSPARENT,
                        );
                    })
                    .build();
            }
            let last_ctx = store.fold.with(|f| f.stats.last_input_tokens);
            let ctx_part = if last_ctx > 0 {
                format!(" · ctx {} tk", fmt_tokens(last_ctx))
            } else {
                String::new()
            };
            // Splitless-usage providers report only totals (bug (e)):
            // show the honest total instead of a false "0 in / 0 out".
            // ALL-zero totals (runs that died before any usage receipt)
            // render NO tokens part at all — a "0 in / 0 out" there
            // claims a measurement that never happened (render-when-
            // known; cycle-2 review P1-B).
            let tokens_part = if totals.input_tokens > 0 || totals.output_tokens > 0 {
                format!(
                    " · {} in / {} out tk",
                    fmt_tokens(totals.input_tokens),
                    fmt_tokens(totals.output_tokens)
                )
            } else if totals.total_tokens > 0 {
                format!(" · {} tk total", fmt_tokens(totals.total_tokens))
            } else {
                String::new()
            };
            let queue_suffix = if queue_part.is_empty() {
                String::new()
            } else {
                format!(" · {queue_part}")
            };
            // "Did it finish?" answered from fixed chrome (review P1-1
            // optional half): the newest conclusion's outcome leads the
            // idle line — the transcript marker alone lives at the tail
            // the user may not be looking at.
            let done_part = {
                let note = store.fold.with(|f| f.done_note.clone());
                if note.is_empty() {
                    String::new()
                } else {
                    format!("last run: {note} · ")
                }
            };
            // Scrolled-up honesty while idle: the conclusion landed
            // BELOW a reading user (review P1-2).
            let scroll_part = if scrolled_up {
                " · scrolled up — Esc jumps to the newest"
            } else {
                ""
            };
            let runs_word = if totals.runs == 1 { "run" } else { "runs" };
            let summary = format!(
                "{done_part}session: {} {runs_word}{tokens_part}{ctx_part}{queue_suffix}{scroll_part} · Enter sends the next task",
                totals.runs,
            );
            return Element::new()
                .style(LayoutStyle::line(1))
                .draw(move |canvas, rect| {
                    // Ellipsize like every sibling branch: canvas.print
                    // clips at the SCREEN edge mid-word, not the pane
                    // (cycle-2 review P1-B — the only unclipped strip
                    // line hard-cut at 60-80 cols).
                    let fitted =
                        abstracttui::text::truncate_ellipsis(&summary, (rect.w - 2).max(4));
                    canvas.print(
                        Point::new(rect.x + 1, rect.y),
                        &fitted,
                        t.text_faint,
                        Rgba::TRANSPARENT,
                    );
                })
                .build();
        }

        let frame = spin.get();
        // ATTRIBUTED by the fold (`cycle_gist`): whose words these are is
        // a question about ledger provenance, not about chrome.
        let cycle_gist = store.fold.with(|f| f.cycle_gist());
        let (activity, cycle, stats) = store.fold.with(|f| {
            // Stale-wait truth filter (lane-3 conformance, 2026-07-23):
            // the two wait-claiming activity strings are written ONLY by
            // `consider_wait`, together with `pending_wait` — and this
            // branch renders only when NO wait is pending (the pending
            // branch above owns the true-waiting render). Reaching here
            // with a wait-claiming text means the wait resolved
            // ELSEWHERE (another app approved it — the observer
            // scenario; the fold's answered-elsewhere rule clears the
            // wait but not the text) and no later record has replaced
            // the text yet: render the honest generic label instead of
            // a wait that no longer exists. The root fix (clear
            // `activity` with the wait) lives in the fold —
            // transcript.rs is another lane's file this wave
            // (docs/roadmap/conformance-handoffs-lane3.md).
            let activity = if matches!(
                f.activity.as_str(),
                "waiting for tool approval" | "waiting for your answer"
            ) {
                String::new()
            } else {
                f.activity.clone()
            };
            (activity, f.cycle, f.stats.clone())
        });
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
            // Skip the cycle chip when the activity text already names it
            // ("thinking (cycle 1) · cycle 1" read twice; live review,
            // 2026-07-22).
            let base_names_cycle = base.contains(&format!("cycle {cycle}"));
            // Cycle intent (review P2-1): a long call names what it is
            // attempting in the model's own words — only while the
            // activity IS a thinking label (tool transitions replace
            // the activity, hiding a stale gist for free).
            //
            // ATTRIBUTED (operator report 2026-08-21). The gateway's
            // ledger carries a cycle's words only in its RESULT record,
            // so while cycle N is in flight the newest gist we hold is
            // an EARLIER cycle's. Rendering it with an em-dash read as
            // "this is what cycle N is thinking" — the strip said
            // "thinking (cycle 2) — “I'll inspect the project…”" while
            // cycle 2 was actually writing "I found an empty
            // workspace…". The words are still worth showing; they just
            // have to say whose they are, and `Fold::cycle_gist` is the
            // one place that decides.
            let base = match (base.starts_with("thinking (cycle"), &cycle_gist) {
                (true, Some(crate::transcript::CycleGist::Own(g))) => format!("{base} — “{g}”"),
                (true, Some(crate::transcript::CycleGist::Last(g))) => {
                    format!("{base} · last: “{g}”")
                }
                _ => base,
            };
            let mut parts = vec![base];
            // The active /goal names itself on the strip (the composer is
            // captured for the whole loop — the strip says why).
            if let Some(g) = store.goal.get() {
                if !g.run_id.is_empty() && g.run_id == store.run_id.get() {
                    parts.insert(0, format!("goal: {}", crate::ui::queue_preview(&g.text)));
                }
            }
            if cycle > 0 && !base_names_cycle {
                parts.push(format!("cycle {cycle}"));
            }
            // OBS-1a-live: the in-flight model call names itself from
            // the FIRST second — elapsed from the started record
            // (client clock), throughput from the LAST completed call's
            // usage receipt, labeled "(last call)". This replaces the
            // frozen dead-air window (the top "feels worse" driver);
            // the ≥60s slow-provider hint stays appended inside the
            // segment (live finding: an MLX 27B at ~0.25 tok/s looked
            // idle; the truth was slow inference).
            if let Some(since) = store.fold.with(|f| f.llm_inflight_since) {
                let s = since.elapsed().as_secs();
                // Conn-aware (review P2-3): during a gateway outage the
                // ≥60s hint blamed the provider while the status bar's
                // orb said Down — two chrome surfaces, two stories.
                let conn_down = matches!(store.conn.get(), crate::store::Conn::Down(..));
                parts.push(model_call_segment(s, store.last_call_rate.get(), conn_down));
            }
            // The tool twin (live P0, 2026-07-23): a search_files over an
            // unignored build tree executed 8m39s gateway-side while the
            // strip said only "running search_files" — no clock, so it
            // read as a client hang. The activity text already NAMES the
            // tools; this adds the elapsed + the ≥60s "gateway-side"
            // teaching so a long scan is visibly the server working, not
            // the client stuck.
            if let Some(since) = store.fold.with(|f| f.tool_inflight_since) {
                let label = store.fold.with(|f| f.inflight_tool_label.clone());
                parts.push(tool_call_segment(
                    since.elapsed().as_secs(),
                    label.as_deref(),
                ));
            }
            // POLISH-1: `9h20m`/`3m05s`, never a raw `33628s`.
            parts.push(crate::convo::fmt_elapsed(elapsed));
            // Splitless-usage providers report only total_tokens (live
            // coder run: "0↑ 0↓ tk" against "23 tools" — bug (e)); show
            // the honest total instead of a false zero split. Before the
            // FIRST receipt (all three zero — run just started) show
            // NOTHING: "0↑ 0↓ tk" beside "model call 0s" claimed a
            // measurement that had not happened yet (cycle-2 live
            // capture, frame-02).
            if stats.input_tokens > 0 || stats.output_tokens > 0 {
                parts.push(format!(
                    "{}↑ {}↓ tk",
                    fmt_tokens(stats.input_tokens),
                    fmt_tokens(stats.output_tokens)
                ));
            } else if stats.total_tokens > 0 {
                parts.push(format!("{} tk", fmt_tokens(stats.total_tokens)));
            }
            if stats.last_input_tokens > 0 {
                // The context the model saw on its latest call — the live
                // "how full is the conversation" number. "~" marks a value
                // DERIVED from a zero-poisoned usage split (total − output)
                // — an estimate labeled, never a stale number (the frozen
                // "ctx 4.0k" of the 2026-07-23 incident).
                let approx = if stats.last_input_is_estimate {
                    "~"
                } else {
                    ""
                };
                parts.push(format!(
                    "ctx {approx}{}",
                    fmt_tokens(stats.last_input_tokens)
                ));
            }
            if stats.cached_tokens > 0 {
                parts.push(format!("cache {}", fmt_tokens(stats.cached_tokens)));
            }
            if stats.tool_calls > 0 {
                // Failure streaks visible at a glance (review P2-2):
                // "38 tools · 5 ✗" instead of ✗ cards scrolling past.
                if stats.tool_failures > 0 {
                    parts.push(format!(
                        "{} tools ({} ✗)",
                        stats.tool_calls, stats.tool_failures
                    ));
                } else {
                    parts.push(format!("{} tools", stats.tool_calls));
                }
            }
            if !queue_part.is_empty() {
                parts.push(queue_part.clone());
            }
            // Scrolled-up honesty (review P1-2): the run keeps appending
            // BELOW a reading user — say so from fixed chrome, and name
            // the way back.
            if scrolled_up {
                parts.push("scrolled up · Esc returns to live".into());
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
            // The working indicator is the app's own wave, not the
            // engine Spinner: six cells moving in height AND ink read as
            // "working" from across the room, where one accent dot read
            // as punctuation (operator report, 2026-08-19). See
            // `ui::thinking` for the theme-floor and exact-wrap rules.
            .child(crate::ui::thinking::element(&t, frame, label).build())
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

/// The composer's live instance id, republished on every focus gain.
///
/// The engine assigns ids at MOUNT, and this composer remounts on every
/// chrome rebuild (theme, phase, focus-lane, caps upgrade), so the app
/// cannot hold one forever — it re-reads it from the FocusIn the
/// remount's `.autofocus()` delivers. The root's type-to-focus handler
/// is the only consumer: it needs a `ViewId` for
/// `EventCtx::request_focus`, and there is no other app-visible route
/// to one. `None` before the first mount; a stale id (generational key)
/// makes `set_focus` a no-op rather than focusing a stranger.
#[derive(Clone, Default)]
pub struct ComposerAnchor(std::rc::Rc<std::cell::Cell<Option<abstracttui::ui::ViewId>>>);

impl ComposerAnchor {
    /// The live composer instance, or `None` before the first mount.
    pub fn get(&self) -> Option<abstracttui::ui::ViewId> {
        self.0.get()
    }

    fn publish(&self, id: Option<abstracttui::ui::ViewId>) {
        self.0.set(id);
    }
}

/// Composer: multiline `TextArea` (grows 1..4 rows, Enter submits,
/// Ctrl+J inserts a newline EVERYWHERE — engine-owned since abstracttui
/// 0.2.2 (our 0295 ask): the edit model consumes the chord under every
/// submit policy, so the app-side shortcut + helper are deleted.
/// Alt+Enter and, where the kitty keyboard protocol is live,
/// Shift+Enter also insert one; block paste inserts whole; ↑/↓ recall
/// submitted history at the buffer edges. A `/`-command completion
/// dropdown anchors at the caret. Rebuilt on THEME + focus + phase
/// changes; the durable `TextAreaState` lives in root scope, so drafts
/// + caret + history survive the rebuild.
///
/// `.autofocus()` re-fires on every dyn regeneration (theme switches), so
/// boot focus and post-theme-switch focus need no app bookkeeping
/// (abstracttui 0.2.0; the 0.1.0 autofocus-in-dyn panic is fixed).
///
/// `anchor` receives the mounted instance id (see [`ComposerAnchor`]) so
/// the root can hand focus BACK here when the user types with the
/// keyboard parked on the transcript.
#[allow(clippy::too_many_arguments)]
pub fn composer(
    cx: Scope,
    t: &TokenSet,
    store: Store,
    state: &abstracttui::widgets::TextAreaState,
    anchor: &ComposerAnchor,
    overlays: &abstracttui::app::Overlays,
    placeholder: String,
    on_submit: impl FnMut(&str) + Clone + 'static,
) -> View {
    let mut submit = on_submit;
    let submit_state = state.clone();
    let anchor = anchor.clone();
    let area = abstracttui::widgets::TextArea::new()
        .state(state)
        // The chat convention: Enter sends, Shift+Enter inserts a newline.
        // Shift+Enter is only REPORTABLE where the kitty keyboard
        // protocol is live (probe-upgraded mid-session since 0.2.2:
        // kitty/Ghostty/foot, iTerm2 ≥ 3.5, VS Code/Cursor, Warp);
        // legacy terminals send plain \r for both, so Ctrl+J (engine
        // edit model, every submit policy) and Alt+Enter (Option+Enter
        // on macOS with "Option as Meta/Esc+" enabled) are the universal
        // spellings. No selection-clear hook: since 0.2.2 (our 0290)
        // EVERY copy ends the drag gesture and clears the region, so
        // typing after a copy routes normally without app help.
        // Placeholder is focus- and phase-aware (root dyn rebuild swaps it).
        // `placeholder_while_focused` (0.2.6, our 0291): the engine paints
        // the hint beside the caret while the composer is focused-and-empty
        // — this composer autofocuses, so without it the phase teaching
        // was dead pixels (HDR-2c). The app-side absolute overlay that
        // worked around it is deleted; exactly one renderer paints in
        // each focus state, engine-side.
        .placeholder(placeholder)
        .placeholder_while_focused(true)
        // Drop-as-paste (engine 0.2.19, our 0273 ask): the hook sees
        // the RAW paste before insertion; a verified file drop becomes
        // attachment chips (Consume — nothing inserted), everything
        // else inserts byte-identical to an unhooked composer. The
        // classify/existence split and the Ctrl+O undo live in
        // `ui::attachments::handle_paste`.
        .on_paste(move |raw| crate::ui::attachments::handle_paste(store, raw))
        .rows(1, 4)
        .on_submit(move |text| {
            let owned = text.to_string();
            if !owned.trim().is_empty() {
                submit_state.push_history(owned.trim());
            }
            submit_state.clear();
            submit(&owned);
        })
        .element(cx, t)
        // Publish the mounted instance to the anchor. FocusIn is
        // delivered TARGET-ONLY by the engine, and this element is the
        // focusable node (`TextArea::element` is what carries
        // `.focusable()`), so a bubble listener here hears exactly the
        // composer's own focus gains — boot autofocus, every dyn
        // rebuild's re-fire, and a Tab back — which keeps the recorded
        // id the LIVE one across rebuilds. Ids are generational, so a
        // stale one can never name a different widget.
        .on(
            abstracttui::ui::Phase::Bubble,
            move |ctx: &mut abstracttui::ui::EventCtx, ev: &abstracttui::ui::UiEvent| {
                if matches!(ev, abstracttui::ui::UiEvent::FocusIn) {
                    anchor.publish(ctx.current());
                }
            },
        )
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
        // 10 visible rows (POLISH-1/UX-14 row-cap lift): the command
        // surface outgrew the engine's 6-row default; longer lists
        // still window around the highlight.
        .max_visible(10)
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
            // Fuzzy match (POLISH-1/UX-14): prefix hits rank first, then
            // subsequence hits — `/wf` finds `/workflow`. The matcher is
            // `commands::completion_matches` (pure, tested there).
            crate::commands::completion_matches(query)
                .into_iter()
                .map(|(c, hint)| {
                    abstracttui::app::anchored::CompletionCandidate::new(
                        format!("/{c}"),
                        format!("/{c} "),
                    )
                    .detail(*hint)
                })
                .collect()
        })
        // '@' entity mentions — deliberately DIFFERENT rules from '/':
        // NO whole-draft guard (mid-prompt reference inserts are wanted;
        // Enter-accept inserts the name, the NEXT Enter submits), cached
        // roster only (never a synchronous fetch), and an exact-slug
        // query yields nothing so a fully-typed @castor submits on the
        // first Enter (rules live in `mention::candidates`, tested there).
        .trigger('@', move |query| {
            store.entities.with_untracked(|roster| {
                crate::mention::candidates(query, roster)
                    .into_iter()
                    .map(|(label, insert, detail)| {
                        abstracttui::app::anchored::CompletionCandidate::new(label, insert)
                            .detail(detail)
                    })
                    .collect()
            })
        })
        .attach(cx, overlays, state, area);
    // No outer border: the TextArea draws its own `▐ ▌` side strokes —
    // `Block::new()` defaults to a Plain box, which double-framed the
    // composer AND stole a row (the caret line scrolled out of view at
    // 4 lines; adversary P1, 2026-07-22). The Block remains only for
    // the surface fill.
    //
    // Accent `❯` prompt glyph (POLISH-1/UX-12): a 2-cell left gutter
    // says "this is where you type" — the glyph sits on the first row
    // while the TextArea grows 1..4 rows beside it.
    let glyph = {
        let accent = t.accent;
        let surface = t.surface;
        Element::new()
            .style(
                LayoutStyle::default()
                    .width(Dimension::Cells(2))
                    .shrink(0.0),
            )
            .draw(move |canvas, rect| {
                canvas.fill(rect, ' ', accent, surface);
                canvas.print(Point::new(rect.x, rect.y), "❯", accent, surface);
            })
            .build()
    };
    // `shrink(0.0)` on the composer ROW (2026-08-20): the TextArea
    // itself already refuses to shrink, but this ancestor did not, so
    // any overflow pressure in the chrome column bought a row back from
    // the composer. The engine then drew a 4-row-tall widget inside a
    // 3-row rect: the widget's own scroll window (computed against
    // `max_rows`, not the drawn height) kept the caret on row 4, which
    // the clip ate — typing past the visible rows scrolled the text out
    // from under the caret. A composer that cannot be crushed cannot
    // desync. The pressure itself is gone at its source (the transcript
    // pane's flex basis, see `transcript_view::pane`); this is the guard
    // the engine's own zero-collapse diagnostic prescribes for a row
    // that must never yield, so the next pressure source cannot make
    // typing invisible again.
    //
    // The shrink stays on the inner column: that child sits on the
    // Block's ROW main axis, where shrink is WIDTH — it is what pulls
    // the TextArea's `width: 100%` basis back to leave the 2-cell `❯`
    // gutter room (shrink 0 there overflows the right `▌` stroke off
    // the screen).
    abstracttui::widgets::Block::new()
        .border(abstracttui::widgets::BorderKind::None)
        .fill(t.surface)
        .layout(LayoutStyle::row().shrink(0.0))
        .child(glyph)
        .child(
            Element::new()
                .style(LayoutStyle::column().grow(1.0))
                .child(area)
                .build(),
        )
        .element(t)
        .build()
}

/// The `ctx` footer segment (CTX-0): used/window (%) against the
/// operator-DECLARED window; window-less keeps the honest absolute; a
/// declared-but-unmeasured window shows `—` (never a fabricated 0).
/// Severity: 0 normal · 1 warn (≥75%) · 2 error (≥90%). The label says
/// "declared" — the source is the operator, never a client capability
/// table (the 2026-07-17 fabricated-selection class is the hard line).
pub fn ctx_meter(last_input: u64, window: u64) -> Option<(String, u8)> {
    match (last_input, window) {
        (0, 0) => None,
        (used, 0) => Some((format!("ctx {} tk", fmt_tokens(used)), 0)),
        (0, w) => Some((format!("ctx —/{} tk (declared)", fmt_tokens(w)), 0)),
        (used, w) => {
            let pct = used.saturating_mul(100) / w.max(1);
            let sev = if pct >= 90 {
                2
            } else if pct >= 75 {
                1
            } else {
                0
            };
            Some((
                format!(
                    "ctx {}/{} tk ({pct}%, declared)",
                    fmt_tokens(used),
                    fmt_tokens(w)
                ),
                sev,
            ))
        }
    }
}

/// The in-flight model-call strip segment (OBS-1a-live): elapsed from
/// the started record, throughput from the LAST completed call's
/// receipts — labeled, never a projection (no predictions-as-status).
/// Elapsed goes through the ONE shared humanizer (`convo::fmt_elapsed`,
/// POLISH-1): `9h20m`/`1m23s`, never a raw `33628s`.
pub fn model_call_segment(elapsed_secs: u64, last_rate: Option<f64>, conn_down: bool) -> String {
    let mut seg = format!("model call {}", crate::convo::fmt_elapsed(elapsed_secs));
    if let Some(rate) = last_rate {
        if rate.is_finite() && rate > 0.0 {
            seg.push_str(&format!(" · {rate:.0} tok/s (last call)"));
        }
    }
    if elapsed_secs >= 60 {
        // One narrative per screen (review P2-3): when the CONNECTION
        // is the known problem, blaming the provider sent operators
        // chasing the wrong culprit while the status orb said Down.
        if conn_down {
            seg.push_str(" — gateway not responding (see the status bar)");
        } else {
            seg.push_str(" — provider may be slow");
        }
    }
    seg
}

/// The in-flight TOOL-batch strip segment (the model-call segment's twin,
/// live P0 2026-07-23): elapsed from the batch's started record via the
/// client clock. At ≥60s it says WHERE the time is going — tools execute
/// on the gateway, so a long scan is the server working, not this client
/// stuck (the operator's exact misread: "there is no way a list or search
/// would take that much time" — the ledger proved 8m39s of real
/// execution). Same shared humanizer as every elapsed (`fmt_elapsed`).
pub fn tool_call_segment(elapsed_secs: u64, label: Option<&str>) -> String {
    let mut seg = format!("tool call {}", crate::convo::fmt_elapsed(elapsed_secs));
    // NAME what is running — a labeled command is the first cue a human
    // needs to judge "is this stuck?" (observability wave 2026-07-27:
    // the 8h hang showed only "large scans can take minutes" while a
    // wedged server+browser probe was never named).
    if let Some(l) = label.map(str::trim).filter(|l| !l.is_empty()) {
        seg.push_str(" · ");
        seg.push_str(l);
    }
    // Tiered escalation. Sub-minute: silent. ≥1m: gateway-side note (a
    // scan can legitimately take minutes). ≥5m: unusual for a shell
    // command. ≥15m: WARN — the model is blocked on this result, so a
    // wedged tool stalls the whole run, not just this client. The
    // deadline itself is the backstop; this is the human's early cue.
    if elapsed_secs >= 900 {
        seg.push_str(" — possibly stuck; the model is blocked on this result");
    } else if elapsed_secs >= 300 {
        seg.push_str(" — long for a shell command; still running gateway-side");
    } else if elapsed_secs >= 60 {
        seg.push_str(" — executing gateway-side (a large scan can take minutes)");
    }
    seg
}

/// The footer's host-RAM segment from the LAST `/resources` fetch —
/// rendered only when a served percent is KNOWN (no polling lane feeds
/// this; it updates when the modal fetches). A STALE snapshot (refresh
/// failed) is marked `*` — last-good must never render indistinguishable
/// from fresh. Severity grades the ROUNDED percent the user actually
/// reads (89.5 displays "90%" and must grade as 90): ≥90 error, ≥75
/// warn, else muted. Pure so a unit test pins presence, grading, the
/// stale marker, and absence under every factless state.
pub(crate) fn mem_segment(state: &crate::store::HostState) -> Option<(String, u8)> {
    use crate::store::HostState;
    let (facts, stale) = match state {
        HostState::Ready(f) => (f, false),
        HostState::Stale(f) => (f, true),
        _ => return None,
    };
    let pct = facts.ram_percent?;
    let shown = pct.round();
    let sev = if shown >= 90.0 {
        2
    } else if shown >= 75.0 {
        1
    } else {
        0
    };
    let mark = if stale { "*" } else { "" };
    Some((format!("mem {shown:.0}%{mark}"), sev))
}

/// Status bar: the always-visible instrument row (REST-1) —
/// `ctx used/window (%) · session tokens · gpu · skills · mcp · ? help`
/// — plus theme + gateway on the right. The key legend moved behind `?`
/// (and /help): the Python predecessor's law, written after its own
/// adversary review, is "the footer is the only always-visible surface"
/// — it must carry NUMBERS, not a legend that is useful once and then
/// permanent dead weight (it truncated itself at 120 cols carrying
/// zero facts). Segments render when KNOWN; absence is omission, never
/// a fabricated zero.
pub fn status_bar(t: &TokenSet, store: Store, ctx: &UiCtx) -> View {
    let tokens = *t;
    let gateway = ctx.gateway_label.clone();
    dyn_view(LayoutStyle::line(1), move || {
        let t = tokens;
        let conn = store.conn.get();
        let theme_label = abstracttui::app::current_theme().label;
        let gateway = gateway.clone();
        // Facts, left to right. Each is (text, ink-class): 0 normal
        // (muted), 1 warn, 2 error — the ctx meter is the only graded one.
        let mut segs: Vec<(String, u8)> = Vec::new();
        let last_ctx = store.fold.with(|f| f.stats.last_input_tokens);
        if let Some(seg) = ctx_meter(last_ctx, store.context_window.get()) {
            segs.push(seg);
        }
        // Session tokens: the in/out split when the provider reports one
        // (the strip's ↑/↓ vocabulary); splitless providers show the
        // honest total (never fabricated zeros — bug (e) class).
        let totals = store.totals.get();
        if totals.input_tokens > 0 || totals.output_tokens > 0 {
            segs.push((
                format!(
                    "{}↑ {}↓ tk session",
                    fmt_tokens(totals.input_tokens),
                    fmt_tokens(totals.output_tokens)
                ),
                0,
            ));
        } else if totals.total_tokens > 0 {
            segs.push((format!("{} tk session", fmt_tokens(totals.total_tokens)), 0));
        }
        // The /gpu meter (OBS-6 seam): worker-owned poller fills the
        // signal; this row renders it. Off/Unsupported = omitted (the
        // /gpu toggle toasts the reason once — never a fabricated number).
        match store.gpu.get() {
            crate::store::GpuMeter::Ready(s) => {
                segs.push((format!("gpu {:.0}%", s.util_pct), 0));
            }
            crate::store::GpuMeter::Pending => segs.push(("gpu …".into(), 0)),
            crate::store::GpuMeter::Error(_) => segs.push(("gpu err".into(), 1)),
            crate::store::GpuMeter::Off | crate::store::GpuMeter::Unsupported(_) => {}
        }
        // Host RAM from the LAST /resources fetch (see `mem_segment`).
        if let Some(seg) = mem_segment(&store.host_state.get()) {
            segs.push(seg);
        }
        let skills = store.selected_skills.with(|s| s.len());
        if skills > 0 {
            segs.push((format!("skills {skills}"), 0));
        }
        let mcp = store.mcp_servers.with(|m| m.len());
        if mcp > 0 {
            segs.push((format!("mcp {mcp}"), 0));
        }
        Element::new()
            .style(LayoutStyle::line(1))
            .draw(move |canvas, rect| {
                canvas.fill(rect, ' ', t.text, t.surface);
                // Right side measured FIRST so the facts yield to it
                // instead of overprinting mid-word at 80 cols
                // (adversary P3, 2026-07-22 — same rule as the header).
                let right = match &conn {
                    Conn::Down(msg, _) => format!(
                        "{theme_label} · {gateway} · {}",
                        text::truncate_ellipsis(msg, 40)
                    ),
                    _ => format!("{theme_label} · {gateway}"),
                };
                let w = text::width(&right) + 1;
                let rx = (rect.right() - w).max(rect.x);
                let clip_to = (rx - 1).max(rect.x);
                let mut x = rect.x + 1;
                // Segments drop WHOLE, right-to-left, when the row is
                // tight (POLISH-1): an instrument row must never
                // self-truncate into `…` fragments — the old key legend
                // rendered "/help comm…" at 120 cols and read as broken
                // (SYNTHESIS §2 baseline capture). The keys pointer is
                // the LAST segment, so it is the first to yield.
                let hint = "? keys + commands";
                let seg_widths: Vec<usize> = segs
                    .iter()
                    .map(|(s, _)| text::width(s).max(0) as usize)
                    .chain(std::iter::once(text::width(hint).max(0) as usize))
                    .collect();
                let fit = prefix_fit(&seg_widths, 0, 5, (clip_to - x).max(0) as usize);
                for (i, (seg, sev)) in segs.iter().take(fit).enumerate() {
                    if i > 0 {
                        x += canvas.print(Point::new(x, rect.y), "  ·  ", t.text_faint, t.surface);
                    }
                    let ink = match sev {
                        2 => t.error,
                        1 => t.warn,
                        _ => t.text_muted,
                    };
                    x += canvas.print(Point::new(x, rect.y), seg, ink, t.surface);
                }
                // The keys pointer — the whole legend lives behind it.
                if fit > segs.len() {
                    if !segs.is_empty() {
                        x += canvas.print(Point::new(x, rect.y), "  ·  ", t.text_faint, t.surface);
                    }
                    x += canvas.print(Point::new(x, rect.y), "?", t.accent, t.surface);
                    canvas.print(
                        Point::new(x, rect.y),
                        " keys + commands",
                        t.text_muted,
                        t.surface,
                    );
                }
                let ink = if matches!(conn, Conn::Down(..)) {
                    t.error
                } else {
                    t.text_faint
                };
                canvas.print(Point::new(rx, rect.y), &right, ink, t.surface);
            })
            .build()
    })
}

/// Keep the TAIL of an identifier, `…`-prefixed, when it exceeds
/// `max_chars` — session ids differ at the END, so tail-keeping is the
/// readable cut (the engine's `truncate_ellipsis` keeps the head).
/// Char-safe: ids are user-supplied (`--session`, `/sessions`, prefs) and
/// a byte slice panicked on multibyte ids (adversary finding 3). One
/// helper for the header + strip sites — they had drifted into two inline
/// copies with different magic numbers (cycle-2 review P2-F).
pub fn tail_ellipsis(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        return s.to_string();
    }
    let keep = max_chars.saturating_sub(3).max(1);
    let tail: String = chars[chars.len() - keep..].iter().collect();
    format!("…{tail}")
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

/// How many LEADING items fit whole into `avail` columns (POLISH-1).
/// `first_sep`/`rest_sep` are the separator widths before the first /
/// subsequent items. Prefix-fit: the first item that misses stops the
/// paint, so the right tail collapses WHOLE — chrome instruments never
/// self-truncate into `…` fragments (ellipsis is for prose; a
/// fragmented `/help comm…` legend read as broken in the 120-col
/// baseline capture). Items are ordered most-important-first, so
/// prefix-fit IS "drop right-to-left".
pub fn prefix_fit(widths: &[usize], first_sep: usize, rest_sep: usize, avail: usize) -> usize {
    let mut used = 0usize;
    for (i, w) in widths.iter().enumerate() {
        let sep = if i == 0 { first_sep } else { rest_sep };
        match used.checked_add(sep + w) {
            Some(next) if next <= avail => used = next,
            _ => return i,
        }
    }
    widths.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctx_meter_grades_against_the_declared_window_only() {
        // No data at all: no segment (render-when-present).
        assert_eq!(ctx_meter(0, 0), None);
        // No declared window: today's honest absolute (never a
        // fabricated denominator).
        assert_eq!(ctx_meter(41_203, 0), Some(("ctx 41k tk".into(), 0)));
        // Declared but unmeasured: an em-dash, never a fabricated 0.
        assert_eq!(
            ctx_meter(0, 262_144),
            Some(("ctx —/262k tk (declared)".into(), 0))
        );
        // The Python reference shape, source-labeled.
        assert_eq!(
            ctx_meter(41_203, 262_144),
            Some(("ctx 41k/262k tk (15%, declared)".into(), 0))
        );
        // Thresholds: warn ≥75%, error ≥90%.
        assert_eq!(ctx_meter(74, 100).unwrap().1, 0);
        assert_eq!(ctx_meter(75, 100).unwrap().1, 1);
        assert_eq!(ctx_meter(90, 100).unwrap().1, 2);
        // Over-window (a served context larger than the declaration)
        // stays error, never panics.
        assert_eq!(ctx_meter(150, 100).unwrap().1, 2);
    }

    #[test]
    fn model_call_segment_ticks_from_second_zero_with_labeled_rate() {
        assert_eq!(model_call_segment(0, None, false), "model call 0s");
        assert_eq!(model_call_segment(14, None, false), "model call 14s");
        assert_eq!(
            model_call_segment(14, Some(41.4), false),
            "model call 14s · 41 tok/s (last call)"
        );
        // ≥60s keeps the slow-provider hint and humanizes the elapsed.
        assert_eq!(
            model_call_segment(83, Some(0.9), false),
            "model call 1m23s · 1 tok/s (last call) — provider may be slow"
        );
        // Hours humanize through the ONE shared fmt (POLISH-1): a raw
        // `33628s` must never render anywhere in chrome.
        assert_eq!(
            model_call_segment(33_628, None, false),
            "model call 9h20m — provider may be slow"
        );
        // Conn-aware (review P2-3): a known-down gateway names ITSELF,
        // never the provider — one narrative per screen.
        assert_eq!(
            model_call_segment(83, None, true),
            "model call 1m23s — gateway not responding (see the status bar)"
        );
        // A zero/garbage rate never renders (absence over fake zeros).
        assert_eq!(model_call_segment(5, Some(0.0), false), "model call 5s");
        assert_eq!(
            model_call_segment(5, Some(f64::NAN), false),
            "model call 5s"
        );
    }

    #[test]
    fn tool_call_segment_names_the_command_and_escalates_over_time() {
        // The tool twin (live P0, 2026-07-23) + observability wave
        // (2026-07-27): a long tool must render a clock, NAME what is
        // running, and escalate its wording so a human sees "stuck"
        // before the 8-hour deadline that starved the model of signal.
        assert_eq!(tool_call_segment(0, None), "tool call 0s");
        assert_eq!(
            tool_call_segment(45, Some("execute_command: npm test")),
            "tool call 45s · execute_command: npm test"
        );
        // ≥1m: gateway-side note.
        assert!(tool_call_segment(90, None).contains("a large scan can take minutes"));
        // ≥5m: unusual-for-a-shell escalation.
        assert!(tool_call_segment(400, None).contains("long for a shell command"));
        // ≥15m: WARN — the model is blocked, the whole run stalls.
        let stuck = tool_call_segment(1000, Some("execute_command: http.server 8765"));
        assert!(stuck.contains("execute_command: http.server 8765"));
        assert!(stuck.contains("possibly stuck; the model is blocked"));
    }

    #[test]
    fn prefix_fit_drops_whole_items_right_to_left() {
        // POLISH-1: instrument rows drop whole segments, never `…`.
        // widths 10+5+10=25 with seps first=0, rest=5: items 0..2 need
        // 0+10, +5+10=25, +5+10=40.
        let widths = [10usize, 10, 10];
        assert_eq!(prefix_fit(&widths, 0, 5, 40), 3, "all fit exactly");
        assert_eq!(prefix_fit(&widths, 0, 5, 39), 2, "tail drops whole");
        assert_eq!(prefix_fit(&widths, 0, 5, 25), 2);
        assert_eq!(prefix_fit(&widths, 0, 5, 24), 1);
        assert_eq!(prefix_fit(&widths, 0, 5, 10), 1);
        assert_eq!(prefix_fit(&widths, 0, 5, 9), 0, "nothing fits: none");
        // A leading separator counts against the first item too (the
        // header's facts follow the route with a wide `  ·  `).
        assert_eq!(prefix_fit(&widths, 5, 3, 15), 1);
        assert_eq!(prefix_fit(&widths, 5, 3, 14), 0);
        assert_eq!(prefix_fit(&[], 0, 5, 100), 0, "empty never panics");
    }

    #[test]
    fn header_facts_fill_rule() {
        // Full house at rest.
        assert_eq!(
            header_facts(
                "abstractcode-tui",
                "workspace_or_allowed",
                2,
                1,
                true,
                128_000
            ),
            vec![
                "⌂ abstractcode-tui".to_string(),
                "workspace_or_allowed".to_string(),
                "skills 2".to_string(),
                "mcp 1".to_string(),
                "128k tk".to_string(),
            ]
        );
        // Empty mode reads server-managed; zero counts are OMITTED (not
        // "skills 0" noise); tokens only at rest and only when nonzero.
        assert_eq!(
            header_facts("", "", 0, 0, true, 0),
            vec!["server-managed".to_string()]
        );
        // During a run the strip owns live numbers — no token fact.
        assert_eq!(
            header_facts("ws", "", 0, 0, false, 999_999),
            vec!["⌂ ws".to_string(), "server-managed".to_string()]
        );
    }

    #[test]
    fn tail_ellipsis_keeps_the_tail_multibyte_safe() {
        // P2-F: ONE helper for the header (18) + strip (24) session-id
        // cuts. Session ids differ at the END, so the tail survives.
        assert_eq!(tail_ellipsis("acode-abc", 18), "acode-abc");
        assert_eq!(
            tail_ellipsis("acode-0123456789abcdef", 18),
            "…123456789abcdef",
            "over-budget keeps the last max-3 chars behind one ellipsis"
        );
        // Output never exceeds the budget (1 ellipsis + keep).
        let out = tail_ellipsis("acode-0123456789abcdefghij", 18);
        assert!(out.chars().count() <= 18 - 2);
        // Multibyte ids cut on CHAR boundaries — the inline byte-slice
        // predecessor panicked here (adversary finding 3).
        let id = "sésame-ouvre-toi-très-long-identifiant";
        let cut = tail_ellipsis(id, 10);
        assert!(cut.starts_with('…'));
        assert!(id.ends_with(cut.trim_start_matches('…')));
        // Degenerate budgets never panic, never return empty for
        // non-empty input.
        assert_eq!(tail_ellipsis("abcdef", 1), "…f");
        assert_eq!(tail_ellipsis("", 4), "");
    }

    #[test]
    fn route_label_never_renders_a_dangling_pair() {
        // P2-E: a model-only (or provider-only) default route joins the
        // non-empty halves — "( · model)" is a malformed fabrication.
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            // Nothing known: the bare label.
            assert_eq!(route_label(store), "gateway defaults");
            // Model-only capability route: no dangling separator.
            store.default_route.set((String::new(), "qwen3-4b".into()));
            assert_eq!(route_label(store), "gateway defaults (qwen3-4b)");
            // Provider-only route: the provider IS the resolution known.
            store.default_route.set(("lmstudio".into(), String::new()));
            assert_eq!(route_label(store), "gateway defaults (lmstudio)");
            // Full pair keeps the one format.
            store
                .default_route
                .set(("lmstudio".into(), "qwen3-4b".into()));
            assert_eq!(route_label(store), "gateway defaults (lmstudio · qwen3-4b)");
            // A served model (run truth) beats the route's model.
            store
                .fold
                .update(|f| f.stats.effective_model = "qwen3-8b".into());
            assert_eq!(route_label(store), "gateway defaults (lmstudio · qwen3-8b)");
            // Served model with NO provider anywhere: still no dangler.
            store.default_route.set((String::new(), String::new()));
            assert_eq!(route_label(store), "gateway defaults (qwen3-8b)");
            // Explicit overrides bypass the defaults label entirely.
            store.provider.set("openai".into());
            store.model.set("gpt-5.2".into());
            assert_eq!(route_label(store), "openai · gpt-5.2");
        });
        root.dispose();
    }

    #[test]
    fn mem_segment_grades_the_rounded_percent_and_marks_stale() {
        use crate::store::{HostFacts, HostState};
        let with_pct = |p: Option<f64>| HostFacts {
            ram_percent: p,
            ..Default::default()
        };
        // Presence + muted below the warn floor.
        assert_eq!(
            mem_segment(&HostState::Ready(with_pct(Some(62.0)))),
            Some(("mem 62%".into(), 0))
        );
        // Severity grades the ROUNDED number the user reads: 89.5 shows
        // "90%" and must grade as 90 (error), 74.6 shows "75%" → warn.
        assert_eq!(
            mem_segment(&HostState::Ready(with_pct(Some(89.5)))),
            Some(("mem 90%".into(), 2))
        );
        assert_eq!(
            mem_segment(&HostState::Ready(with_pct(Some(74.6)))),
            Some(("mem 75%".into(), 1))
        );
        assert_eq!(
            mem_segment(&HostState::Ready(with_pct(Some(74.4)))),
            Some(("mem 74%".into(), 0))
        );
        // A stale snapshot is MARKED — never indistinguishable from fresh.
        assert_eq!(
            mem_segment(&HostState::Stale(with_pct(Some(62.0)))),
            Some(("mem 62%*".into(), 0))
        );
        // Absence is omission: no percent, or no facts at all.
        assert_eq!(mem_segment(&HostState::Ready(with_pct(None))), None);
        assert_eq!(mem_segment(&HostState::Idle), None);
        assert_eq!(mem_segment(&HostState::Pending), None);
        assert_eq!(mem_segment(&HostState::Unsupported("no".into())), None);
        assert_eq!(mem_segment(&HostState::Error("boom".into())), None);
    }
}
