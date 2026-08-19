//! Entity collaboration headless tests: the REAL interface driven through
//! AbstractTUI's capture harness — focus switching, the corruption cases
//! the plan pins for `wire_feed`, submit routing, chips, and honesty
//! surfaces. No gateway: runner commands land on a plain receiver and the
//! convo folds are driven exactly as posted closures would drive them.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use abstracttui::app::Driver;
use abstracttui::prelude::*;
use abstracttui::testing::CaptureTerm;

use abstractcode_tui::config::Prefs;
use abstractcode_tui::convo::{self, ConvoStatus, EntityConvo, Focus};
use abstractcode_tui::entities::{EntityInfo, VisitOpen};
use abstractcode_tui::runner::Cmd;
use abstractcode_tui::store::{Phase, Store, Workflow};
use abstractcode_tui::transcript::Item;
use abstractcode_tui::ui::{self, UiCtx};

struct Harness {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
    rx: mpsc::Receiver<Cmd>,
}

fn harness() -> Harness {
    abstracttui::app::set_theme_by_id("abstract-dark");
    let size = Size::new(100, 30);
    let mut app = App::new(size);
    let overlays = app.overlays();
    let quitter = app.quitter();
    let (tx, rx) = mpsc::channel::<Cmd>();
    let store_slot: Rc<RefCell<Option<Store>>> = Rc::new(RefCell::new(None));
    let store_out = store_slot.clone();
    let ctx_slot: Rc<RefCell<Option<UiCtx>>> = Rc::new(RefCell::new(None));
    let ctx_out = ctx_slot.clone();
    let actions = app.actions();
    app.mount(move |cx| {
        let store = Store::create(cx);
        *store_out.borrow_mut() = Some(store);
        store.session_id.set("acode-entity-test".into());
        store.workflow.set(Workflow {
            bundle_id: "basic-agent".into(),
            flow_id: "81795ea9".into(),
            name: "basic-agent".into(),
            description: String::new(),
        });
        let ctx = UiCtx {
            tx,
            client: abstractcode_tui::gateway::GatewayClient::new("http://127.0.0.1:1", None),
            overlays: overlays.clone(),
            quitter: quitter.clone(),
            // Path-less Prefs: ephemeral, never touches the real file.
            prefs: Rc::new(RefCell::new(Prefs::default())),
            workspace_root: Some("/tmp/ws".into()),
            max_iterations_explicit: false,
            max_iterations: 50,
            // Hermetic: harness runs never read the repo's AGENTS.md, so
            // editing that file can never move a UI assertion.
            no_project_context: true,
            // Harness default: absent posture = server truth, same as a launch
            // without --no-prompt-cache.
            no_prompt_cache: false,
            replay_turns: 20,
            gateway_label: "127.0.0.1:8080".into(),
            modal: Rc::new(RefCell::new(None)),
            modal_epoch: cx.signal(0u64),
            dismissed_wait: Rc::new(RefCell::new(None)),
            wait_modal_for: Rc::new(RefCell::new(None)),
        };
        *ctx_out.borrow_mut() = Some(ctx.clone());
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
    let store = store_slot.borrow().expect("store created");
    // The UiCtx slot exists to keep the mount closure shape identical to
    // production; these tests drive everything through input + signals.
    let _ = ctx_slot.borrow().clone().expect("ctx created");
    Harness {
        app,
        term,
        driver,
        store,
        rx,
    }
}

impl Harness {
    fn turn(&mut self) -> String {
        self.driver
            .turn(&mut self.app, &mut self.term)
            .expect("turn");
        self.term.screen().to_text()
    }

    fn type_text(&mut self, text: &str) {
        self.term.push_input(text.as_bytes());
    }

    fn press_enter(&mut self) {
        self.term.push_input(b"\r");
    }

    fn drain_cmds(&mut self) -> Vec<Cmd> {
        let mut out = Vec::new();
        while let Ok(c) = self.rx.try_recv() {
            out.push(c);
        }
        out
    }
}

fn roster_entry(slug: &str, state: &str) -> EntityInfo {
    EntityInfo {
        slug: slug.into(),
        name: slug.into(),
        state: state.into(),
        liveness: "alive".into(),
        ..Default::default()
    }
}

/// A parked conversation with rendered items, as the open fold leaves it.
fn parked_convo(name: &str, items: Vec<Item>) -> EntityConvo {
    let mut c = EntityConvo::opening(name, "awake");
    convo::fold_open_success(&mut c, &VisitOpen::default());
    c.run_id = format!("run-{name}");
    c.status = ConvoStatus::Parked;
    c.items = items;
    c
}

// ---------------------------------------------------------------------------
// The wire_feed corruption cases (plan cycle-2, pinned)
// ---------------------------------------------------------------------------

#[test]
fn focus_round_trip_renders_byte_identically() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.fold.update(|f| {
        f.push_item(Item::User {
            text: "agent question".into(),
        });
        f.push_item(Item::Assistant {
            text: "agent answer".into(),
            final_answer: true,
        });
    });
    // Two pumps: markdown blocks discover width on first draw and
    // typeset one frame later (engine contract; see headless_ui.rs).
    h.turn();
    let agent_before = h.turn();
    assert!(
        agent_before.contains("agent answer"),
        "screen:\n{agent_before}"
    );

    store.convos.update(|cs| {
        cs.push(parked_convo(
            "doorcheck",
            vec![
                Item::User {
                    text: "entity question".into(),
                },
                Item::Assistant {
                    text: "entity reply words".into(),
                    final_answer: true,
                },
            ],
        ))
    });
    store.focus.set(Focus::Entity("doorcheck".into()));
    h.turn();
    let entity_view = h.turn();
    assert!(
        entity_view.contains("entity reply words"),
        "entity items render on switch:\n{entity_view}"
    );
    assert!(
        !entity_view.contains("agent answer"),
        "no stale agent cards inside the entity conversation:\n{entity_view}"
    );

    store.focus.set(Focus::Agent);
    h.turn();
    let agent_after = h.turn();
    // Byte-identical BELOW the header: the header legitimately gained the
    // ◆doorcheck chip (a conversation now exists); the transcript pane +
    // strip + composer + status must render exactly as before the trip.
    let below_header = |s: &str| s.lines().skip(1).collect::<Vec<_>>().join("\n");
    assert_eq!(
        below_header(&agent_before),
        below_header(&agent_after),
        "agent→entity→agent round trip is byte-identical below the header"
    );
}

#[test]
fn same_length_same_fingerprint_switch_still_rebuilds() {
    // The plan's cross-contamination case: the entity convo holds items
    // whose (index, fingerprint) pairs EQUAL the agent fold's — without
    // the SyncState focus dimension the fast path would skip everything.
    // The rebuild is observable through the typeset counter.
    let mut h = harness();
    h.turn();
    let store = h.store;
    let same = vec![
        Item::Info {
            text: "identical line".into(),
        },
        Item::Assistant {
            text: "identical answer".into(),
            final_answer: true,
        },
    ];
    store.fold.update(|f| {
        for i in same.clone() {
            f.push_item(i);
        }
    });
    h.turn();
    store
        .convos
        .update(|cs| cs.push(parked_convo("doorcheck", same)));
    h.turn();

    store.focus.set(Focus::Entity("doorcheck".into()));
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("identical answer"),
        "the equal-content conversation still renders:\n{screen}"
    );
    // And the switch did not corrupt the seen bookkeeping: appending to
    // the ENTITY convo now must render (a skipped rebuild would have left
    // the state describing the agent fold).
    store.convos.update(|cs| {
        cs[0].items.push(Item::Assistant {
            text: "post-switch entity growth".into(),
            final_answer: true,
        })
    });
    h.turn();
    let grown = h.turn();
    assert!(grown.contains("post-switch entity growth"), "{grown}");
}

