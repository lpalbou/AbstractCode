//! A right-side docs conversation; all inference stays in the Gateway docs-qa workflow.
use crate::{runner::Cmd, store::Store, ui::UiCtx};
use abstracttui::app::{Drawer, DrawerEdge, DrawerSize};
use abstracttui::prelude::*;

fn submit(store: Store, ctx: &UiCtx, question: &str) {
    send_question(store, ctx, question, true);
}

fn send_question(store: Store, ctx: &UiCtx, question: &str, append: bool) {
    let question = question.trim();
    if question.is_empty() || store.docs_chat.with_untracked(|s| s.busy) {
        return;
    }
    let mut chat = store.docs_chat.get_untracked();
    chat.operation = chat.operation.wrapping_add(1);
    chat.busy = true;
    chat.error.clear();
    chat.run_id = None;
    if append {
        chat.messages.push(("user".into(), question.into()));
    }
    let cmd = Cmd::DocsAsk {
        session: chat.session_id.clone(),
        operation: chat.operation,
        question: question.into(),
        corpus: chat.corpus.clone(),
    };
    store.docs_chat.set(chat);
    if append {
        store.docs_draft.set(String::new());
    }
    if !ctx.send(cmd) {
        store.docs_chat.update(|s| {
            s.busy = false;
            s.error = "The Gateway worker is unavailable. Your question was not sent.".into();
        });
        store.docs_draft.set(question.into());
    }
}

pub fn install(cx: Scope, store: Store, ctx: &UiCtx) {
    let context = ctx.clone();
    let handle = Drawer::new(DrawerEdge::Right).size(DrawerSize::Cells(76))
        .title("Docs assistant · AbstractCode").motion(std::time::Duration::ZERO)
        .bind(store.docs_open).overlays(&ctx.overlays)
        .install(cx, move |dcx| {
            let t = abstracttui::app::current_theme().tokens;
            let submit_ctx = context.clone();
            let button_ctx = context.clone();
            let follow = dcx.signal(true);
            Element::new().style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
                .child(MarkdownView::new("Ask about AbstractCode. Answers use the Gateway's documentation corpus; this is separate from your coding task.").view(dcx))
                .child(dyn_view_scoped(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)), move |scx| {
                    let chat = store.docs_chat.get();
                    let mut body = String::new();
                    for (role,text) in &chat.messages {
                        body.push_str(if role == "user" { "**You**\n\n" } else { "**Docs assistant**\n\n" });
                        body.push_str(text);body.push_str("\n\n");
                    }
                    if chat.busy { body.push_str("Answering on the Gateway…\n\n"); }
                    if !chat.error.is_empty() { body.push_str(&format!("**{}:** {}\n\n", if chat.busy {"Connection notice"} else {"Could not answer"},chat.error)); }
                    Scroll::new(MarkdownView::new(body).view(scx)).follow_tail(follow).scrollbar_auto_hide(true)
                        .layout(LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0))).view(scx)
                }))
                .child(TextInput::new().value(store.docs_draft).placeholder("Ask a documentation question…")
                    .placeholder_while_focused(true).layout(LayoutStyle::line(1))
                    .on_submit(move |q| submit(store,&submit_ctx,q)).element(dcx,&t).autofocus().build())
                .child(dyn_view_scoped(LayoutStyle::line(1), move |scx| {
                    let busy = store.docs_chat.with(|s| s.busy);
                    let context = button_ctx.clone();
                    let retry_ctx=button_ctx.clone();
                    let error=store.docs_chat.with(|s|!s.error.is_empty());
                    let mut controls=Element::new().style(LayoutStyle::row().gap(2))
                        .child(Button::new("Send").disabled(busy).on_click(move || {
                            let q = store.docs_draft.get_untracked();submit(store,&context,&q);
                        }).view(scx))
                        .child(Button::new("New conversation").disabled(busy).on_click(move || {
                            store.docs_chat.set(Default::default());store.docs_draft.set(String::new());
                        }).view(scx))
                        ;
                    if error && !busy {
                        controls=controls.child(Button::new("Retry").on_click(move || {
                            let q=store.docs_chat.with_untracked(|s|s.messages.iter().rev().find(|(role,_)|role=="user").map(|(_,text)|text.clone()));
                            if let Some(q)=q {send_question(store,&retry_ctx,&q,false);}
                        }).view(scx));
                    }
                    controls.build()
                }))
                .build()
        });
    cx.provide_context(handle);
}
