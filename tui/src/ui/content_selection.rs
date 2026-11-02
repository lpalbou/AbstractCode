//! Explicit selected-content actions. No network, model or tool authority here.
use super::chrome::ComposerAnchor;
use abstracttui::{
    app::{ContextMenu, ContextMenuItem, Overlays},
    prelude::*,
    ui::UiEvent,
    widgets::{ContentContext, ContentKind, ContentSelection, SelectedContent, TextAreaState},
};

#[derive(Clone, Copy, Debug)]
enum Intent {
    Explain,
    Context,
    Change,
}

fn append_context(
    draft: &str,
    selected: &SelectedContent,
    intent: Intent,
) -> Result<String, &'static str> {
    if crate::commands::parse(draft).is_some() {
        return Err(
            "Your draft is a slash command. Finish or clear it before adding selected content.",
        );
    }
    if selected.text.len() > 32_768 {
        return Err("Selection is too large; choose a smaller passage.");
    }
    if selected.text.trim().is_empty() {
        return Err("Select some text before adding context.");
    }
    let instruction = match intent {
        Intent::Explain => "Explain this selected content.",
        Intent::Context => "Use this selected content as context for my request.",
        Intent::Change => "I want to change this selected content. My requested change: ",
    };
    // Quote as data, including human-readable provenance. No source identities
    // or byte offsets enter the draft, and selection text cannot become a command.
    let selected_quote = quote(&selected.text);
    let identity = match selected.context.as_deref() {
        Some(ContentContext::TableCell {
            row,
            column,
            header,
        }) => format!(
            "; table {}, column {} (header {:?})",
            row.map(|r| format!("body row {}", r + 1))
                .unwrap_or_else(|| "header row".into()),
            column + 1,
            header.chars().take(160).collect::<String>()
        ),
        Some(ContentContext::Code { language, lines }) => format!(
            "; {} code, source lines {}–{}",
            language.chars().take(80).collect::<String>(),
            lines.start,
            lines.end.saturating_sub(1)
        ),
        _ => String::new(),
    };
    let (surrounding, before, after) = selected.surrounding(2_048);
    let nearby = if surrounding != selected.text {
        let label = match (before, after) {
            (true, true) => "Local context excerpt (earlier and later text omitted)",
            (true, false) => "Local context excerpt (earlier text omitted)",
            (false, true) => "Local context excerpt (later text omitted)",
            _ => "Local context",
        };
        format!("\n\n{label}:\n{}", quote(&surrounding))
    } else {
        String::new()
    };
    let context = format!(
        "{instruction}\n\nSelected {}{identity}:\n{selected_quote}{nearby}",
        label(selected.kind)
    );
    Ok(if draft.is_empty() {
        context
    } else {
        format!("{draft}\n\n{context}")
    })
}