#[test]
fn details_toggle_composes_with_focus_probe_items() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.convos.update(|cs| {
        cs.push(parked_convo(
            "doorcheck",
            vec![
                Item::Info {
                    text: "· 2 memories · 1 diary entry".into(),
                },
                Item::Probe {
                    title: "memories in context (2)".into(),
                    body: "[episode] a prior door check — lived conversation".into(),
                },
                Item::Assistant {
                    text: "reply".into(),
                    final_answer: true,
                },
            ],
        ))
    });
    store.focus.set(Focus::Entity("doorcheck".into()));
    h.turn();
    let with_details = h.turn();
    assert!(
        with_details.contains("memories in context (2)"),
        "probe body visible with details on:\n{with_details}"
    );
    store.show_details.set(false);
    h.turn();
    let without = h.turn();
    assert!(
        !without.contains("memories in context (2)"),
        "probe body folds with details off:\n{without}"
    );
    assert!(
        without.contains("2 memories"),
        "the count chip stays ALWAYS visible:\n{without}"
    );
    store.show_details.set(true);
    h.turn();
    let back = h.turn();
    assert!(back.contains("memories in context (2)"));
}

#[test]
fn rapid_focus_toggling_mid_stream_stays_correct() {
    // The plan's corruption case driven PAST the round trip: the agent
    // fold and the entity convo BOTH mutate while the OTHER conversation
    // holds focus, across rapid toggles — every switch must land on the
    // full current content of the newly focused conversation and never
    // leak the other's cards.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.fold.update(|f| {
        f.push_item(Item::Assistant {
            text: "agent one".into(),
            final_answer: true,
        });
    });
    store
        .convos
        .update(|cs| cs.push(parked_convo("doorcheck", vec![])));
    h.turn();

    for round in 0..3 {
        // Mutate the ENTITY convo while agent-focused (background growth).
        store.focus.set(Focus::Agent);
        store.convos.update(|cs| {
            cs[0].items.push(Item::Assistant {
                text: format!("entity growth {round}"),
                final_answer: true,
            })
        });
        // And the AGENT fold in the same breath.
        store.fold.update(|f| {
            f.push_item(Item::Assistant {
                text: format!("agent growth {round}"),
                final_answer: true,
            })
        });
        h.turn();
        let agent_view = h.turn();
        assert!(
            agent_view.contains(&format!("agent growth {round}")),
            "agent growth renders under agent focus:\n{agent_view}"
        );
        assert!(
            !agent_view.contains("entity growth"),
            "no entity cards leak into the agent view:\n{agent_view}"
        );
        // Switch mid-stream: the entity view must hold ALL its growth.
        store.focus.set(Focus::Entity("doorcheck".into()));
        h.turn();
        let entity_view = h.turn();
        for r in 0..=round {
            assert!(
                entity_view.contains(&format!("entity growth {r}")),
                "entity growth {r} present after toggle round {round}:\n{entity_view}"
            );
        }
        assert!(
            !entity_view.contains("agent growth"),
            "no agent cards leak into the entity view:\n{entity_view}"
        );
    }
}

