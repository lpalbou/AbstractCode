//! Labelled terminal equivalents of the WUI Docs/Appearance/About/account controls.
use crate::{
    runner::Cmd,
    store::AccountIdentity,
    store::{Conn, Store},
    ui::{chrome, UiCtx},
};
use abstracttui::{
    app::{ContextMenu, ContextMenuItem},
    prelude::*,
    text,
};

pub struct Action {
    pub id: &'static str,
    pub label: String,
    pub rect: Rect,
}

/// One layout authority for drawing and popup placement. Hit testing belongs to ActionId.
pub fn right_plan(rect: Rect, account: &AccountIdentity, conn: &Conn) -> Vec<Action> {
    let orb = match conn {
        Conn::Ok => "●",
        Conn::Unknown => "◌",
        Conn::Down(..) => "✗",
    };
    let user = if account.user_id.is_empty() {
        "Account"
    } else {
        &account.user_id
    };
    let account = format!("◎ {} {orb} ▾", text::truncate_ellipsis(user, 16));
    let items = if rect.w >= 140 {
        vec![
            (chrome::ACT_DOCS, "▤ Docs".into()),
            (chrome::ACT_THEME, "◐ Theme".into()),
            (chrome::ACT_ABOUT, "i About".into()),
            (chrome::ACT_ACCOUNT, account),
        ]
    } else {
        vec![
            (chrome::ACT_MENU, "Menu ▾".into()),
            (chrome::ACT_ACCOUNT, account),
        ]
    };
    let total: i32 = items
        .iter()
        .map(|(_, s): &(&str, String)| text::width(s))
        .sum::<i32>()
        + (items.len() as i32 - 1) * 3;
    let mut x = (rect.right() - 1 - total).max(rect.x);
    items
        .into_iter()
        .map(|(id, label)| {
            let width = text::width(&label);
            let action = Action {
                id,
                label,
                rect: Rect::new(x, rect.y, width, 1),
            };
            x += width + 3;
            action
        })
        .collect()
}

pub fn open_tools(cx: Scope, store: Store, ctx: &UiCtx, anchor: Rect) {
    let context = ctx.clone();
    ContextMenu::new([
        ContextMenuItem::new("tools", "Tools").hint("choices for this session"),
        ContextMenuItem::new("skills", "Skills").hint(format!(
            "{} attached",
            store.selected_skills.with_untracked(|s| s.len())
        )),
        ContextMenuItem::new("mcp", "MCP servers").hint(format!(
            "{} declared by Gateway",
            store.mcp_servers.with_untracked(|s| s.len())
        )),
    ])
    .overlays(&ctx.overlays)
    .min_width(34)
    .on_action(move |key| match key {
        "tools" => {
            context.send(Cmd::LoadTools);
            super::modals::open_tools(cx, store, &context);
        }
        "skills" => {
            context.send(Cmd::LoadSkills);
            super::modals::open_skills(cx, store, &context);
        }
        "mcp" => {
            context.send(Cmd::LoadMcp);
            super::modals::open_mcp(cx, store, &context);
        }
        _ => {}
    })
    .open(cx, Point::new(anchor.x, anchor.bottom()));
}

pub fn open_account(cx: Scope, store: Store, ctx: &UiCtx, anchor: Rect) {
    ctx.send(Cmd::LoadAccount);
    let account = store.account.get_untracked();
    let mut items = vec![
        ContextMenuItem::new(
            "identity",
            if account.user_id.is_empty() {
                "Identity unavailable".into()
            } else {
                format!("Signed in as {}", account.user_id)
            },
        )
        .disabled(true),
        ContextMenuItem::new("host", format!("Gateway: {}", ctx.gateway_label)).disabled(true),
        ContextMenuItem::new("status", "Connection status"),
        ContextMenuItem::new("copy-user", "Copy user id").disabled(account.user_id.is_empty()),
        ContextMenuItem::new("disconnect", "Disconnect this terminal…"),
    ];
    if !account.tenant_id.is_empty() && account.tenant_id != "default" {
        items.insert(
            1,
            ContextMenuItem::new("tenant", format!("Tenant: {}", account.tenant_id)).disabled(true),
        );
    }
    if !account.error.is_empty() {
        items.insert(
            1,
            ContextMenuItem::new("identity-error", account.error).disabled(true),
        );
    }
    let context = ctx.clone();
    ContextMenu::new(items)
        .overlays(&ctx.overlays)
        .min_width(32)
        .on_action(move |key| match key {
            "status" => {
                super::chrome_action(cx, store, &context, chrome::ACT_CONNECTION);
            }
            "copy-user" => {
                abstracttui::app::selection::copy_to_clipboard(account.user_id.clone());
                store.notify("Copied user id");
            }
            "disconnect" => confirm_disconnect(cx, store, &context),
            _ => {}
        })
        .open(cx, Point::new(anchor.x, anchor.bottom()));
}

pub fn open_menu(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    anchor: Rect,
    theme: super::theme_menu::TriggerState,
) {
    let context = ctx.clone();
    ContextMenu::new([
        ContextMenuItem::new(chrome::ACT_WORKFLOW, "Workflow"),
        ContextMenuItem::new(chrome::ACT_MODEL, "Model"),
        ContextMenuItem::new(chrome::ACT_DOCS, "Docs assistant"),
        ContextMenuItem::new(chrome::ACT_THEME, "Theme"),
        ContextMenuItem::new(chrome::ACT_ABOUT, "About AbstractCode"),
        ContextMenuItem::new(chrome::ACT_CONNECTION, "Connection status"),
    ])
    .overlays(&ctx.overlays)
    .on_action(move |id| {
        if id == chrome::ACT_THEME {
            super::theme_menu::open(cx, &context, theme, anchor);
        } else {
            super::chrome_action(cx, store, &context, id);
        }
    })
    .open(cx, Point::new(anchor.x, anchor.bottom()));
}

fn confirm_disconnect(cx: Scope, store: Store, ctx: &UiCtx) {
    let context = ctx.clone();
    let escape = ctx.clone();
    ctx.open_blocking_modal(cx, Size::new(64, 12), move |mcx| {
        let stay = context.clone();
        Element::new().style(LayoutStyle::column().gap(1)).shortcut(KeyChord::plain(Key::Escape),move |_| escape.close_modal())
            .child(MarkdownView::new("**Disconnect this terminal?**\n\nClear the saved sign-in and close this client. Runs already on the Gateway continue. Credentials supplied through your shell can still apply on the next launch.").view(mcx))
            .child(Button::new("Stay connected").on_click(move || stay.close_modal()).view(mcx))
            .child(Button::new("Disconnect and exit").on_click(move || {
                match crate::config::write_login(None,None) {
                    Ok(_) => {context.send(Cmd::StopFollows);context.quitter.quit();},
                    Err(e) => store.notify(format!("Could not clear saved sign-in: {e}")),
                }
            }).view(mcx)).build()
    });
}
