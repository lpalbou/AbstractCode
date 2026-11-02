//! Host adapter for the engine's reusable chat panel; inference stays on Gateway.
use crate::{docs_chat::ChatScope, runner::Cmd, store::Store, ui::UiCtx};
use abstracttui::{
    app::{Drawer, DrawerEdge, DrawerHandle, DrawerSize},
    prelude::*,
    widgets::{ChatMessage, ChatPanel, ChatRole},
};

pub fn run_reference(store: Store) -> String {
    crate::assistant_context::capture(store)
}

fn send_question(store: Store, ctx: &UiCtx, scope: ChatScope, question: &str, append: bool) {
    let question = question.trim();
    let signal = scope.chat(store);
    if question.is_empty() || signal.with_untracked(|s| s.busy) {
        return;
    }
    let mut chat = signal.get_untracked();
    {
        let sid = store.session_id.get_untracked();
        if chat.source_session != sid {
            chat = Default::default();
            chat.source_session = sid;
        }
    }
    chat.operation = chat.operation.wrapping_add(1);
    chat.busy = true;
    chat.error.clear();
    chat.run_id = None;
    if append {
        chat.messages.push(("user".into(), question.into()));
    }
    let corpus = if scope != ChatScope::Docs {
        Some(String::new())
    } else {
        chat.corpus.clone()
    };
    let cmd = Cmd::AssistantAsk {
        scope,
        session: chat.session_id.clone(),
        operation: chat.operation,
        question: question.into(),
        corpus,
        context: run_reference(store),
    };
    signal.set(chat);
    if append {
        scope.draft(store).set(String::new());
    }
    if !ctx.send(cmd) {
        signal.update(|s| {
            s.busy = false;
            s.error = "The Gateway worker is unavailable. Your question was not sent.".into();
        });
        scope.draft(store).set(question.into());
    }
}
fn conversation_view(root: Scope, cx: Scope, store: Store, ctx: UiCtx, scope: ChatScope) -> View {
    let chat = scope.chat(store);
    let draft = scope.draft(store);
    {
        let sid = store.session_id.get();
        if chat.with_untracked(|s| s.source_session != sid) {
            chat.set(crate::docs_chat::DocsChat {
                source_session: sid,
                ..Default::default()
            });
        }
    }
    let label = if scope == ChatScope::Docs {
        "Documentation"
    } else {
        "Companion"
    };
    let intro = if scope == ChatScope::Docs {
        "Ask about AbstractCode documentation, features and settings. Recent conversation context is available when relevant.".to_string()
    } else {
        "Your current conversation. Recent requests and replies are included; tool details are excluded. Answers do not change the run.".to_string()
    };
    let messages = move || {
        chat.with(|s| {
            let mut rows = s
                .messages
                .iter()
                .map(|(role, text)| {
                    if role == "user" {
                        ChatMessage::user(text)
                    } else {
                        ChatMessage::assistant(text)
                    }
                })
                .collect::<Vec<_>>();
            if scope == ChatScope::Run {
                if let Some((source, text)) = store.companion_advice.get() {
                    if source == store.session_id.get() {
                        rows.push(ChatMessage::new(ChatRole::Notice, text).author("Companion"));
                    }
                }
            }
            if !s.error.is_empty() {
                rows.push(ChatMessage::new(ChatRole::Error, s.error.clone()));
            }
            rows
        })
    };
    let send = ctx.clone();
    let panel = ChatPanel::new(draft, messages, move |q| {
        send_question(store, &send, scope, q, true)
    })
    .send_label("↑")
    .busy(move || chat.with(|s| s.busy))
    .assistant_label(label)
    .placeholder(if scope == ChatScope::Docs {
        "Ask a documentation question…"
    } else {
        "Ask your companion…"
    })
    .empty_message(if scope == ChatScope::Docs {
        "How can I help you use AbstractCode?"
    } else {
        "Ask what happened, why the run is waiting, or what an error means."
    });
    let actions = move |scx: Scope| {
        let busy = chat.with(|s| s.busy);
        let failed = chat.with(|s| !s.error.is_empty());
        let mut row = Element::new()
            .style(LayoutStyle::row().gap(2))
            .child(Tooltip::attach(
                scx,
                &ctx.overlays,
                "New conversation",
                std::time::Duration::from_millis(400),
                Button::new("＋")
                    .disabled(busy)
                    .on_click(move || {
                        let fresh = crate::docs_chat::DocsChat {
                            source_session: store.session_id.get_untracked(),
                            ..Default::default()
                        };
                        chat.set(fresh);
                        draft.set(String::new());
                    })
                    .element(scx, &use_theme(scx).get_untracked().tokens)
                    .access_label("New conversation")
                    .build(),
            ));
        let mic_ctx = ctx.clone();
        let settings_ctx = ctx.clone();
        let mic = super::companion_voice::is_recording_for(store, scope);
        row = row.child(Tooltip::attach(
            scx,
            &ctx.overlays,
            if scope == ChatScope::Docs {
                "Voice documentation question · press again to transcribe"
            } else {
                "Voice question to your companion · press again to transcribe"
            },
            std::time::Duration::from_millis(400),
            Button::new(if mic { "■" } else { "●" })
                .on_click(move || super::companion_voice::toggle_microphone(store, &mic_ctx, scope))
                .element(scx, &use_theme(scx).get_untracked().tokens)
                .access_label(if scope == ChatScope::Docs {
                    "Voice documentation question"
                } else {
                    "Voice question to companion"
                })
                .build(),
        ));
        if scope == ChatScope::Run {
            row = row.child(Tooltip::attach(
                scx,
                &ctx.overlays,
                "Companion appearance, comments and voice",
                std::time::Duration::from_millis(400),
                Button::new("⚙")
                    .on_click(move || super::companion_view::settings(root, store, &settings_ctx))
                    .element(scx, &use_theme(scx).get_untracked().tokens)
                    .access_label("Companion settings")
                    .build(),
            ));
        }
        if failed && !busy {
            let retry = ctx.clone();
            row = row.child(Tooltip::attach(
                scx,
                &ctx.overlays,
                "Retry question",
                std::time::Duration::from_millis(400),
                Button::new("↻")
                    .on_click(move || {
                        let q = chat.with_untracked(|s| {
                            s.messages
                                .iter()
                                .rev()
                                .find(|(role, _)| role == "user")
                                .map(|(_, q)| q.clone())
                        });
                        if let Some(q) = q {
                            send_question(store, &retry, scope, &q, false);
                        }
                    })
                    .element(scx, &use_theme(scx).get_untracked().tokens)
                    .access_label("Retry question")
                    .build(),
            ));
        }
        row.build()
    };
    Element::new()
        .style(LayoutStyle::column().grow(1.0).gap(1))
        .child(
            MarkdownView::new(intro)
                .layout(LayoutStyle::default().shrink(0.0))
                .view(cx),
        )
        .child(panel.actions(actions).view(cx))
        .build()
}

