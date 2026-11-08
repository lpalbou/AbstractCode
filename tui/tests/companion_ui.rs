//! Host-level companion integration with ephemeral preferences and no worker/network.
use abstractcode::{
    config::Prefs,
    docs_chat::ChatScope,
    runner::Cmd,
    store::{Conn, Store, Workflow},
    transcript::{Item, ToolStatus},
    ui::{self, UiCtx},
};
use abstracttui::{app::Driver, prelude::*, testing::CaptureTerm};
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc, sync::mpsc};

struct Harness {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
    ctx: UiCtx,
    rx: mpsc::Receiver<Cmd>,
}
fn harness(width: i32, companion: Option<Value>) -> Harness {
    abstracttui::app::set_theme_by_id("abstract-dark");
    let size = Size::new(width, 40);
    let mut app = App::new(size);
    let overlays = app.overlays();
    let quitter = app.quitter();
    let actions = app.actions();
    let (tx, rx) = mpsc::channel();
    let slot = Rc::new(RefCell::new(None));
    let out = slot.clone();
    app.mount(move |cx| {
        let store = Store::create(cx);
        store.session_id.set("companion-fixture".into());
        store.conn.set(Conn::Ok);
        store.fold.update(|f| {
            f.begin_run("main-root");
            f.push_item(Item::User {
                text: "Existing user request".into(),
            });
            f.push_item(Item::Assistant {
                text: "Existing assistant reply".into(),
                final_answer: true,
                author: None,
            });
        });
        store.workflow.set(Workflow {
            bundle_id: "basic-agent".into(),
            flow_id: "basic".into(),
            name: "Basic agent".into(),
            ..Default::default()
        });
        let prefs = Prefs {
            companion,
            ..Default::default()
        };
        let ctx = UiCtx {
            tx,
            client: abstractcode::gateway::GatewayClient::new("http://127.0.0.1:1", None),
            overlays: overlays.clone(),
            quitter: quitter.clone(),
            prefs: Rc::new(RefCell::new(prefs)),
            workspace_root: None,
            max_iterations_explicit: false,
            max_iterations: 50,
            no_project_context: true,
            no_prompt_cache: false,
            replay_turns: 20,
            gateway_label: "fixture".into(),
            modal: Rc::new(RefCell::new(None)),
            modal_epoch: cx.signal(0),
            dismissed_wait: Rc::new(RefCell::new(None)),
            wait_modal_for: Rc::new(RefCell::new(None)),
        };
        *out.borrow_mut() = Some((store, ctx.clone(), cx));
        ui::root(cx, store, ctx, &actions)
    })
    .unwrap();
    let mut term = CaptureTerm::new(size);
    let driver = Driver::new(
        &mut app,
        &mut term,
        RunConfig {
            probe: false,
            caps: Some(abstracttui::term::Capabilities::with(|c| {
                c.truecolor = true;
                c.colors_256 = true;
                c.unicode_ok = true;
            })),
            ..Default::default()
        },
    )
    .unwrap();
    let (store, ctx, _scope) = slot.borrow().clone().unwrap();
    Harness {
        app,
        term,
        driver,
        store,
        ctx,
        rx,
    }
}
impl Harness {
    fn turn(&mut self) -> String {
        self.driver.turn(&mut self.app, &mut self.term).unwrap();
        self.term.screen().to_text()
    }
    fn click(&mut self, label: &str) {
        let screen = self.turn();
        let (y, line) = screen
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(label))
            .unwrap_or_else(|| panic!("{label} missing:\n{screen}"));
        let byte = line.find(label).unwrap();
        let x = line[..byte].chars().count();
        self.term
            .push_input(format!("\x1b[<0;{};{}M", x + 1, y + 1).as_bytes());
        self.turn();
        self.term
            .push_input(format!("\x1b[<0;{};{}m", x + 1, y + 1).as_bytes());
        self.turn();
    }
    fn enter(&mut self) {
        self.term.push_input(b"\r");
        self.turn();
    }
    fn commands(&self) -> Vec<Cmd> {
        self.rx.try_iter().collect()
    }
}
fn advice_count(commands: &[Cmd]) -> usize {
    commands
        .iter()
        .filter(|c| {
            matches!(
                c,
                Cmd::AssistantAsk {
                    scope: ChatScope::Advice,
                    ..
                }
            )
        })
        .count()
}
fn tool() -> Item {
    Item::Tool {
        key: "tool-row".into(),
        name: "read_file".into(),
        args_preview: "file".into(),
        args_full: "never-in-advice".into(),
        status: ToolStatus::Ok,
        result: "large-payload-excluded".into(),
        error: String::new(),
        sandbox: None,
    }
}