#[test]
fn details_and_focus_compose_with_agent_images() {
    // details toggle × focus × images: the image placeholder must survive
    // an entity-focus detour with the details flag flipped along the way.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.fold.update(|f| {
        f.push_item(Item::Image {
            run_id: "r".into(),
            artifact_id: "art-9".into(),
            label: "diagram.png".into(),
        });
        f.push_item(Item::Assistant {
            text: "the diagram above".into(),
            final_answer: true,
        });
    });
    h.turn();
    let before = h.turn();
    assert!(before.contains("fetching image…"), "{before}");

    store.convos.update(|cs| {
        cs.push(parked_convo(
            "doorcheck",
            vec![Item::Probe {
                title: "memories in context (1)".into(),
                body: "[episode] a check".into(),
            }],
        ))
    });
    store.focus.set(Focus::Entity("doorcheck".into()));
    h.turn();
    let entity_details_on = h.turn();
    assert!(entity_details_on.contains("memories in context (1)"));
    // Flip details INSIDE entity focus, then come back to the agent.
    store.show_details.set(false);
    h.turn();
    let entity_details_off = h.turn();
    assert!(!entity_details_off.contains("memories in context (1)"));
    store.focus.set(Focus::Agent);
    h.turn();
    let agent_after = h.turn();
    assert!(
        agent_after.contains("fetching image…"),
        "image placeholder survives the focus+details detour:\n{agent_after}"
    );
    assert!(
        agent_after.contains("the diagram above"),
        "agent items intact:\n{agent_after}"
    );
}

#[test]
fn entity_focus_never_shows_the_agent_empty_state() {
    // Boot leaves an Info-only fold (the guidance screen). Entity focus
    // must render the CONVERSATION, never the agent guidance — and the
    // guidance must come back on the agent when focus returns.
    let mut h = harness();
    h.turn();
    let before = h.turn();
    assert!(
        before.contains("describe a task below"),
        "agent guidance on an info-only fold:\n{before}"
    );
    let store = h.store;
    // Built the natural way (never items-wiped): the opening Info line is
    // the ≥1 item every real conversation holds.
    store.convos.update(|cs| {
        let mut c = EntityConvo::opening("doorcheck", "awake");
        convo::fold_open_success(&mut c, &VisitOpen::default());
        c.run_id = "run-doorcheck".into();
        c.status = ConvoStatus::Parked;
        cs.push(c);
    });
    store.focus.set(Focus::Entity("doorcheck".into()));
    h.turn();
    let entity_view = h.turn();
    assert!(
        !entity_view.contains("describe a task below"),
        "the agent guidance would lie inside an entity conversation:\n{entity_view}"
    );
    assert!(
        entity_view.contains("opening a visit with doorcheck"),
        "the conversation's own items render:\n{entity_view}"
    );
    store.focus.set(Focus::Agent);
    h.turn();
    let back = h.turn();
    assert!(back.contains("describe a task below"), "{back}");
}

#[test]
fn chips_overflow_collapses_to_a_count_never_a_fragment() {
    // 100 cols, 3 conversations: whole chips render; the tail collapses
    // to "+N" — never a mangled "◆eph…" fragment (cycle-2 UX review).
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.convos.update(|cs| {
        let mut a = parked_convo("castor", vec![]);
        convo::fold_send_turn(&mut a, "x");
        cs.push(a);
        cs.push(parked_convo("doorcheck", vec![]));
        cs.push(parked_convo("ephemeral", vec![]));
    });
    let screen = h.turn();
    let header = screen.lines().next().unwrap_or("").to_string();
    assert!(
        header.contains("+1") || header.contains("+2"),
        "hidden chips collapse to an honest count:\n{header}"
    );
    assert!(
        !header.contains('…') || !header.contains("eph"),
        "no truncated chip fragment:\n{header}"
    );
}

#[test]
fn focused_chip_always_renders_even_from_the_overflow_tail() {
    // 100 cols, 5 conversations, focus on the FIFTH: identity order would
    // hide it behind "+N" — the focused chip must paint (first), and the
    // "+N" count must still name every unpainted chip (cycle-3 task 3).
    // Alt+E cycle order is untouched by the paint reorder (asserted via
    // cycle_focus below).
    let mut h = harness();
    h.turn();
    let store = h.store;
    let names = ["alpha", "bravo", "charl", "delta", "fifth"];
    store.convos.update(|cs| {
        for n in names {
            cs.push(parked_convo(n, vec![]));
        }
    });
    store.focus.set(Focus::Entity("fifth".into()));
    let screen = h.turn();
    let header = screen.lines().next().unwrap_or("").to_string();
    assert!(
        header.contains("◆fifth"),
        "the focused chip renders even when identity order would hide it:\n{header}"
    );
    // Count what painted; "+N" must cover exactly the rest of the 5.
    let visible = header.matches('◆').count();
    assert!((1..5).contains(&visible), "overflow forced: {header}");
    assert!(
        header.contains(&format!("+{}", 5 - visible)),
        "+N counts every unpainted chip:\n{header}"
    );
    // Paint order is presentation only: Alt+E from "fifth" (last in the
    // convos vec) cycles back to the AGENT, exactly as identity order says.
    abstractcode_tui::ui::entity_actions::cycle_focus(store);
    assert_eq!(store.focus.get_untracked(), Focus::Agent);
    // And from agent it enters at convo 1 ("alpha"), never the painted-first
    // chip.
    abstractcode_tui::ui::entity_actions::cycle_focus(store);
    assert_eq!(store.focus.get_untracked(), Focus::Entity("alpha".into()));
}