// The two features share presentation, not an in-drawer mode switch. Keep both
// handles alive for the root scope while their bound signals follow entry routing.
#[derive(Clone)]
struct SideDrawers {
    _handles: Vec<DrawerHandle>,
}

pub fn install(cx: Scope, store: Store, ctx: &UiCtx) {
    let documentation_open = cx.signal(false);
    let companion_open = cx.signal(false);
    let mut handles = Vec::new();
    for (scope, open, title) in [
        (
            ChatScope::Docs,
            documentation_open,
            "Documentation · AbstractCode",
        ),
        (ChatScope::Run, companion_open, "Companion · AbstractCode"),
    ] {
        let context = ctx.clone();
        let handle = Drawer::new(DrawerEdge::Right)
            .size(DrawerSize::Cells(84))
            .title(title)
            .motion(std::time::Duration::ZERO)
            .bind(open)
            .overlays(&ctx.overlays)
            .on_close(move |_| {
                // Switching entry points closes the previous drawer; only the
                // currently selected feature may close the shared open state.
                if store.help_scope.try_get_untracked() == Some(scope)
                    && store.docs_open.try_get_untracked() == Some(true)
                {
                    store.docs_open.set(false);
                }
            })
            .install(cx, move |_dcx| {
                let context = context.clone();
                Element::new()
                    .style(LayoutStyle::column().grow(1.0).padding(Edges::all(1)))
                    .child(dyn_view_scoped(
                        LayoutStyle::column().grow(1.0).basis(Dimension::Cells(0)),
                        move |scx| conversation_view(cx, scx, store, context.clone(), scope),
                    ))
                    .build()
            });
        handles.push(handle);
    }
    cx.provide_context(SideDrawers { _handles: handles });
    cx.effect(move || {
        let open = store.docs_open.get();
        let scope = store.help_scope.get();
        // Close the other feature before opening the selected one on this edge.
        if scope == ChatScope::Docs {
            companion_open.set_if_changed(false);
            documentation_open.set_if_changed(open);
        } else {
            documentation_open.set_if_changed(false);
            companion_open.set_if_changed(open && scope == ChatScope::Run);
        }
    });
}
