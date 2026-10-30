//! Real input/render coverage for the folded Tools picker, with ephemeral preferences.
use abstractcode::{
    config::Prefs,
    runner::Cmd,
    store::{Store, ToolInfo},
    ui::{tools_view, UiCtx},
};
use abstracttui::{app::Driver, prelude::*, testing::CaptureTerm};
use std::{cell::RefCell, rc::Rc, sync::mpsc};

struct Harness {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
    ctx: UiCtx,
    cx: Scope,
    _rx: mpsc::Receiver<Cmd>,
}
fn tool(name: &str, category: &str, unavailable: bool) -> ToolInfo {
    ToolInfo {
        name: name.into(),
        toolset: category.into(),
        description: format!("Detailed help for {name}; keep this out of the list."),
        served_disabled: unavailable,
        enable_gate: "Enable the camera on the Gateway.".into(),
        ..Default::default()
    }
}
fn harness(size: Size, theme: &str) -> Harness {
    abstracttui::app::set_theme_by_id(theme);
    let mut app = App::new(size);
    let overlays = app.overlays();
    let quitter = app.quitter();
    let (tx, rx) = mpsc::channel();
    let slot = Rc::new(RefCell::new(None));
    let out = slot.clone();
    app.mount(move |cx| {
        let store = Store::create(cx);
        store.session_id.set("tool-picker-fixture".into());
        // Intentionally interleaved: grouping must not depend on Gateway ordering.
        store.tools.set(vec![
            tool("read_file", "files", false),
            tool("camera_open", "camera", true),
            tool("write_file", "files", false),
            tool("web_search", "web", false),
        ]);
        let ctx = UiCtx {
            tx,
            client: abstractcode::gateway::GatewayClient::new("http://127.0.0.1:1", None),
            overlays,
            quitter,
            prefs: Rc::new(RefCell::new(Prefs::default())),
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
        *out.borrow_mut() = Some((store, ctx, cx));
        Element::new().build()
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
                c.unicode_ok = true;
            })),
            ..Default::default()
        },
    )
    .unwrap();
    let (store, ctx, cx) = slot.borrow().clone().unwrap();
    let mut h = Harness {
        app,
        term,
        driver,
        store,
        ctx,
        cx,
        _rx: rx,
    };
    h.turn();
    tools_view::open(h.cx, h.store, &h.ctx);
    h.turn();
    h
}
impl Harness {
    fn turn(&mut self) -> String {
        self.driver.turn(&mut self.app, &mut self.term).unwrap();
        self.term.screen().to_text()
    }
    fn press(&mut self, bytes: &[u8]) -> String {
        self.term.push_input(bytes);
        self.turn()
    }
}
#[test]
fn collapsed_categories_expand_without_changing_modes_and_reopen_collapsed() {
    let mut h = harness(Size::new(180, 45), "abstract-dark");
    let screen = h.turn();
    assert_eq!(screen.matches("▸ files").count(), 1, "{screen}");
    for name in ["read_file", "write_file", "camera_open", "web_search"] {
        assert!(!screen.contains(name), "collapsed: {screen}");
    }
    h.press(b"\x1b[B"); // files
    let screen = h.press(b"\r");
    assert!(
        screen.contains("▾ files")
            && screen.contains("│   [Auto] read_file")
            && screen.contains("│   [Ask ] write_file"),
        "{screen}"
    );
    assert!(
        !screen.contains("Detailed help"),
        "descriptions only when a tool is selected: {screen}"
    );
    assert!(h.store.disabled_tools.get_untracked().is_empty());
    h.press(b"\x1b[B"); // read_file
    assert!(h.turn().contains("Detailed help for read_file"));
    let screen = h.press(b"\x1b[D"); // collapse from child and select its parent
    assert!(
        screen.contains("▸ files") && !screen.contains("read_file"),
        "{screen}"
    );
    h.press(b"\x1b[C"); // expand again
    h.ctx.close_modal();
    h.turn();
    tools_view::open(h.cx, h.store, &h.ctx);
    let screen = h.turn();
    assert!(
        screen.contains("▸ files") && !screen.contains("read_file"),
        "{screen}"
    );
}
#[test]
fn category_and_individual_modes_keep_gateway_gates_and_other_categories() {
    let mut h = harness(Size::new(120, 32), "abstract-dark");
    h.press(b"\x1b[B"); // files, mixed Auto/Ask -> Ask
    h.press(b"c");
    assert_eq!(
        h.store.tool_overrides.get_untracked(),
        vec![
            ("read_file".into(), "ask".into()),
            ("write_file".into(), "ask".into())
        ]
    );
    h.press(b"c"); // files -> Auto
    let screen = h.press(b"c"); // files -> Off
    assert!(screen.contains("files  [Off ]"), "{screen}");
    assert_eq!(
        h.store.disabled_tools.get_untracked(),
        vec!["read_file", "write_file"]
    );
    h.press(b" ");
    h.press(b"\x1b[B"); // expand and read_file
    let screen = h.press(b" "); // read_file -> Ask; write_file remains Off
    assert!(
        screen.contains("[Ask ] read_file")
            && screen.contains("[Off ] write_file")
            && screen.contains("files  [Mixed]"),
        "{screen}"
    );
    assert_eq!(h.store.disabled_tools.get_untracked(), vec!["write_file"]);
    assert!(!h
        .store
        .tool_overrides
        .get_untracked()
        .iter()
        .any(|(n, _)| n == "web_search"));
    h.press(b"\x1b[H"); // first category, camera unavailable
    h.press(b"c");
    assert!(!h
        .store
        .tool_overrides
        .get_untracked()
        .iter()
        .any(|(n, _)| n == "camera_open"));
    let screen = h.press(b"\r");
    assert!(screen.contains("[Unavailable] camera_open"), "{screen}");
    h.press(b"\x1b[B");
    let screen = h.turn();
    assert!(
        screen.contains("Enable the camera on the Gateway."),
        "selected gate details: {screen}"
    );
}
#[test]
fn mode_colors_are_distinct_and_names_stay_neutral_on_selected_rows() {
    for theme in ["abstract-dark", "abstract-light"] {
        let mut h = harness(Size::new(120, 32), theme);
        h.press(b"\x1b[B");
        h.press(b"\r");
        h.press(b"\x1b[B"); // selected read_file
        let text = h.turn();
        let shot = h.term.screen().screenshot();
        let tokens = abstracttui::app::current_theme().tokens;
        for (needle, color) in [
            ("[Auto] read_file", tokens.ok),
            ("[Ask ] write_file", tokens.warn),
        ] {
            let (y, line) = text
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains(needle))
                .expect(&text);
            let byte = line.find(needle).unwrap();
            let x = line[..byte].chars().count();
            assert_eq!(
                shot.cell((x + 1) as i32, y as i32).unwrap().fg(),
                Some(color),
                "mode ink in {theme}"
            );
            assert_eq!(
                shot.cell((x + 7) as i32, y as i32).unwrap().fg(),
                Some(tokens.text),
                "neutral name in {theme}"
            );
        }
        let text = h.press(b" "); // selected tool is now Off
        let shot = h.term.screen().screenshot();
        let (y, line) = text
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("[Off ] read_file"))
            .unwrap();
        let x = line[..line.find("[Off ]").unwrap()].chars().count();
        assert_eq!(
            shot.cell((x + 1) as i32, y as i32).unwrap().fg(),
            Some(tokens.error)
        );
        assert_eq!(
            shot.cell((x + 8) as i32, y as i32).unwrap().bg(),
            Some(tokens.surface_raised),
            "neutral selection background in {theme}"
        );
        if let Some(dir) = std::env::var_os("TOOLS_PICKER_CAPTURE_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                std::path::Path::new(&dir).join(format!("tools-{theme}.svg")),
                shot.to_svg(),
            )
            .unwrap();
        }
    }
}

#[test]
fn category_mouse_accessory_and_inventory_updates_preserve_selected_tool() {
    for size in [Size::new(80, 24), Size::new(120, 32)] {
        let mut h = harness(size, "abstract-dark");
        let text = h.turn();
        assert!(
            text.contains("Esc close") && text.contains("future runs"),
            "complete compact help: {text}"
        );
        let (y, line) = text
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("▸ files"))
            .unwrap();
        let x = line[..line.find("Expand").unwrap()].chars().count();
        h.press(format!("\x1b[<0;{};{}M", x + 1, y + 1).as_bytes());
        let text = h.press(format!("\x1b[<0;{};{}m", x + 1, y + 1).as_bytes());
        assert!(
            text.contains("▾ files") && text.contains("│   [Auto] read_file"),
            "one-click expansion: {text}"
        );
        assert!(h.store.tool_overrides.get_untracked().is_empty());
        h.press(b"\x1b[B"); // selected read_file
        h.store
            .tools
            .update(|tools| tools.insert(0, tool("new_tool", "a new category", false)));
        h.turn();
        let text = h.press(b" ");
        assert_eq!(
            h.store.disabled_tools.get_untracked(),
            vec!["read_file"],
            "stable tool selection after inventory insert: {text}"
        );
    }
}