#[test]
fn strip_held_marker_renders_only_while_a_hold_can_exist() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    let mut c = parked_convo("doorcheck", vec![]);
    convo::fold_send_turn(&mut c, "x");
    convo::hold_draft(&mut c, "queued words");
    store.convos.update(|cs| cs.push(c));
    store.focus.set(Focus::Entity("doorcheck".into()));
    let running = h.turn();
    assert!(
        running.contains("draft held (sends when the turn parks)"),
        "marker present while the turn runs:\n{running}"
    );
    // Parked with a residual hold (transient between fold and dispatch):
    // the marker must NOT promise a send tied to a turn that already
    // parked (cycle-2 UX review: "parked — … · draft held" contradiction).
    store.convos.update(|cs| {
        cs[0].status = ConvoStatus::Parked;
        cs[0].turn_started = None;
    });
    let parked = h.turn();
    assert!(
        !parked.contains("draft held"),
        "no contradictory marker beside the parked status:\n{parked}"
    );
}

// ---------------------------------------------------------------------------
// Submit routing + chips + honesty surfaces
// ---------------------------------------------------------------------------

#[test]
fn at_name_submit_opens_the_conversation_and_sends_open_cmd() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store
        .entities
        .set(vec![roster_entry("doorcheck", "asleep")]);
    h.turn();
    h.type_text("@doorcheck");
    h.turn();
    h.press_enter();
    h.turn();
    let screen = h.turn();
    // The conversation opened in Opening state with the wake honesty note
    // (the cached roster said asleep).
    assert!(
        screen.contains("opening a visit with doorcheck"),
        "opening line renders:\n{screen}"
    );
    assert!(
        screen.contains("was asleep — this visit wakes"),
        "the B1 wake honesty note renders:\n{screen}"
    );
    let cmds = h.drain_cmds();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Cmd::EntityOpen { name } if name == "doorcheck")),
        "EntityOpen command sent"
    );
    assert_eq!(
        store.focus.get_untracked(),
        Focus::Entity("doorcheck".into())
    );
    // Chips row appears in the header once a conversation exists.
    let screen = h.turn();
    assert!(
        screen.contains("◆doorcheck"),
        "header chip renders:\n{screen}"
    );
}

#[test]
fn unknown_mention_notifies_and_preserves_the_draft() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.entities.set(vec![roster_entry("doorcheck", "awake")]);
    h.turn();
    h.type_text("@ghost hello there");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("@ghost hello there"),
        "draft preserved in the composer:\n{screen}"
    );
    assert!(
        store.convos.with_untracked(|cs| cs.is_empty()),
        "no conversation opened for an unknown name"
    );
    let cmds = h.drain_cmds();
    assert!(
        !cmds.iter().any(|c| matches!(c, Cmd::Start { .. })),
        "an unknown @name must never become an agent prompt"
    );
}

#[test]
fn entity_focus_send_and_hold_routing() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.entities.set(vec![roster_entry("doorcheck", "awake")]);
    store
        .convos
        .update(|cs| cs.push(parked_convo("doorcheck", vec![])));
    store.focus.set(Focus::Entity("doorcheck".into()));
    h.turn();

    // Parked → Enter sends a turn (EntityTurn with the bumped epoch).
    h.type_text("hello door");
    h.turn();
    h.press_enter();
    h.turn();
    let cmds = h.drain_cmds();
    let sent = cmds.iter().find_map(|c| match c {
        Cmd::EntityTurn {
            name,
            run_id,
            epoch,
            text,
        } => Some((name.clone(), run_id.clone(), *epoch, text.clone())),
        _ => None,
    });
    let (name, run_id, epoch, text) = sent.expect("EntityTurn sent");
    assert_eq!(name, "doorcheck");
    assert_eq!(run_id, "run-doorcheck");
    assert_eq!(text, "hello door");
    assert_eq!(
        store.convos.with_untracked(|cs| cs[0].turn_epoch),
        epoch,
        "the command carries the convo's current epoch"
    );
    assert_eq!(
        store.convos.with_untracked(|cs| cs[0].status),
        ConvoStatus::TurnRunning
    );

    // TurnRunning → Enter HOLDS the draft (never a second send).
    h.type_text("steer between turns");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        store
            .convos
            .with_untracked(|cs| cs[0].held_draft == "steer between turns"),
        "draft held while the turn runs"
    );
    assert!(
        h.drain_cmds()
            .iter()
            .all(|c| !matches!(c, Cmd::EntityTurn { .. })),
        "no second EntityTurn while one runs"
    );
    // The strip names the held draft.
    assert!(
        screen.contains("draft held"),
        "held-draft marker on the strip:\n{screen}"
    );
}