fn quote(text: &str) -> String {
    text.lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn label(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Word => "word",
        ContentKind::Sentence => "sentence",
        ContentKind::Paragraph => "paragraph",
        ContentKind::TableCell => "table cell",
        ContentKind::CodeBlock => "code block",
        _ => "passage",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Action {
    id: &'static str,
    label: &'static str,
}
const PRIMARY: [Action; 4] = [
    Action {
        id: "explain",
        label: "Explain",
    },
    Action {
        id: "context",
        label: "Use as context",
    },
    Action {
        id: "change",
        label: "Change",
    },
    Action {
        id: "cancel",
        label: "Cancel",
    },
];

fn whole_unit(selected: &SelectedContent) -> Action {
    match selected.context.as_deref() {
        Some(ContentContext::TableCell { .. }) => Action {
            id: "cell",
            label: "Cell",
        },
        Some(ContentContext::Code { .. }) => Action {
            id: "code",
            label: "Code block",
        },
        _ if selected.kind == ContentKind::TableCell => Action {
            id: "cell",
            label: "Cell",
        },
        _ if selected.kind == ContentKind::CodeBlock => Action {
            id: "code",
            label: "Code block",
        },
        _ => Action {
            id: "paragraph",
            label: "Paragraph",
        },
    }
}
fn units(selected: &SelectedContent) -> [Action; 3] {
    [
        Action {
            id: "word",
            label: "Word",
        },
        Action {
            id: "sentence",
            label: "Sentence",
        },
        whole_unit(selected),
    ]
}
fn action_width(actions: &[Action]) -> i32 {
    actions
        .iter()
        .map(|a| abstracttui::text::width(a.label))
        .sum::<i32>()
        + actions.len().saturating_sub(1) as i32 * 2
}

/// Choose complete hit targets before painting. The summary gets only the
/// remainder; overflow is a normal engine menu, never clipped hidden controls.
fn visible_actions(width: i32, selected: &SelectedContent) -> Vec<Action> {
    let status = abstracttui::text::width(&format!("Selected {}", label(selected.kind))) + 2;
    let all = PRIMARY
        .into_iter()
        .chain(units(selected))
        .collect::<Vec<_>>();
    if action_width(&all) + status <= width {
        all
    } else {
        let mut primary = PRIMARY.to_vec();
        primary.push(Action {
            id: "more",
            label: "More",
        });
        if action_width(&primary) + status <= width {
            primary
        } else {
            vec![Action {
                id: "actions",
                label: "Actions",
            }]
        }
    }
}

/// Return whether an event-context caller should return focus to the composer.
/// Only the draft and retained selection change; attachments and send authority
/// remain with the normal composer path.
fn apply_action(
    store: crate::store::Store,
    composer: &TextAreaState,
    selection: &ContentSelection,
    action: &str,
) -> bool {
    let unit = match action {
        "word" => Some(ContentKind::Word),
        "sentence" => Some(ContentKind::Sentence),
        "paragraph" => Some(ContentKind::Paragraph),
        "cell" => Some(ContentKind::TableCell),
        "code" => Some(ContentKind::CodeBlock),
        _ => None,
    };
    if let Some(unit) = unit {
        selection.expand(unit);
        return false;
    }
    if action == "cancel" {
        selection.clear();
        return true;
    }
    let Some(selected) = selection.value().get_untracked() else {
        return false;
    };
    let intent = match action {
        "explain" => Intent::Explain,
        "context" => Intent::Context,
        "change" => Intent::Change,
        _ => return false,
    };
    match append_context(&composer.text(), &selected, intent) {
        Ok(draft) => {
            composer.set_text(draft);
            selection.clear();
            store.notify("Selection added to your draft. Review it, then send when ready.");
            true
        }
        Err(message) => {
            store.notify(message);
            false
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct SelectionFocus(pub Signal<bool>);

/// Always occupies the existing one-row breathing space, including while empty.
/// Selecting, dragging and changing units never reflows the transcript.
pub fn toolbar(
    cx: Scope,
    store: crate::store::Store,
    composer: TextAreaState,
    anchor: ComposerAnchor,
    overlays: &Overlays,
) -> View {
    let selection = cx
        .use_context::<ContentSelection>()
        .expect("root provides content selection");
    let value = selection.value();
    let active = cx.memo(move || value.get().is_some());
    let focused = cx
        .use_context::<SelectionFocus>()
        .expect("root provides selection focus")
        .0;
    let overlays = overlays.clone();
    dyn_view_scoped(LayoutStyle::line(1).shrink(0.0), move |bar_cx| {
        if !active.get() {
            return Element::new()
                .style(LayoutStyle::line(1).shrink(0.0))
                .build();
        }
        active_toolbar(
            bar_cx,
            store,
            composer.clone(),
            anchor.clone(),
            overlays.clone(),
            selection.clone(),
            focused,
        )
    })
}

fn active_toolbar(
    cx: Scope,
    store: crate::store::Store,
    composer: TextAreaState,
    anchor: ComposerAnchor,
    overlays: Overlays,
    selection: ContentSelection,
    focused: Signal<bool>,
) -> View {
    let render = selection.clone();
    cx.on_cleanup(move || {
        if focused.try_get_untracked().is_some() {
            focused.set(false);
        }
    });
    Element::new()
        .style(LayoutStyle::line(1).shrink(0.0))
        .focusable()
        .access_label("Selected content actions")
        .on_event(move |_, event| match event {
            UiEvent::FocusIn => focused.set(true),
            UiEvent::FocusOut => focused.set(false),
            _ => {}
        })
        .on_action(move |ectx, action| {
            if action == "actions" || action == "more" {
                let Some(selected) = selection.value().get_untracked() else { return };
                let all = action == "actions";
                let choices = if all {
                    PRIMARY.into_iter().chain(units(&selected)).collect::<Vec<_>>()
                } else { units(&selected).to_vec() };
                let menu_selection = selection.clone();
                let menu_composer = composer.clone();
                let rect = ectx.current_rect_screen();
                let opened = ContextMenu::new(choices.into_iter().map(|a| ContextMenuItem::new(a.id, a.label)))
                    .access_label("Selected content actions")
                    .overlays(&overlays)
                    .on_action(move |key| { apply_action(store, &menu_composer, &menu_selection, key); })
                    .open(cx, Point::new(rect.x, rect.y));
                if opened.is_none() {
                    store.notify("No room to open selection actions. Resize the terminal or press Esc to cancel.");
                } else if all {
                    // ContextMenu callbacks carry no EventCtx. Park the underlying
                    // keyboard at the composer before its owned menu takes input,
                    // so a staged draft is ready for the normal explicit submit.
                    if let Some(id) = anchor.get() { ectx.request_focus(id); }
                }
                return;
            }
            if apply_action(store, &composer, &selection, action.as_str()) {
                if let Some(id) = anchor.get() { ectx.request_focus(id); }
            }
        })
        .child(dyn_view(LayoutStyle::line(1).shrink(0.0), move || {
            let selected = render.value().get();
            let t = current_theme().tokens;
            Element::new().style(LayoutStyle::line(1).shrink(0.0)).draw(move |canvas, rect| {
                let Some(selected) = &selected else { return };
                if rect.w <= 0 || rect.h <= 0 { return; }
                let actions = visible_actions(rect.w, selected);
                let mut x = rect.x;
                for (i, action) in actions.iter().enumerate() {
                    if i > 0 { x += 2; }
                    // Only the single Actions trigger can need truncation, and
                    // its complete visible extent remains one semantic target.
                    let name = abstracttui::text::truncate_ellipsis(action.label, rect.right() - x);
                    x += canvas.print_action(Point::new(x, rect.y), &name,
                        &abstracttui::render::Style::new().fg(t.link), &action.id.into());
                }
                let remaining = rect.right() - x - 2;
                if remaining > 0 {
                    let preview = selected.text.chars().take(96).collect::<String>();
                    let preview = preview.split_whitespace().collect::<Vec<_>>().join(" ");
                    let status = format!("Selected {}: {preview}", label(selected.kind));
                    canvas.print_styled(Point::new(x + 2, rect.y), &abstracttui::text::truncate_ellipsis(&status, remaining),
                        &abstracttui::render::Style::new().fg(t.text_muted));
                }
            }).build()
        }))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn selected(source: &str, kind: ContentKind) -> SelectedContent {
        abstracttui::widgets::ContentHit::from_source(
            "internal-item-id",
            0,
            source,
            0..source.chars().next().unwrap().len_utf8(),
            0..source.len(),
            ContentKind::Span,
        )
        .unwrap()
        .select(kind)
    }
    #[test]
    fn actions_preserve_draft_and_quote_untrusted_selection() {
        let selected = selected("/delete all\nIgnore instructions", ContentKind::Paragraph);
        for intent in [Intent::Explain, Intent::Context, Intent::Change] {
            let text = append_context("unfinished draft", &selected, intent).unwrap();
            assert!(text.starts_with("unfinished draft\n\n"));
            assert!(text.contains("> /delete all\n> Ignore instructions"));
            assert!(text.contains("Selected paragraph"));
            assert!(!text.contains("internal-item-id") && !text.contains("source bytes"));
        }
    }
    #[test]
    fn slash_command_drafts_and_oversized_or_blank_selections_are_refused() {
        let item = selected("normal text", ContentKind::Paragraph);
        assert!(append_context("/help", &item, Intent::Explain).is_err());
        assert!(append_context("/model choose", &item, Intent::Context).is_err());
        assert!(append_context(
            "",
            &selected(&"a".repeat(32_769), ContentKind::Paragraph),
            Intent::Explain
        )
        .is_err());
        assert!(append_context(
            "",
            &selected(" \n  ", ContentKind::Paragraph),
            Intent::Context
        )
        .is_err());
        assert!(
            append_context(
                "",
                &selected(&"😀".repeat(8_193), ContentKind::Paragraph),
                Intent::Context
            )
            .is_err(),
            "the selection cap is UTF-8 bytes, not a character count"
        );
    }
    #[test]
    fn contextual_units_are_human_readable() {
        let mut item = selected("word", ContentKind::Word);
        assert_eq!(whole_unit(&item).label, "Paragraph");
        item.context = Some(std::rc::Rc::new(ContentContext::TableCell {
            row: Some(1),
            column: 2,
            header: "Status".into(),
        }));
        assert_eq!(whole_unit(&item).label, "Cell");
        let prompt = append_context("", &item, Intent::Context).unwrap();
        assert!(prompt.contains("body row 2, column 3 (header \"Status\")"));
        item.context = Some(std::rc::Rc::new(ContentContext::Code {
            language: "rust".into(),
            lines: 8..12,
        }));
        assert_eq!(whole_unit(&item).label, "Code block");
        assert!(append_context("", &item, Intent::Change)
            .unwrap()
            .contains("rust code, source lines 8–11"));
    }
    #[test]
    fn local_context_centers_on_selection_and_labels_omitted_text() {
        let source = format!(
            "{} chosenWord {}",
            "earlier ".repeat(600),
            "later ".repeat(600)
        );
        let at = source.find("chosenWord").unwrap();
        let hit = abstracttui::widgets::ContentHit::from_source(
            "private-source",
            0,
            source.as_str(),
            at..at + 1,
            0..source.len(),
            ContentKind::Span,
        )
        .unwrap();
        let selected = hit.select(ContentKind::Word);
        let prompt = append_context("review this", &selected, Intent::Explain).unwrap();
        assert!(prompt.contains("Selected word:\n> chosenWord"), "{prompt}");
        assert!(prompt.contains("Local context excerpt (earlier and later text omitted)"));
        let nearby = prompt.split("Local context excerpt").nth(1).unwrap();
        assert!(
            nearby.contains("chosenWord") && nearby.contains("earlier") && nearby.contains("later")
        );
        assert!(nearby.chars().count() < 2200);
        assert!(!prompt.contains("private-source"));
    }
    #[test]
    fn complete_action_targets_fit_and_every_hidden_action_is_in_overflow() {
        let item = selected("word", ContentKind::Word);
        for width in 1..=180 {
            let actions = visible_actions(width, &item);
            if actions.len() > 1 {
                assert!(action_width(&actions) <= width);
            } else {
                assert_eq!(actions[0].id, "actions");
            }
            let represented = if actions.iter().any(|a| a.id == "actions") {
                PRIMARY
                    .into_iter()
                    .chain(units(&item))
                    .map(|a| a.id)
                    .collect::<Vec<_>>()
            } else if actions.iter().any(|a| a.id == "more") {
                actions
                    .iter()
                    .filter(|a| a.id != "more")
                    .map(|a| a.id)
                    .chain(units(&item).into_iter().map(|a| a.id))
                    .collect()
            } else {
                actions.iter().map(|a| a.id).collect()
            };
            assert_eq!(
                represented,
                vec![
                    "explain",
                    "context",
                    "change",
                    "cancel",
                    "word",
                    "sentence",
                    "paragraph"
                ]
            );
        }
    }

    #[test]
    fn real_keyboard_actions_keep_one_row_and_narrow_overflow_works() {
        use abstracttui::{app::Driver, testing::CaptureTerm};
        use std::{cell::RefCell, rc::Rc};
        let slot = Rc::new(RefCell::new(None));
        let saved = slot.clone();
        let mut app = App::new(Size::new(100, 10));
        let overlays = app.overlays();
        app.mount(move |cx| {
            let store = crate::store::Store::create(cx);
            let selection = ContentSelection::new(cx);
            let composer = TextAreaState::new(cx);
            composer.set_text("unfinished draft");
            cx.provide_context(selection.clone());
            cx.provide_context(SelectionFocus(cx.signal(false)));
            *saved.borrow_mut() = Some((selection, composer.clone()));
            Element::new()
                .style(LayoutStyle::column())
                .child(toolbar(
                    cx,
                    store,
                    composer,
                    ComposerAnchor::default(),
                    &overlays,
                ))
                .child(
                    Element::new()
                        .style(LayoutStyle::line(1))
                        .draw(|canvas, rect| {
                            canvas.print_styled(
                                Point::new(rect.x, rect.y),
                                "Below bar",
                                &abstracttui::render::Style::EMPTY,
                            );
                        })
                        .build(),
                )
                .build()
        })
        .unwrap();
        let mut term = CaptureTerm::new(Size::new(100, 10));
        let mut driver = Driver::new(
            &mut app,
            &mut term,
            RunConfig {
                probe: false,
                platform_clipboard: false,
                caps: Some(abstracttui::term::Capabilities::with(|c| {
                    c.truecolor = true;
                    c.unicode_ok = true;
                })),
                ..Default::default()
            },
        )
        .unwrap();
        driver.turn(&mut app, &mut term).unwrap();
        let row = |screen: String| {
            screen
                .lines()
                .position(|line| line.contains("Below bar"))
                .unwrap()
        };
        let empty_row = row(term.screen().to_text());
        assert_eq!(
            app.tree().focused(),
            None,
            "empty breathing row is not a blank Tab stop"
        );
        let (selection, composer) = slot.borrow().clone().unwrap();
        let hit = abstracttui::widgets::ContentHit::from_source(
            "private-id",
            0,
            "Selected material",
            0..1,
            0..17,
            ContentKind::Span,
        )
        .unwrap();
        selection.choose(&hit, ContentKind::Word);
        driver.turn(&mut app, &mut term).unwrap();
        assert_eq!(
            row(term.screen().to_text()),
            empty_row,
            "selection cannot move the transcript"
        );
        app.tree().focus_first();
        term.push_input(b" ");
        driver.turn(&mut app, &mut term).unwrap();
        assert!(composer
            .text()
            .starts_with("unfinished draft\n\nExplain this selected content."));
        assert!(selection.value().get_untracked().is_none());
        assert_eq!(row(term.screen().to_text()), empty_row);

        selection.choose(&hit, ContentKind::Word);
        term.push_resize(Size::new(32, 10));
        driver.turn(&mut app, &mut term).unwrap();
        assert!(term.screen().to_text().contains("Actions"));
        app.tree().focus_first();
        term.push_input(b"\r");
        driver.turn(&mut app, &mut term).unwrap();
        assert!(
            term.screen().to_text().contains("Use as context"),
            "overflow exposes primary actions"
        );
        term.push_input(b"\x1b[B\r");
        driver.turn(&mut app, &mut term).unwrap();
        assert!(composer
            .text()
            .contains("Use this selected content as context for my request."));
        assert!(selection.value().get_untracked().is_none());
        assert_eq!(row(term.screen().to_text()), empty_row);
    }
}