#[test]
fn every_style_offers_a_ready_companion_and_open_the_run_without_sending_the_coding_draft() {
    for style in ["halo", "hover", "vortex", "eclipse", "eye", "rover"] {
        let mut h = harness(
            220,
            Some(json!({"enabled":true,"style":style,"animate":false})),
        );
        let screen = h.turn();
        assert!(
            screen.contains("Ready") && screen.contains("Talk / ask"),
            "{style}:\n{screen}"
        );
        h.term.push_input(b"My coding draft");
        h.turn();
        h.click(if style == "halo" {
            "Ready"
        } else {
            "Talk / ask"
        });
        let screen = h.turn();
        assert!(
            screen.contains("Companion · AbstractCode") && screen.contains("My coding draft"),
            "{screen}"
        );
        assert!(
            !h.commands().iter().any(|c| matches!(
                c,
                Cmd::Start { .. }
                    | Cmd::Steer { .. }
                    | Cmd::Resume { .. }
                    | Cmd::AssistantAsk { .. }
            )),
            "opening is presentation only"
        );
    }
}
#[test]
fn narrow_companion_remains_reachable_and_yields_to_the_assistant_drawer() {
    let mut h = harness(90, Some(json!({"enabled":true,"animate":false})));
    let screen = h.turn();
    assert!(
        screen.contains('◉') && !screen.contains("Talk / ask"),
        "{screen}"
    );
    h.click("◉");
    let screen = h.turn();
    assert!(
        screen.contains("Companion · AbstractCode")
            && !screen.contains("Documentation · AbstractCode"),
        "{screen}"
    );
    assert!(
        !screen.contains('◉'),
        "duplicate compact avatar should be hidden in drawer:\n{screen}"
    );
}
#[test]
fn companion_settings_are_reachable_and_change_only_ephemeral_preferences() {
    let mut h = harness(220, Some(json!({"enabled":true,"animate":false})));
    h.turn();
    h.click("⚙");
    let screen = h.turn();
    for label in [
        "Enable companion",
        "Appearance",
        "Animate activity",
        "Offer comments",
        "Read side replies",
        "Speaking voice",
        "Position",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    assert!(h.ctx.modal_open());
    assert!(!screen.contains("Talk / ask"), "modal owns interaction");
    h.click("Appearance");
    h.enter();
    assert!(h.turn().contains("Preview companion appearance"));
    h.click("Hover robot");
    h.click("Apply avatar");
    assert_eq!(
        h.ctx.prefs.borrow().companion.as_ref().unwrap()["style"],
        "hover"
    );
    assert!(!h.ctx.modal_open());
    h.click("⚙");
    h.click("Offer comments");
    h.enter();
    assert_eq!(
        h.ctx.prefs.borrow().companion.as_ref().unwrap()["advice"],
        true
    );
    assert!(h.ctx.prefs.borrow().path.is_none());
    assert_eq!(
        advice_count(&h.commands()),
        0,
        "editing settings is not an inference trigger"
    );
}

#[test]
fn position_changes_move_the_same_avatar_and_survive_reopening_settings() {
    let mut h = harness(220, Some(json!({"enabled":true,"animate":false})));
    let top = h
        .turn()
        .lines()
        .position(|s| s.contains("Talk / ask"))
        .unwrap();
    h.click("⚙");
    h.click("Position");
    h.enter();
    assert_eq!(
        h.ctx.prefs.borrow().companion.as_ref().unwrap()["position"],
        "bottom-right"
    );
    assert!(h.turn().contains("Bottom right"));
    h.term.push_input(b"\x1b");
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while h.ctx.modal_open() && std::time::Instant::now() < deadline {
        h.turn();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let screen = h.turn();
    let bottom = screen
        .lines()
        .position(|s| s.contains("Talk / ask"))
        .unwrap();
    let composer = screen
        .lines()
        .position(|s| s.contains("describe a task"))
        .unwrap();
    assert!(bottom > top + 5 && bottom < composer, "{screen}");
    h.click("⚙");
    assert!(h.turn().contains("Bottom right"));
    assert!(!h
        .commands()
        .iter()
        .any(|c| matches!(c, Cmd::AssistantAsk { .. } | Cmd::Start { .. })));
}

#[test]
fn account_companion_entry_is_available_with_the_avatar_disabled() {
    for width in [220, 90] {
        let mut h = harness(width, None);
        h.turn();
        h.term.push_input(b"Unsent coding draft");
        h.turn();
        h.click(if width == 220 { "●" } else { "Menu" });
        let screen = h.turn();
        assert!(screen.contains("Companion"), "{screen}");
        h.click("Companion");
        let screen = h.turn();
        assert!(screen.contains("Companion · AbstractCode"), "{screen}");
        assert_eq!(h.store.help_scope.get_untracked(), ChatScope::Run);
        h.term.push_input(b"\x1b");
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while h.store.docs_open.get_untracked() && std::time::Instant::now() < deadline {
            h.turn();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let restored = h.turn();
        assert!(restored.contains("Unsent coding draft"), "{restored}");
        assert!(!h
            .commands()
            .iter()
            .any(|c| matches!(c, Cmd::AssistantAsk { .. } | Cmd::Start { .. })));
    }
}

#[test]
fn appearance_preview_holds_every_phase_and_cancel_does_not_save_or_send_commands() {
    let mut h = harness(
        220,
        Some(json!({"enabled":true,"style":"halo","animate":true})),
    );
    h.turn();
    h.commands();
    h.click("⚙");
    h.click("Appearance");
    h.enter();
    h.click("Hover robot");
    for label in [
        "Ready",
        "Listening",
        "Thinking",
        "Working",
        "Speaking",
        "Waiting",
        "Error",
    ] {
        // Button and current-phase label share a name. Numeric shortcuts also
        // provide a keyboard-only path that cannot accidentally hit the label.
        let key = match label {
            "Ready" => b'1',
            "Listening" => b'2',
            "Thinking" => b'3',
            "Working" => b'4',
            "Speaking" => b'5',
            "Waiting" => b'6',
            _ => b'7',
        };
        h.term.push_input(&[key]);
        let screen = h.turn();
        assert!(screen.contains(&format!("{label} · held")), "{screen}");
    }
    assert_eq!(
        h.ctx.prefs.borrow().companion.as_ref().unwrap()["style"],
        "halo"
    );
    h.click("Cancel");
    assert!(h.turn().contains("Companion · Enter"));
    assert_eq!(
        h.ctx.prefs.borrow().companion.as_ref().unwrap()["style"],
        "halo"
    );
    assert!(
        !h.commands().iter().any(|c| matches!(
            c,
            Cmd::Start { .. } | Cmd::Steer { .. } | Cmd::Resume { .. } | Cmd::AssistantAsk { .. }
        )),
        "preview never executes or changes a run"
    );
}

#[test]
fn preview_motion_can_be_paused_and_resumed_without_changing_reduced_motion_preference() {
    let mut h = harness(
        80,
        Some(json!({"enabled":true,"style":"halo","animate":false})),
    );
    // A small viewport exposes the same action from the side drawer.
    h.turn();
    h.click("◉");
    h.click("⚙");
    h.click("Appearance");
    h.enter();
    let screen = h.turn();
    for label in [
        "Halo orb",
        "Hover robot",
        "Apply avatar",
        "Cancel",
        "Auto cycle",
        "Pause motion",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    assert!(screen.contains("Listening · held"));
    h.click("Auto cycle");
    assert!(h.turn().contains("Listening · cycling"));
    h.click("Pause motion");
    assert!(h.turn().contains("Listening · held"));
    h.term.push_input(b"c");
    assert!(h.turn().contains("Listening · cycling"));
    h.term.push_input(b"\x1b");
    h.turn();
    assert_eq!(
        h.ctx.prefs.borrow().companion.as_ref().unwrap()["animate"],
        false
    );
}
#[test]
fn default_has_no_companion_or_background_inference_and_entity_focus_hides_it() {
    let mut h = harness(220, None);
    let screen = h.turn();
    assert!(!screen.contains("Talk / ask"));
    h.store.fold.update(|f| {
        f.push_item(Item::User {
            text: "A changed request".into(),
        })
    });
    h.turn();
    assert_eq!(advice_count(&h.commands()), 0);
    let mut h = harness(220, Some(json!({"enabled":true,"animate":false})));
    h.turn();
    h.store
        .focus
        .set(abstractcode::convo::Focus::Entity("named-entity".into()));
    let screen = h.turn();
    assert!(
        !screen.contains("Talk / ask"),
        "companion must not impersonate entity identity:\n{screen}"
    );
}
#[test]
fn advice_is_change_driven_not_replayed_and_tool_payloads_never_trigger_an_extra_request() {
    let mut h = harness(
        220,
        Some(json!({"enabled":true,"advice":true,"animate":false})),
    );
    h.turn();
    assert_eq!(
        advice_count(&h.commands()),
        0,
        "initial loaded conversation is a baseline"
    );
    h.store.fold.update(|f| {
        f.push_item(Item::User {
            text: "A new visible question".into(),
        })
    });
    h.turn();
    let commands = h.commands();
    assert_eq!(advice_count(&commands), 1);
    let context = commands
        .iter()
        .find_map(|c| match c {
            Cmd::AssistantAsk {
                scope: ChatScope::Advice,
                context,
                ..
            } => Some(context),
            _ => None,
        })
        .unwrap();
    assert!(context.contains("A new visible question"));
    h.store.fold.update(|f| f.push_item(tool()));
    h.turn();
    assert_eq!(advice_count(&h.commands()), 0);
    h.store.advice_chat.update(|s| {
        s.busy = false;
        s.messages.push((
            "assistant".into(),
            "Check the source date before relying on this result.".into(),
        ));
    });
    h.turn();
    assert!(
        h.store.companion_advice.get_untracked().is_some(),
        "tool-only events do not obsolete the visible-message snapshot"
    );
    h.store.fold.update(|f| {
        f.push_item(Item::User {
            text: "Another new question during cooldown".into(),
        })
    });
    h.turn();
    assert_eq!(
        advice_count(&h.commands()),
        0,
        "second request is deferred by cooldown"
    );
    h.store.restoring.set(true);
    h.turn();
    h.store.fold.update(|f| {
        f.push_item(Item::User {
            text: "An earlier replayed question".into(),
        })
    });
    h.turn();
    h.store.restoring.set(false);
    h.turn();
    assert_eq!(
        advice_count(&h.commands()),
        0,
        "history replay is not new activity"
    );
}

#[test]
fn automatic_comments_read_the_history_window_not_a_character_cap() {
    let mut h = harness(
        220,
        Some(json!({"enabled":true,"advice":true,"animate":false})),
    );
    h.turn();
    // 200,000 characters: far over the old 24,000-character cap, under no
    // message cap, and larger than the 50,000-token window as an older turn.
    let long = "earlier detail ".repeat(14_000);
    h.store.fold.update(|f| {
        f.push_item(Item::Assistant {
            text: long.clone(),
            final_answer: true,
            author: None,
        });
        f.push_item(Item::User {
            text: "A new visible question".into(),
        });
    });
    h.turn();
    let commands = h.commands();
    let context = commands
        .iter()
        .find_map(|c| match c {
            Cmd::AssistantAsk {
                scope: ChatScope::Advice,
                context,
                ..
            } => Some(context),
            _ => None,
        })
        .expect("one automatic comment request");
    assert!(context.contains("## User\nA new visible question"));
    assert!(!context.contains(&long) && !context.contains("Existing user request"));
    assert!(
        context.contains("[#TRUNCATION: 3 earlier message(s)"),
        "{context}"
    );
    assert!(!context.contains("message shortened"));
}

#[test]
fn a_genuine_approval_hides_the_companion_and_an_outside_click_never_resumes_it() {
    use abstractcode::transcript::{PendingWait, WaitKind};
    let mut h = harness(
        220,
        Some(json!({"enabled":true,"advice":true,"animate":false})),
    );
    h.turn();
    h.store.fold.update(|f| {
        f.pending_wait = Some(PendingWait {
            run_id: "main-root".into(),
            wait_key: "tool-approval".into(),
            step_id: "step-1".into(),
            kind: WaitKind::Approval {
                tool_calls: vec![
                    json!({"name":"write_file","arguments":{"path":"file.txt","content":"data"}}),
                ],
            },
        })
    });
    let screen = h.turn();
    assert!(h.ctx.modal_open(), "approval should be visible:\n{screen}");
    assert!(
        !screen.contains("Talk / ask"),
        "companion must yield to approval:\n{screen}"
    );
    h.term.push_input(b"\x1b[<0;1;1M\x1b[<0;1;1m");
    h.turn();
    assert!(h.ctx.modal_open() && h.store.fold.with_untracked(|f| f.pending_wait.is_some()));
    assert!(
        !h.commands().iter().any(|c| matches!(c, Cmd::Resume { .. })),
        "outside interaction cannot approve"
    );
}
#[test]
fn a_late_automatic_comment_cannot_cross_a_main_session_switch() {
    let mut h = harness(
        220,
        Some(json!({"enabled":true,"advice":true,"animate":false})),
    );
    h.turn();
    h.store.fold.update(|f| {
        f.push_item(Item::User {
            text: "New work in original session".into(),
        })
    });
    h.turn();
    assert_eq!(advice_count(&h.commands()), 1);
    h.store.session_id.set("another-session".into());
    h.store.fold.update(|f| {
        f.begin_run("another-root");
        f.push_item(Item::User {
            text: "A different conversation".into(),
        });
    });
    h.turn();
    h.store.advice_chat.update(|s| {
        s.busy = false;
        s.messages.push((
            "assistant".into(),
            "Stale original-session suggestion".into(),
        ));
    });
    h.turn();
    assert!(h.store.companion_advice.get_untracked().is_none());
    assert_eq!(
        advice_count(&h.commands()),
        0,
        "switching to existing history does not trigger new inference"
    );
}

#[test]
fn a_reduced_motion_companion_emits_no_terminal_output_on_idle_turns() {
    let mut h = harness(220, Some(json!({"enabled":true,"animate":false})));
    h.turn();
    h.turn();
    h.term.take_bytes();
    for _ in 0..4 {
        h.term.push_idle();
        h.turn();
    }
    assert!(
        h.term.bytes().is_empty(),
        "ready state must not repaint in an idle loop"
    );
    assert_eq!(advice_count(&h.commands()), 0);
}

#[test]
fn a_floating_companion_preserves_the_main_transcript_viewport() {
    for width in [80, 160, 220] {
        let mut hidden = harness(width, Some(json!({"enabled":false})));
        hidden.turn();
        let mut shown = harness(width, Some(json!({"enabled":true,"animate":false})));
        shown.turn();
        let scroll_bounds = |h: &mut Harness| {
            h.app
                .tree()
                .accessibility_tree()
                .entries
                .into_iter()
                .filter(|e| e.role == abstracttui::ui::Role::ScrollArea)
                .map(|e| e.bounds)
                .collect::<Vec<_>>()
        };
        let original = scroll_bounds(&mut hidden);
        assert!(!original.is_empty(), "main transcript must be measurable");
        assert_eq!(
            scroll_bounds(&mut shown),
            original,
            "enabling the companion must not narrow or shorten the transcript at {width} columns"
        );
    }
}

#[test]
fn transparent_companion_preserves_transcript_cells_and_restores_them_when_hidden() {
    for style in ["halo", "hover", "vortex", "eclipse", "eye", "rover"] {
        for position in ["top-right", "bottom-right"] {
            let mut h = harness(
                220,
                Some(json!({"enabled":false,"animate":false,"style":style,"position":position})),
            );
            h.store.fold.update(|f| {
                f.items.clear();
                f.push_item(Item::Assistant {
                    text: (0..60)
                        .map(|_| "0123456789".repeat(22))
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                    final_answer: true,
                    author: None,
                });
            });
            h.turn();
            let baseline = (0..40)
                .flat_map(|y| (0..220).map(move |x| (x, y)))
                .map(|(x, y)| h.term.screen().cell(x, y).unwrap().clone())
                .collect::<Vec<_>>();
            h.ctx.prefs.borrow_mut().companion.as_mut().unwrap()["enabled"] = json!(true);
            h.store.companion_tick.update(|n| *n += 1);
            h.turn();
            let art = h
                .app
                .tree()
                .accessibility_tree()
                .entries
                .into_iter()
                .find(|e| e.label == "Activity avatar")
                .unwrap()
                .bounds;
            let mut preserved_text = 0;
            let mut changed = 0;
            for y in art.y..art.bottom() {
                for x in art.x..art.right() {
                    let old = &baseline[(y * 220 + x) as usize];
                    let now = h.term.screen().cell(x, y).unwrap();
                    if old == now && !old.display().trim().is_empty() {
                        preserved_text += 1;
                    } else if old != now {
                        changed += 1;
                    }
                }
            }
            assert!(
                preserved_text >= 20,
                "{style}/{position}: rectangular wipe, only {preserved_text} text cells preserved"
            );
            assert!(
                changed >= 40,
                "{style}/{position}: avatar must still paint its silhouette"
            );
            h.ctx.prefs.borrow_mut().companion.as_mut().unwrap()["enabled"] = json!(false);
            h.store.companion_tick.update(|n| *n += 1);
            h.turn();
            for y in art.y..art.bottom() {
                for x in art.x..art.right() {
                    assert_eq!(
                        h.term.screen().cell(x, y).unwrap(),
                        &baseline[(y * 220 + x) as usize],
                        "{style}/{position}: stale art at {x},{y}"
                    );
                }
            }
        }
    }
}

#[test]
fn floating_art_and_actions_align_under_header_controls_without_stealing_root_keys() {
    for width in [160, 220] {
        let mut h = harness(width, Some(json!({"enabled":true,"animate":false})));
        h.turn();
        let entries = h.app.tree().accessibility_tree().entries;
        let bounds = |label: &str| {
            entries
                .iter()
                .find(|e| e.label == label)
                .unwrap_or_else(|| panic!("missing {label} at {width} columns"))
                .bounds
        };
        let art = bounds("Activity avatar");
        let button = bounds("Open companion conversation");
        assert_eq!(art, button, "art should fill its clickable surface");
        assert!(
            art.w >= 28 && art.h >= 15,
            "main art should retain preview detail"
        );
        let header = h
            .app
            .tree()
            .action_targets()
            .into_iter()
            .filter(|(id, r)| {
                r.y == 0
                    && matches!(
                        id.as_str(),
                        "chrome.tools"
                            | "chrome.docs"
                            | "chrome.theme"
                            | "chrome.about"
                            | "chrome.account"
                    )
            })
            .map(|(_, r)| r)
            .collect::<Vec<_>>();
        assert_eq!(
            header.len(),
            5,
            "measure the complete visible right header group"
        );
        let header_left = header.iter().map(|r| r.x).min().unwrap();
        let header_right = header.iter().map(|r| r.right()).max().unwrap();
        let twice_center = 2 * art.x + art.w;
        assert!((twice_center - header_left - header_right).abs() <= 1);
        let talk = bounds("Talk / ask");
        let settings = bounds("Companion settings");
        assert_eq!(talk.y, settings.y, "actions should share one row");
        assert!(talk.y >= art.bottom(), "actions must not cover the artwork");
        assert!((talk.x + settings.right() - twice_center).abs() <= 1);

        let x = button.x + button.w / 2 + 1;
        let y = button.y + button.h / 2 + 1;
        h.term.push_input(format!("\x1b[<0;{x};{y}M").as_bytes());
        h.turn();
        h.term.push_input(b"\x1b[<0;1;39m");
        h.turn();
        assert!(
            !h.store.docs_open.get_untracked(),
            "cancelled press should not activate"
        );
        h.term.push_input(b"\x11");
        h.turn();
        assert!(
            h.app.quit_requested(),
            "root Ctrl+Q remains reachable after cancelled art press"
        );
    }
}

#[test]
fn every_new_design_can_be_previewed_and_saved_by_stable_id() {
    for style in [
        AvatarStyle::Vortex,
        AvatarStyle::Eclipse,
        AvatarStyle::Eye,
        AvatarStyle::Rover,
    ] {
        let mut h = harness(220, Some(json!({"enabled":true,"animate":false})));
        h.turn();
        h.click("⚙");
        h.click("Appearance");
        h.enter();
        h.click(style.label());
        assert!(h
            .turn()
            .contains(&format!("Previewing {} · Not applied", style.label())));
        h.click("Apply avatar");
        assert!(
            !h.ctx.modal_open(),
            "apply should show the chosen avatar immediately"
        );
        assert!(h.turn().contains("Talk / ask"));
        assert_eq!(
            h.ctx.prefs.borrow().companion.as_ref().unwrap()["style"],
            style.id()
        );
        assert!(
            !h.commands().iter().any(|c| matches!(
                c,
                Cmd::Start { .. }
                    | Cmd::Steer { .. }
                    | Cmd::Resume { .. }
                    | Cmd::AssistantAsk { .. }
            )),
            "appearance is presentation only"
        );
    }
}
#[test]
fn terminal_failure_keeps_error_pose_after_summary_and_new_run_clears_it() {
    let mut h = harness(
        220,
        Some(json!({"enabled":true,"style":"hover","animate":false})),
    );
    h.turn();
    h.store.fold.update(|f| f.failed = true);
    assert!(h.turn().contains("Error"));
    h.store.fold.update(|f| f.begin_run("next-root"));
    assert!(h.turn().contains("Ready"));
}
#[test]
fn actual_mouse_motion_drives_gaze_without_typing_or_sending_a_task() {
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };
    let mut h = harness(
        220,
        Some(json!({"enabled":true,"style":"hover","animate":true})),
    );
    let start = Instant::now();
    let now = Rc::new(Cell::new(start));
    let clock = now.clone();
    h.driver.set_clock(move || clock.get());
    h.turn();
    h.turn();
    // Real SGR motion crosses the root capture handler without entering the composer.
    h.term.push_input(b"\x1b[<35;1;2M");
    h.turn();
    now.set(start + Duration::from_millis(90));
    h.turn();
    let positions: Vec<_> = (1..35)
        .flat_map(|y| (160..218).map(move |x| (x, y)))
        .collect();
    let before: Vec<_> = positions
        .iter()
        .map(|&(x, y)| h.term.screen().cell(x, y).unwrap().clone())
        .collect();
    h.term.push_input(b"\x1b[<35;220;35M");
    h.turn();
    now.set(start + Duration::from_millis(180));
    h.turn();
    let changed = positions
        .iter()
        .zip(&before)
        .any(|(&(x, y), old)| h.term.screen().cell(x, y).unwrap() != old);
    assert!(
        changed,
        "actual terminal cursor reports must reach the avatar"
    );
    assert!(
        !h.commands().iter().any(|c| matches!(
            c,
            Cmd::Start { .. } | Cmd::Steer { .. } | Cmd::Resume { .. } | Cmd::AssistantAsk { .. }
        )),
        "pointer does not send input"
    );
    abstracttui::reactive::set_loop_clock(None);
}

fn failed_tool(h: &Harness, id: &str) {
    h.store.fold.update(|f| {
        f.apply(
            "main-root",
            &json!({"run_id":"main-root","step_id":id,"status":"completed",
            "effect":{"type":"emit_event","payload":{"name":"abstract.tool_result","payload":[
                {"tool":"run_command","call_id":id,"success":false,"error":"command not found"}
            ]}}}),
        );
    });
}

#[test]
fn a_fast_recoverable_tool_error_holds_for_one_second_even_during_speech() {
    use abstractcode::{store::Phase, voice::Speaking};
    use abstracttui::reactive::{batch, set_loop_clock};
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };
    let mut h = harness(
        220,
        Some(json!({"enabled":true,"style":"hover","animate":false})),
    );
    let start = Instant::now();
    let now = Rc::new(Cell::new(start));
    let clock = now.clone();
    h.driver.set_clock(move || clock.get());
    h.turn();
    set_loop_clock(Some(start));
    batch(|| {
        h.store.phase.set(Phase::Running);
        failed_tool(&h, "fast-failure");
        h.store.fold.update(|f| {
            f.push_item(Item::Assistant {
                text: "Recovered immediately".into(),
                final_answer: true,
                author: None,
            })
        });
        h.store.phase.set(Phase::Idle);
        h.store.voice.speaking.set(Speaking::Playing);
    });
    assert!(
        !h.store.fold.with(|f| f.failed),
        "tool failure is not a failed run"
    );
    assert_eq!(h.store.fold.with(|f| f.stats.tool_failures), 1);
    assert!(h.turn().contains("Error"));
    for ms in [1, 500, 999] {
        now.set(start + Duration::from_millis(ms));
        h.store.voice.speak_levels.set(vec![0.8]);
        assert!(h.turn().contains("Error"), "lost Error at {ms}ms");
    }
    now.set(start + Duration::from_millis(1000));
    assert!(h.turn().contains("Speaking"));
    assert!(!h.store.companion_error_active.get_untracked());
    set_loop_clock(None);
}

#[test]
fn each_new_failure_restarts_the_hold_but_replay_and_session_switches_do_not() {
    use abstracttui::reactive::set_loop_clock;
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };
    let mut h = harness(220, Some(json!({"enabled":true,"animate":false})));
    let start = Instant::now();
    let now = Rc::new(Cell::new(start));
    let clock = now.clone();
    h.driver.set_clock(move || clock.get());
    h.turn();
    set_loop_clock(Some(start));
    h.store.restoring.set(true);
    failed_tool(&h, "historical");
    h.store.restoring.set(false);
    assert!(h.turn().contains("Ready"), "history must not flash Error");
    set_loop_clock(Some(start));
    failed_tool(&h, "live-1");
    assert!(h.turn().contains("Error"));
    now.set(start + Duration::from_millis(600));
    h.turn();
    set_loop_clock(Some(now.get()));
    failed_tool(&h, "live-2");
    h.turn();
    now.set(start + Duration::from_millis(1599));
    assert!(h.turn().contains("Error"));
    now.set(start + Duration::from_millis(1600));
    assert!(h.turn().contains("Ready"));
    set_loop_clock(Some(now.get()));
    failed_tool(&h, "live-3");
    h.turn();
    assert!(h.store.companion_error_active.get_untracked());
    h.store.session_id.set("different-session".into());
    h.turn();
    assert!(!h.store.companion_error_active.get_untracked());
    now.set(start + Duration::from_secs(3));
    assert!(h.turn().contains("Ready"));
    set_loop_clock(None);
}

#[test]
fn preview_focus_and_selected_marker_match_every_saved_design_and_reopen_after_apply() {
    for style in AvatarStyle::ALL {
        let mut h = harness(
            220,
            Some(json!({"enabled":true,"style":style.id(),"animate":false})),
        );
        h.turn();
        h.click("⚙");
        h.click("Appearance");
        h.enter();
        let screen = h.turn();
        assert!(
            screen.contains(&format!("Previewing {} · In use", style.label())),
            "{screen}"
        );
        assert!(
            screen.contains(&format!("● {}", style.label()))
                || screen.contains(&format!("●{}", style.label())),
            "{screen}"
        );
        h.click("Vortex");
        h.click("Apply avatar");
        assert!(!h.ctx.modal_open());
        assert_eq!(
            h.ctx.prefs.borrow().companion.as_ref().unwrap()["style"],
            "vortex"
        );
        h.click("⚙");
        h.click("Appearance");
        h.enter();
        assert!(h.turn().contains("Previewing Vortex · In use"));
        h.enter();
        assert!(
            !h.ctx.modal_open(),
            "Enter on the focused saved design applies it"
        );
        assert_eq!(
            h.ctx.prefs.borrow().companion.as_ref().unwrap()["style"],
            "vortex"
        );
    }
}

#[test]
fn a_new_run_can_fail_in_one_batch_and_disposal_cancels_the_hold() {
    use abstracttui::reactive::{batch, next_timer_deadline, run_due_timers};
    let mut h = harness(220, Some(json!({"enabled":true,"animate":false})));
    h.turn();
    batch(|| {
        h.store.fold.update(|f| {
            f.begin_run("fast-new-root");
            f.apply("fast-new-root", &json!({"run_id":"fast-new-root","step_id":"fail","status":"completed",
                "effect":{"type":"emit_event","payload":{"name":"abstract.tool_result","payload":[
                    {"tool":"run_command","call_id":"fail","success":false,"error":"command not found"}
                ]}}}));
        });
    });
    assert!(h.turn().contains("Error"));
    assert!(h.store.companion_error_active.get_untracked());
    drop(h);
    // Other host one-shots (focus/toast) may still be queued. Draining them must
    // neither touch disposed signals nor re-arm the error indication.
    for _ in 0..20 {
        let Some(deadline) = next_timer_deadline() else {
            break;
        };
        run_due_timers(deadline);
    }
    assert!(
        next_timer_deadline().is_none(),
        "disposed hold must not keep a repeating clock"
    );
}

#[test]
fn enter_applies_the_focused_design_while_phase_enter_only_previews() {
    for style in AvatarStyle::ALL {
        let mut h = harness(
            220,
            Some(json!({"enabled":true,"style":"hover","animate":false})),
        );
        h.turn();
        h.click("⚙");
        h.click("Appearance");
        h.enter();
        h.click("Speaking");
        h.enter();
        assert!(
            h.ctx.modal_open(),
            "Enter on a phase must not apply appearance"
        );
        assert_eq!(
            h.ctx.prefs.borrow().companion.as_ref().unwrap()["style"],
            "hover"
        );
        h.click(style.label());
        assert!(
            h.ctx.modal_open(),
            "mouse click should only preview the design"
        );
        h.enter();
        assert!(!h.ctx.modal_open(), "Enter must apply {style:?}");
        assert_eq!(
            h.ctx.prefs.borrow().companion.as_ref().unwrap()["style"],
            style.id()
        );
        assert!(h.turn().contains("Talk / ask"));
    }
}