#[test]
fn escape_during_entity_turn_says_non_interruptible_never_cancels() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    let mut c = parked_convo("doorcheck", vec![]);
    convo::fold_send_turn(&mut c, "x");
    store.convos.update(|cs| cs.push(c));
    store.focus.set(Focus::Entity("doorcheck".into()));
    // An AGENT run is also live: Esc-Esc must NOT arm its cancel while
    // the user is looking at the entity.
    store.phase.set(Phase::Running);
    store.run_id.set("agent-run".into());
    h.turn();
    h.term.push_input(b"\x1b"); // Esc
    h.turn();
    h.term.push_input(b"\x1b"); // Esc again (the agent double-cancel arm)
    h.turn();
    let notices = store.notices.get_untracked();
    assert!(
        notices.iter().any(|n| n.contains("non-interruptible")),
        "honest notice shown: {notices:?}"
    );
    let cmds = h.drain_cmds();
    assert!(
        !cmds.iter().any(|c| matches!(c, Cmd::Cancel { .. })),
        "no cancel command from Esc-Esc under entity focus"
    );
}

#[test]
fn slash_end_refuses_mid_turn_and_closes_when_parked() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    let mut c = parked_convo("doorcheck", vec![]);
    convo::fold_send_turn(&mut c, "x");
    store.convos.update(|cs| cs.push(c));
    store.focus.set(Focus::Entity("doorcheck".into()));
    h.turn();
    h.type_text("/end");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        store
            .notices
            .get_untracked()
            .iter()
            .any(|n| n.contains("turn in flight")),
        "mid-turn /end refused client-side"
    );
    assert!(!h
        .drain_cmds()
        .iter()
        .any(|c| matches!(c, Cmd::EntityClose { .. })));

    // Parked → /end sends the close with the convo's epoch.
    store.convos.update(|cs| {
        cs[0].status = ConvoStatus::Parked;
        cs[0].turn_started = None;
    });
    h.turn();
    h.type_text("/end doorcheck thanks for the check");
    h.turn();
    h.press_enter();
    h.turn();
    let close = h.drain_cmds().into_iter().find_map(|c| match c {
        Cmd::EntityClose {
            name,
            run_id,
            reason,
            ..
        } => Some((name, run_id, reason)),
        _ => None,
    });
    let (name, run_id, reason) = close.expect("EntityClose sent");
    assert_eq!(name, "doorcheck");
    assert_eq!(run_id, "run-doorcheck");
    assert_eq!(reason, "thanks for the check");
}

#[test]
fn agent_wait_owns_the_strip_in_entity_focus_with_prefix() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store
        .convos
        .update(|cs| cs.push(parked_convo("doorcheck", vec![])));
    store.focus.set(Focus::Entity("doorcheck".into()));
    store.phase.set(Phase::Running);
    store.fold.update(|f| {
        f.begin_run("root");
        let _ = f.apply(
            "root",
            &serde_json::json!({"run_id": "root", "status": "waiting", "step_id": "s1",
                "result": {"wait": {"reason": "user", "wait_key": "tool_approval:1",
                    "details": {"mode": "approval_required",
                                 "tool_calls": [{"name": "write_file", "call_id": "c1"}]}}}}),
        );
    });
    let screen = h.turn();
    assert!(
        screen.contains("agent: approval needed"),
        "a pending AGENT wait keeps owning the strip in entity focus, prefixed:\n{screen}"
    );
}

#[test]
fn composer_placeholder_swaps_in_entity_focus_and_footer_stays_facts() {
    // REST-1 moved the key legend behind `?`: the footer is the
    // always-visible instrument row in EVERY focus, and the focus-aware
    // teaching lives in the composer placeholder — which HDR-2c made
    // actually visible while focused (the engine only paints its own
    // placeholder unfocused, and the composer autofocuses).
    let mut h = harness();
    h.turn();
    let store = h.store;
    let agent_screen = h.turn();
    assert!(
        agent_screen.contains("? keys"),
        "footer points at the legend home:\n{agent_screen}"
    );
    assert!(
        agent_screen.contains("describe a task"),
        "agent placeholder renders while focused:\n{agent_screen}"
    );
    store
        .convos
        .update(|cs| cs.push(parked_convo("doorcheck", vec![])));
    store.focus.set(Focus::Entity("doorcheck".into()));
    let entity_screen = h.turn();
    assert!(
        entity_screen.contains("message doorcheck"),
        "entity placeholder renders while focused:\n{entity_screen}"
    );
    assert!(
        entity_screen.contains("? keys"),
        "the instrument row stays in entity focus:\n{entity_screen}"
    );
    assert_eq!(
        ui::entity_actions::entity_placeholder("doorcheck"),
        "message doorcheck — non-interruptible mid-turn; Enter holds during a turn · /help"
    );
}

