//! Folded tool categories; engine List owns scrolling, selection and pointer routing.
use crate::{
    store::{Store, ToolInfo},
    tool_modes::{self, Mode},
    ui::UiCtx,
};
use abstracttui::{
    app::current_viewport,
    prelude::*,
    render::{
        rich::{RichLine, RichText, Span},
        Attrs, Style,
    },
    theme::TokenSet,
    widgets::List,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
enum Target {
    Category(String),
    Tool(String),
}
impl Target {
    fn key(&self) -> String {
        match self {
            Self::Category(c) => format!("category:{c}"),
            Self::Tool(n) => format!("tool:{n}"),
        }
    }
    fn category<'a>(&'a self, tools: &'a [ToolInfo]) -> Option<&'a str> {
        match self {
            Self::Category(c) => Some(c),
            Self::Tool(n) => tools
                .iter()
                .find(|t| &t.name == n)
                .map(|t| t.toolset.as_str()),
        }
    }
}
fn targets(tools: &[ToolInfo], expanded: &BTreeSet<String>) -> Vec<Target> {
    // Inventory order is not a grouping contract: one category must remain one row.
    let mut groups = BTreeMap::<&str, Vec<&str>>::new();
    for tool in tools {
        groups.entry(&tool.toolset).or_default().push(&tool.name);
    }
    let mut rows = Vec::new();
    for (category, mut names) in groups {
        rows.push(Target::Category(category.into()));
        if expanded.contains(category) {
            names.sort_unstable();
            rows.extend(names.into_iter().map(|n| Target::Tool(n.into())));
        }
    }
    rows
}
#[derive(Clone, Copy)]
struct Picker {
    cursor: Signal<usize>,
    key: Signal<String>,
    expanded: Signal<BTreeSet<String>>,
}
impl Picker {
    fn selected(self, tools: &[ToolInfo]) -> Option<Target> {
        targets(tools, &self.expanded.get_untracked())
            .get(self.cursor.get_untracked())
            .cloned()
    }
    fn fold(self, tools: &[ToolInfo], target: &Target, open: bool) {
        if let Some(category) = target.category(tools) {
            let category = category.to_string();
            self.key.set(Target::Category(category.clone()).key());
            self.expanded.update(|groups| {
                if open {
                    groups.insert(category);
                } else {
                    groups.remove(&category);
                }
            });
        }
    }
}
fn members(target: &Target, tools: &[ToolInfo]) -> Vec<String> {
    tools
        .iter()
        .filter(|t| match target {
            Target::Category(c) => &t.toolset == c,
            Target::Tool(n) => &t.name == n,
        })
        .map(|t| t.name.clone())
        .collect()
}
fn state(
    target: &Target,
    tools: &[ToolInfo],
    disabled: &[String],
    pins: &[(String, String)],
    tier: &str,
) -> Option<Mode> {
    let names = members(target, tools);
    let mut modes = tools
        .iter()
        .filter(|t| !t.served_disabled && names.contains(&t.name))
        .map(|t| tool_modes::effective(t, disabled, pins, tier));
    let mode = modes.next()?;
    if modes.all(|m| m == mode) {
        Some(mode)
    } else {
        None
    }
}
fn apply(store: Store, ctx: &UiCtx, target: &Target, mode: Mode) {
    let tools = store.tools.get_untracked();
    let names = members(target, &tools);
    let mut disabled = store.disabled_tools.get_untracked();
    let mut pins = store.tool_overrides.get_untracked();
    if tool_modes::set(&tools, &names, mode, &mut disabled, &mut pins) == 0 {
        store.notify("Disabled by the Gateway; this client cannot enable these tools.");
        return;
    }
    store.disabled_tools.set(disabled);
    store.tool_overrides.set(pins);
    super::persist_tool_prefs(store, ctx, |_| {});
}
fn cycle(store: Store, ctx: &UiCtx, mut target: Target, category: bool) {
    let tools = store.tools.get_untracked();
    if category {
        if let Some(c) = target.category(&tools) {
            target = Target::Category(c.into());
        }
    }
    let mode = state(
        &target,
        &tools,
        &store.disabled_tools.get_untracked(),
        &store.tool_overrides.get_untracked(),
        &store.accepted_tier.get_untracked(),
    )
    .map(Mode::next)
    .unwrap_or(Mode::Ask);
    apply(store, ctx, &target, mode);
}
fn row_text(
    target: &Target,
    tools: &[ToolInfo],
    expanded: &BTreeSet<String>,
    disabled: &[String],
    pins: &[(String, String)],
    tier: &str,
    t: &TokenSet,
) -> (String, RichText) {
    let mode = state(target, tools, disabled, pins, tier);
    let available = tools
        .iter()
        .filter(|tool| {
            !tool.served_disabled
                && match target {
                    Target::Category(c) => &tool.toolset == c,
                    Target::Tool(n) => &tool.name == n,
                }
        })
        .count();
    let label = if available == 0 {
        "Unavailable"
    } else {
        mode.map(Mode::label).unwrap_or("Mixed")
    };
    let color = if available == 0 {
        t.text_muted
    } else {
        match mode {
            Some(Mode::Off) => t.error,
            Some(Mode::Ask) => t.warn,
            Some(Mode::Auto) => t.ok,
            None => t.info,
        }
    };
    let status = Span::new(
        format!("[{label:4}]"),
        Style::new().fg(color).attrs(Attrs::BOLD),
    );
    let spans = match target {
        Target::Category(c) => {
            let count = tools.iter().filter(|tool| &tool.toolset == c).count();
            vec![
                Span::new(
                    format!(
                        "{} {}  ",
                        if expanded.contains(c) { "▾" } else { "▸" },
                        if c.is_empty() { "Other" } else { c }
                    ),
                    Style::new().fg(t.text).attrs(Attrs::BOLD),
                ),
                status,
                Span::new(
                    format!("  {count} tool{}", if count == 1 { "" } else { "s" }),
                    Style::new().fg(t.text_muted),
                ),
            ]
        }
        Target::Tool(n) => vec![
            Span::new("│   ", Style::new().fg(t.text_muted)),
            status,
            Span::new(format!(" {n}"), Style::new().fg(t.text)),
        ],
    };
    let plain = spans.iter().map(|s| s.text.as_str()).collect::<String>();
    (
        plain,
        RichText::from_lines(vec![RichLine::from_spans(spans)]),
    )
}
pub fn open(cx: Scope, store: Store, ctx: &UiCtx) {
    let vp = current_viewport();
    let size = Size::new((vp.w * 96 / 100).clamp(1, 116), (vp.h - 2).clamp(1, 34));
    let context = ctx.clone();
    ctx.open_modal(cx, size, move |mcx| {
        let t = abstracttui::app::current_theme().tokens;
        // These are instructions, not decoration: retain readable ink in every theme.
        let mut help_tokens = t;
        help_tokens.text_faint = t.text;
        // A new visit always starts with an overview, even after a previous expansion.
        let picker = Picker { cursor: mcx.signal(0), key: mcx.signal(String::new()), expanded: mcx.signal(BTreeSet::new()) };
        let offset = mcx.signal(0i32);
        let mut root = Element::new().style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .shortcut(KeyChord::plain(Key::Escape), { let ctx = context.clone(); move |_| ctx.close_modal() })
            .shortcut(KeyChord::plain(Key::Char('c')), { let ctx = context.clone(); move |_| {
                if let Some(target) = picker.selected(&store.tools.get_untracked()) { cycle(store, &ctx, target, true); }
            }});
        for (key, open) in [(Key::Left, false), (Key::Right, true)] {
            root = root.shortcut(KeyChord::plain(key), move |_| {
                let tools = store.tools.get_untracked();
                if let Some(target) = picker.selected(&tools) { picker.fold(&tools, &target, open); }
            });
        }
        root = root.child(dyn_view(LayoutStyle::line(1), move || {
            let tools = store.tools.get(); let mut counts = [0usize; 3]; let mut unavailable = 0;
            let disabled = store.disabled_tools.get(); let pins = store.tool_overrides.get(); let tier = store.accepted_tier.get();
            for tool in &tools {
                if tool.served_disabled { unavailable += 1; continue; }
                counts[match tool_modes::effective(tool, &disabled, &pins, &tier) { Mode::Off => 0, Mode::Ask => 1, Mode::Auto => 2 }] += 1;
            }
            let err = store.tools_error.get();
            super::modals::title_row(&t, if !err.is_empty() { format!("Tools — {err}") } else { format!("Tools — {} Off · {} Ask · {} Auto · {unavailable} unavailable", counts[0], counts[1], counts[2]) })
        })).child(dyn_view_scoped(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)), {
            let ctx = context.clone(); move |scx| {
                let tools = store.tools.get(); let expanded = picker.expanded.get(); let rows = targets(&tools, &expanded);
                let disabled = store.disabled_tools.get(); let pins = store.tool_overrides.get(); let tier = store.accepted_tier.get();
                let (labels, rich): (Vec<_>, Vec<_>) = rows.iter().map(|row| row_text(row, &tools, &expanded, &disabled, &pins, &tier, &t)).unzip();
                let keys = rows.iter().map(Target::key).collect::<Vec<_>>();
                let accessory = rows.iter().map(|row| match row { Target::Category(c) => Some(if expanded.contains(c) { "Collapse" } else { "Expand" }.to_string()), Target::Tool(_) => None }).collect::<Vec<_>>();
                let activate_rows = rows.clone(); let accessory_rows = rows;
                let ctx = ctx.clone();
                let mut list_tokens = t;
                list_tokens.selection_bg = t.surface_raised;
                list_tokens.selection_fg = t.text;
                List::new(labels).rich_items(rich).selection(picker.cursor).selection_key(picker.key).offset_y(offset)
                    .key_fn(move |ix, _| keys[ix].clone())
                    .row_accessory(move |ix, _| accessory[ix].clone()).accessory_width(10)
                    .on_accessory_click(move |ix| {
                        if let Some(target @ Target::Category(c)) = accessory_rows.get(ix) {
                            picker.fold(&store.tools.get_untracked(), target, !picker.expanded.get_untracked().contains(c));
                        }
                    })
                    .on_activate(move |ix| {
                        if let Some(target) = activate_rows.get(ix) {
                            match target {
                                Target::Category(c) => picker.fold(&store.tools.get_untracked(), target, !picker.expanded.get_untracked().contains(c)),
                                Target::Tool(_) => cycle(store, &ctx, target.clone(), false),
                            }
                        }
                    })
                    .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0))).element(scx, &list_tokens).autofocus().build()
            }
        })).child(dyn_view_scoped(LayoutStyle::default().height(Dimension::Cells(4)).shrink(0.0), move |scx| {
            let tools = store.tools.get(); let rows = targets(&tools, &picker.expanded.get());
            let detail = match rows.get(picker.cursor.get()) {
                Some(Target::Tool(n)) => {
                    let tool = tools.iter().find(|t| &t.name == n).expect("row from inventory");
                    let mut text = format!("**{n}** — {}", tool.description);
                    if tool.served_disabled { text.push_str(&format!("\n\nUnavailable on this Gateway. {}", tool.enable_gate)); }
                    if let Some(sb) = &tool.sandbox { text.push_str(&format!("\n\nCommand sandbox: {}", sb.label)); }
                    text
                }
                Some(Target::Category(c)) => format!("**{}** — Set the category's mode below, or expand to adjust individual tools. Gateway restrictions apply.", if c.is_empty() { "Other" } else { c }),
                None => "No tools returned by the Gateway.".into(),
            };
            Scroll::new(MarkdownView::new(detail).view(scx)).scrollbar_auto_hide(true).view(scx)
        }));
        let mut buttons = Element::new().style(LayoutStyle::row().gap(2));
        for mode in [Mode::Off, Mode::Ask, Mode::Auto] {
            let ctx = context.clone();
            let mut mode_tokens = t;
            mode_tokens.text = match mode { Mode::Off => t.error, Mode::Ask => t.warn, Mode::Auto => t.ok };
            buttons = buttons.child(Button::new(mode.label()).on_click(move || {
                if let Some(target) = picker.selected(&store.tools.get_untracked()) { apply(store, &ctx, &target, mode); }
            }).element(mcx, &mode_tokens).build());
        }
        root.child(buttons.build())
            .child(super::modals::hint_row(&help_tokens, "↑↓ browse · ←→ fold · Enter/Space expand/cycle · c modes · Esc close".into()))
            .child(super::modals::hint_row(&help_tokens, "Off: excluded · Ask: approval · Auto: no approval · future runs".into())).build()
    });
}
