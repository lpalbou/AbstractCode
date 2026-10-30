//! Tool and category modes; engine List owns scrolling, selection and pointer routing.
use crate::{
    store::{Store, ToolInfo},
    tool_modes::{self, Mode},
    ui::UiCtx,
};
use abstracttui::{app::current_viewport, prelude::*, widgets::List};

#[derive(Clone)]
enum Target {
    Category(String),
    Tool(String),
}
fn targets(tools: &[ToolInfo]) -> Vec<Target> {
    let mut rows = Vec::new();
    let mut group = None;
    for t in tools {
        if group.as_ref() != Some(&t.toolset) {
            group = Some(t.toolset.clone());
            rows.push(Target::Category(t.toolset.clone()));
        }
        rows.push(Target::Tool(t.name.clone()));
    }
    rows
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
fn selected(store: Store, cursor: Signal<usize>) -> Option<Target> {
    targets(&store.tools.get_untracked())
        .get(cursor.get_untracked())
        .cloned()
}
fn cycle(store: Store, ctx: &UiCtx, cursor: Signal<usize>, category: bool) {
    if let Some(mut target) = selected(store, cursor) {
        if category {
            if let Target::Tool(n) = &target {
                if let Some(t) = store
                    .tools
                    .with_untracked(|ts| ts.iter().find(|t| &t.name == n).cloned())
                {
                    target = Target::Category(t.toolset);
                }
            }
        }
        let mode = state(
            &target,
            &store.tools.get_untracked(),
            &store.disabled_tools.get_untracked(),
            &store.tool_overrides.get_untracked(),
            &store.accepted_tier.get_untracked(),
        )
        .map(Mode::next)
        .unwrap_or(Mode::Ask);
        apply(store, ctx, &target, mode);
    }
}
pub fn open(cx: Scope, store: Store, ctx: &UiCtx) {
    let vp = current_viewport();
    let size = Size::new((vp.w * 96 / 100).max(1), (vp.h - 2).max(1));
    let context = ctx.clone();
    ctx.open_modal(cx,size,move |mcx| {
        let t=abstracttui::app::current_theme().tokens;
        let cursor=mcx.signal(if store.tools.with_untracked(|t|t.is_empty()){0}else{1});
        let offset=mcx.signal(0i32);
        let mut root=Element::new().style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .shortcut(KeyChord::plain(Key::Escape),{let ctx=context.clone();move |_|ctx.close_modal()})
            .shortcut(KeyChord::plain(Key::Char('c')),{let ctx=context.clone();move |_|cycle(store,&ctx,cursor,true)})
            .child(dyn_view(LayoutStyle::line(1),move || {
                let tools=store.tools.get();let mut counts=[0usize;3];let mut unavailable=0;
                let disabled=store.disabled_tools.get();let pins=store.tool_overrides.get();let tier=store.accepted_tier.get();
                for tool in &tools {if tool.served_disabled {unavailable+=1;continue;}let mode=tool_modes::effective(tool,&disabled,&pins,&tier);counts[match mode{Mode::Off=>0,Mode::Ask=>1,Mode::Auto=>2}]+=1;}
                let err=store.tools_error.get();
                super::modals::title_row(&t,if !err.is_empty(){format!("Tools — {err}")}else{format!("Tools — {} Off · {} Ask · {} Auto · {unavailable} unavailable",counts[0],counts[1],counts[2])})
            }))
            .child(dyn_view_scoped(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),{let ctx=context.clone();move |scx| {
                let tools=store.tools.get();let rows=targets(&tools);
                let disabled=store.disabled_tools.get();let pins=store.tool_overrides.get();let tier=store.accepted_tier.get();
                let labels=rows.iter().map(|row| {
                    let mode=state(row,&tools,&disabled,&pins,&tier).map(|m|m.label()).unwrap_or("Mixed");
                    match row {
                        Target::Category(c)=>{
                            let available=tools.iter().filter(|t|&t.toolset==c&&!t.served_disabled).count();
                            format!("▸ {}  [{}]  {available} available",if c.is_empty(){"Other"}else{c},if available==0{"Unavailable"}else{mode})
                        },
                        Target::Tool(n)=>{
                            let tool=tools.iter().find(|t|&t.name==n).expect("row from inventory");
                            format!("  [{:4}] {}{}  {}",if tool.served_disabled{"—"}else{mode},n,tool.sandbox.as_ref().map(|sb|format!("  [{}]",sb.label)).unwrap_or_default(),if tool.served_disabled{"Unavailable on this Gateway"}else{&tool.description})
                        }
                    }
                }).collect();
                let keys=rows.iter().map(|r|match r{Target::Category(c)=>format!("category:{c}"),Target::Tool(n)=>format!("tool:{n}")}).collect::<Vec<_>>();
                let ctx=ctx.clone();
                List::new(labels).selection(cursor).offset_y(offset).key_fn(move |ix,_|keys[ix].clone())
                    .on_activate(move |_|cycle(store,&ctx,cursor,false))
                    .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0))).element(scx,&t).autofocus().build()
            }}))
            .child(dyn_view_scoped(LayoutStyle::default().height(Dimension::Cells(3)).shrink(0.0),move |scx| {
                let tools=store.tools.get();let row=targets(&tools).get(cursor.get()).cloned();
                let detail=match row {
                    Some(Target::Tool(n))=>{let tool=tools.iter().find(|t|t.name==n).unwrap();
                        let mut text=format!("**{n}** — {}",tool.description);
                        if tool.served_disabled {text.push_str(&format!("\n\nUnavailable on this Gateway. {}",tool.enable_gate));}
                        if let Some(sb)=&tool.sandbox {text.push_str(&format!("\n\nCommand sandbox: {}",sb.label));}text
                    },
                    Some(Target::Category(c))=>format!("**{}** — Off, Ask or Auto applies to all currently listed, available tools in this category. Gateway restrictions remain in force.",if c.is_empty(){"Other"}else{&c}),
                    None=>"No tools returned by the Gateway.".into()
                };
                Scroll::new(MarkdownView::new(detail).view(scx)).scrollbar_auto_hide(true).view(scx)
            }));
        let mut buttons=Element::new().style(LayoutStyle::row().gap(2));
        for mode in [Mode::Off,Mode::Ask,Mode::Auto] {
            let ctx=context.clone();buttons=buttons.child(Button::new(mode.label()).on_click(move ||{if let Some(target)=selected(store,cursor){apply(store,&ctx,&target,mode);}}).view(mcx));
        }
        root=root.child(buttons.build())
            .child(super::modals::hint_row(&t,"↑↓ / PgUp PgDn / wheel browse · Space/Enter cycles mode · c cycles category · Esc closes".into()))
            .child(super::modals::hint_row(&t,"Off excludes · Ask requires approval · Auto skips approval · future runs; workflow/Gateway limits still apply".into()));
        root.build()
    });
}