/// Alt+E (ESC-prefixed 'e' on the legacy wire) cycles conversation
/// focus. It was Ctrl+E until the abstracttui 0.3.2 bump handed the text
/// widgets Codex's editor keymap, where Ctrl+E is move-to-line-end: a
/// focused editor consumes its chords before any shortcut sees them, so
/// the old binding was DEAD while the composer held focus (which is
/// nearly always).
#[test]
fn alt_e_cycles_focus_and_new_session_resets_to_agent() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.convos.update(|cs| {
        cs.push(parked_convo("castor", vec![]));
        cs.push(parked_convo("doorcheck", vec![]));
    });
    h.turn();
    h.term.push_input(b"\x1be"); // Alt+E
    h.turn();
    assert_eq!(store.focus.get_untracked(), Focus::Entity("castor".into()));
    h.term.push_input(b"\x1be");
    h.turn();
    assert_eq!(
        store.focus.get_untracked(),
        Focus::Entity("doorcheck".into())
    );
    h.term.push_input(b"\x1be");
    h.turn();
    assert_eq!(store.focus.get_untracked(), Focus::Agent);

    // /new returns focus to the agent and keeps convos (server-side).
    store.focus.set(Focus::Entity("castor".into()));
    h.turn();
    h.type_text("/new");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(store.focus.get_untracked(), Focus::Agent);
    assert_eq!(
        store.convos.with_untracked(|cs| cs.len()),
        2,
        "entity convos survive /new (they are server-side visits)"
    );
}

#[test]
fn task_command_validates_and_sends() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store
        .entities
        .set(vec![roster_entry("doorcheck", "asleep")]);
    h.turn();
    h.type_text("/task doorcheck check the west door");
    h.turn();
    h.press_enter();
    h.turn();
    let task = h.drain_cmds().into_iter().find_map(|c| match c {
        Cmd::EntityTask { name, title } => Some((name, title)),
        _ => None,
    });
    let (name, title) = task.expect("EntityTask sent");
    assert_eq!(name, "doorcheck");
    assert_eq!(title, "check the west door");

    // Missing title → usage notice, no command.
    h.type_text("/task doorcheck");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(h.drain_cmds().is_empty());
    assert!(store
        .notices
        .get_untracked()
        .iter()
        .any(|n| n.contains("usage: /task")));
}

#[test]
fn entities_modal_opens_instantly_on_cache_with_as_of_label() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.entities.set(vec![
        roster_entry("castor", "asleep"),
        roster_entry("doorcheck", "awake"),
        EntityInfo {
            slug: "lost-home".into(),
            error: "home unreadable: manifest.json missing".into(),
            ..Default::default()
        },
    ]);
    store.entities_as_of.set("12:30".into());
    store.entities_loading.set(true);
    h.turn();
    h.type_text("/entities");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("as of 12:30 UTC"),
        "cached-roster freshness label:\n{screen}"
    );
    assert!(
        screen.contains("refreshing") && screen.contains("roster can be slow"),
        "the hang honesty note renders while loading:\n{screen}"
    );
    assert!(screen.contains("castor"), "{screen}");
    assert!(
        screen.contains("broken home: home unreadable"),
        "error rows render labeled:\n{screen}"
    );
    assert!(
        screen.contains("[t] leave a task"),
        "footer actions:\n{screen}"
    );
    // The /entities dispatch kicked an async refresh.
    assert!(h
        .drain_cmds()
        .iter()
        .any(|c| matches!(c, Cmd::LoadEntities)));
}

// ---------------------------------------------------------------------------
// The FLOW-BRAIN lane (c5190/c5280 proof — summon-per-prompt)
// ---------------------------------------------------------------------------

#[test]
fn flow_brain_open_send_reply_and_local_end() {
    let mut h = harness();
    h.store
        .entities
        .set(vec![roster_entry("doorcheck", "awake")]);
    h.turn();

    // /brain <name> opens a FLOW conversation: Ready immediately (no
    // server open), focused, with the lane's teaching line.
    h.type_text("/brain doorcheck");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert_eq!(
        h.store.focus.get_untracked(),
        Focus::Entity("doorcheck".into())
    );
    let (brain, status, sid) = h.store.convos.with_untracked(|cs| {
        let ix = convo::find(cs, "doorcheck").expect("convo exists");
        (cs[ix].brain, cs[ix].status, cs[ix].session_id.clone())
    });
    assert_eq!(brain, convo::Brain::Flow);
    assert_eq!(status, ConvoStatus::Ready, "no server open — ready now");
    assert!(
        sid.starts_with("tui-flow-"),
        "client-minted session id: {sid}"
    );
    assert!(
        screen.contains("each message is one door summon"),
        "the lane teaches its semantics:\n{screen}"
    );
    assert!(
        !h.drain_cmds()
            .iter()
            .any(|c| matches!(c, Cmd::EntityOpen { .. })),
        "flow open never issues a visit open"
    );

    // Typing sends ONE summon command carrying the session id + epoch.
    h.type_text("my favorite color is teal");
    h.turn();
    h.press_enter();
    h.turn();
    let cmds = h.drain_cmds();
    let (sent_sid, sent_epoch) = cmds
        .iter()
        .find_map(|c| match c {
            Cmd::EntityFlowTurn {
                name,
                session_id,
                epoch,
                text,
            } if name == "doorcheck" && text.contains("teal") => Some((session_id.clone(), *epoch)),
            _ => None,
        })
        .expect("EntityFlowTurn dispatched");
    assert_eq!(sent_sid, sid, "every summon groups under the convo session");
    assert!(
        h.store
            .convos
            .with_untracked(|cs| cs[convo::find(cs, "doorcheck").unwrap()].status)
            == ConvoStatus::TurnRunning
    );

    // The turn thread's reply fold: answer + degraded warn + Ready.
    h.store.convos.update(|cs| {
        let ix =
            convo::guard_flow(cs, "doorcheck", &sid, sent_epoch).expect("guard admits live epoch");
        let held = convo::fold_flow_reply(&mut cs[ix], "run-abc123", "Teal — noted.", 1, "");
        assert!(held.is_none());
    });
    let screen = h.turn();
    assert!(screen.contains("Teal — noted."), "reply renders:\n{screen}");
    assert!(
        screen.contains("degraded turn"),
        "structured degraded contract renders as a warn line:\n{screen}"
    );
    let (status, turn_n, run_id) = h.store.convos.with_untracked(|cs| {
        let ix = convo::find(cs, "doorcheck").unwrap();
        (cs[ix].status, cs[ix].turn_n, cs[ix].run_id.clone())
    });
    assert_eq!(status, ConvoStatus::Ready, "flow convos never park");
    assert_eq!(turn_n, 1);
    assert_eq!(run_id, "run-abc123", "latest summon run recorded");

    // /end closes LOCALLY: no EntityClose command, epoch bumped, honest note.
    h.type_text("/end");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        !h.drain_cmds()
            .iter()
            .any(|c| matches!(c, Cmd::EntityClose { .. })),
        "flow end never issues a server close"
    );
    assert!(
        screen.contains("memory of it persists"),
        "the local-end note is honest about what persists:\n{screen}"
    );
    let (status, epoch_after) = h.store.convos.with_untracked(|cs| {
        let ix = convo::find(cs, "doorcheck").unwrap();
        (cs[ix].status, cs[ix].turn_epoch)
    });
    assert_eq!(status, ConvoStatus::Closed);
    assert!(epoch_after > sent_epoch, "end invalidates in-flight posts");
    // A stale reply from the pre-end epoch applies NOTHING.
    h.store.convos.update(|cs| {
        assert!(
            convo::guard_flow(cs, "doorcheck", &sid, sent_epoch).is_none(),
            "stale epoch guarded out"
        );
    });
}

#[test]
fn flow_convo_never_chimeras_and_reopen_flips_brain() {
    let mut h = harness();
    h.store
        .entities
        .set(vec![roster_entry("doorcheck", "awake")]);
    h.turn();
    h.type_text("/brain doorcheck");
    h.turn();
    h.press_enter();
    h.turn();
    let _ = h.drain_cmds();

    // /brain again on a LIVE flow convo: focused notice, never a replace.
    h.type_text("/brain doorcheck");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(h
        .store
        .notices
        .get_untracked()
        .iter()
        .any(|n| n.contains("already conversing with doorcheck through the flow brain")));

    // @name on the LIVE flow convo: focuses, never opens a visit over it
    // (the reference implementation's P0 chimera class).
    h.store.focus.set(Focus::Agent);
    h.turn();
    h.type_text("@doorcheck");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(
        h.store.focus.get_untracked(),
        Focus::Entity("doorcheck".into())
    );
    assert!(
        !h.drain_cmds()
            .iter()
            .any(|c| matches!(c, Cmd::EntityOpen { .. })),
        "@name focuses the live flow convo without opening a visit"
    );
    assert_eq!(
        h.store
            .convos
            .with_untracked(|cs| cs[convo::find(cs, "doorcheck").unwrap()].brain),
        convo::Brain::Flow,
        "the live convo keeps its brain"
    );

    // After /end, @name REOPENS as a visit — and the brain flips with the
    // transport (fold_reopen's chimera guard).
    h.type_text("/end");
    h.turn();
    h.press_enter();
    h.turn();
    h.type_text("@doorcheck");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        h.drain_cmds()
            .iter()
            .any(|c| matches!(c, Cmd::EntityOpen { .. })),
        "reopen after end goes through the visit door"
    );
    assert_eq!(
        h.store
            .convos
            .with_untracked(|cs| cs[convo::find(cs, "doorcheck").unwrap()].brain),
        convo::Brain::Visit,
        "the reopened conversation's brain matches its transport"
    );
}

#[test]
fn flow_failure_keeps_the_conversation_usable() {
    let mut h = harness();
    h.store
        .entities
        .set(vec![roster_entry("doorcheck", "awake")]);
    h.turn();
    h.type_text("/brain doorcheck");
    h.turn();
    h.press_enter();
    h.turn();
    h.type_text("hello");
    h.turn();
    h.press_enter();
    h.turn();
    let (sid, epoch) = h.store.convos.with_untracked(|cs| {
        let ix = convo::find(cs, "doorcheck").unwrap();
        (cs[ix].session_id.clone(), cs[ix].turn_epoch)
    });
    // The thread posts a failure: the convo returns to Ready (no server
    // thread was lost — the next message just summons again).
    h.store.convos.update(|cs| {
        let ix = convo::guard_flow(cs, "doorcheck", &sid, epoch).unwrap();
        convo::fold_flow_failure(&mut cs[ix], "summon refused: gateway HTTP 503");
    });
    let screen = h.turn();
    assert!(screen.contains("summon refused"), "{screen}");
    assert_eq!(
        h.store
            .convos
            .with_untracked(|cs| cs[convo::find(cs, "doorcheck").unwrap()].status),
        ConvoStatus::Ready,
        "a failed summon leaves the conversation usable"
    );
}

#[test]
fn flow_replace_inherits_the_epoch_so_stale_posts_stay_dead() {
    // The end→reopen→send race: a thread from the OLD conversation is
    // still polling (send bumped its epoch to 1); /end bumps to 2; /brain
    // replaces the closed record; the NEW conversation's first send would
    // reach epoch 1 again if the record restarted at 0 — and the old
    // thread's late post would fold a stale reply into the new thread.
    // Epoch inheritance keeps the stale post guarded out forever.
    let mut h = harness();
    h.store
        .entities
        .set(vec![roster_entry("doorcheck", "awake")]);
    h.turn();
    h.type_text("/brain doorcheck");
    h.turn();
    h.press_enter();
    h.turn();
    h.type_text("first message");
    h.turn();
    h.press_enter();
    h.turn(); // in-flight at epoch 1
    let (stale_sid, stale_epoch) = h.store.convos.with_untracked(|cs| {
        let ix = convo::find(cs, "doorcheck").unwrap();
        (cs[ix].session_id.clone(), cs[ix].turn_epoch)
    });
    assert_eq!(stale_epoch, 1);
    h.type_text("/end");
    h.turn();
    h.press_enter();
    h.turn();
    h.type_text("/brain doorcheck");
    h.turn();
    h.press_enter();
    h.turn();
    h.type_text("second thread first message");
    h.turn();
    h.press_enter();
    h.turn();
    // The stale epoch from the OLD thread must match NOTHING now.
    h.store.convos.update(|cs| {
        assert!(
            convo::guard_flow(cs, "doorcheck", &stale_sid, stale_epoch).is_none(),
            "epoch inheritance + the sid guard keep the old thread's posts dead"
        );
    });
    let _ = h.drain_cmds();
}

#[test]
fn flow_reply_edge_shapes_render_honestly() {
    // moment_error + the empty-clean-answer line (adversary test asks) —
    // and the poller skips flow convos entirely.
    let mut h = harness();
    h.store
        .entities
        .set(vec![roster_entry("doorcheck", "awake")]);
    h.turn();
    h.type_text("/brain doorcheck");
    h.turn();
    h.press_enter();
    h.turn();
    h.type_text("hello");
    h.turn();
    h.press_enter();
    h.turn();
    let (sid, epoch) = h.store.convos.with_untracked(|cs| {
        let ix = convo::find(cs, "doorcheck").unwrap();
        (cs[ix].session_id.clone(), cs[ix].turn_epoch)
    });
    // While the turn runs, the visit poller view must NOT include the
    // flow convo (its run_id would be a completed run, not a visit).
    abstractcode_tui::gateway::entities::sync_poller_view(
        &h.store.convos.with_untracked(|cs| cs.clone()),
    );
    let view = abstractcode_tui::gateway::entities::poller_view();
    assert!(
        view.lock().unwrap().open.is_empty(),
        "flow convos never enter the visit poller"
    );
    // moment_error renders as its own warn line.
    h.store.convos.update(|cs| {
        let ix = convo::guard_flow(cs, "doorcheck", &sid, epoch).unwrap();
        let _ = convo::fold_flow_reply(&mut cs[ix], "r1", "partial words", 0, "engine hiccup");
    });
    let screen = h.turn();
    assert!(
        screen.contains("moment error: engine hiccup"),
        "structured moment_error renders:\n{screen}"
    );
    // Empty answer with a CLEAN contract still says something.
    h.type_text("hello again");
    h.turn();
    h.press_enter();
    h.turn();
    let (sid2, epoch2) = h.store.convos.with_untracked(|cs| {
        let ix = convo::find(cs, "doorcheck").unwrap();
        (cs[ix].session_id.clone(), cs[ix].turn_epoch)
    });
    h.store.convos.update(|cs| {
        let ix = convo::guard_flow(cs, "doorcheck", &sid2, epoch2).unwrap();
        let _ = convo::fold_flow_reply(&mut cs[ix], "r2", "", 0, "");
    });
    let screen = h.turn();
    assert!(
        screen.contains("completed without words"),
        "silence is a stated fact, never a hang:\n{screen}"
    );
}

#[test]
fn summon_output_parse_reads_the_structured_contract() {
    use abstractcode_tui::gateway::entities::parse_summon_output;
    let v = serde_json::json!({
        "status": "completed",
        "output": {"answer": "hi", "degraded": 2, "moment_error": "x"}
    });
    assert_eq!(
        parse_summon_output(&v),
        ("hi".to_string(), 2, "x".to_string())
    );
    // Missing fields fail to honest defaults, never panic.
    assert_eq!(
        parse_summon_output(&serde_json::json!({"status": "completed"})),
        (String::new(), 0, String::new())
    );
    // Non-object output likewise.
    assert_eq!(
        parse_summon_output(&serde_json::json!({"output": "weird"})),
        (String::new(), 0, String::new())
    );
}
