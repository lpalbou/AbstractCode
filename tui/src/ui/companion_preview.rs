//! Silent, local appearance preview. Only “Use this” persists a choice.
use super::{companion_view, modals, UiCtx};
use crate::store::Store;
use abstracttui::prelude::*;
use std::time::Duration;

const PHASES: [AvatarPose; 7] = [
    AvatarPose::Ready,
    AvatarPose::Listening,
    AvatarPose::Thinking,
    AvatarPose::Working,
    AvatarPose::Speaking,
    AvatarPose::Waiting,
    AvatarPose::Error,
];
const FRAME_MS: u64 = 84;
const PHASE_MS: u64 = 3500;

fn synthetic_level(elapsed_ms: u64) -> f32 {
    let t = elapsed_ms as f32 / 1000.0;
    let phrase = t % 3.6;
    if phrase >= 2.6 {
        return 0.0;
    }
    let envelope = (phrase / 0.25).min(1.0) * ((2.6 - phrase) / 0.35).min(1.0);
    let syllable = (t * std::f32::consts::TAU * 4.4).sin().max(0.0).powf(0.7);
    (envelope * syllable * (0.65 + 0.35 * (t * 1.7).sin())).clamp(0.0, 1.0)
}

pub(super) fn open(cx: Scope, store: Store, ctx: &UiCtx) {
    let current = companion_view::prefs(ctx);
    let context = ctx.clone();
    // The engine clamps and re-clamps to the viewport. Reserve no extra chrome
    // rows here: this focus-trapped preview should use the same space whether
    // opened in a short terminal or resized there from a larger one.
    let size = Size::new(96, 42);
    ctx.open_modal(cx, size, move |mcx| {
        let tokens = abstracttui::app::current_theme().tokens;
        // Overlay is an alpha wash, not an opaque material ground. Resolve it
        // once and use the same opaque surface for the panel and its artwork.
        let preview_tokens = TokenSet {
            bg: tokens.overlay.over(tokens.bg),
            ..tokens
        };
        let style = mcx.signal(if current.style == "hover" {
            AvatarStyle::Hover
        } else {
            AvatarStyle::Halo
        });
        let saved_style = style.get_untracked();
        let phase = mcx.signal(1usize);
        let pose = mcx.signal(PHASES[1]);
        let automatic = mcx.signal(current.animate);
        let animated = mcx.signal(current.animate);
        let level = mcx.signal(0.55f32);
        // Owned by the modal, including its synthetic voice envelope. There is
        // no recorder, speech synthesis, workflow command, or inference here.
        mcx.effect(move || {
            pose.set_if_changed(PHASES[phase.get()]);
        });
        mcx.effect(move || {
            let voice_phase = matches!(
                PHASES[phase.get()],
                AvatarPose::Listening | AvatarPose::Speaking
            );
            if !animated.get() || (!automatic.get() && !voice_phase) {
                return;
            }
            let clock = mcx.child();
            let mut elapsed = 0u64;
            let mut phase_elapsed = 0u64;
            abstracttui::reactive::interval(clock, Duration::from_millis(FRAME_MS), move || {
                elapsed += FRAME_MS;
                if automatic.get_untracked() {
                    phase_elapsed += FRAME_MS;
                    if phase_elapsed >= PHASE_MS {
                        phase_elapsed = 0;
                        phase.update(|i| *i = (*i + 1) % PHASES.len());
                    }
                } else {
                    phase_elapsed = 0;
                }
                level.set_if_changed(synthetic_level(elapsed));
            });
            abstracttui::reactive::on_cleanup(move || clock.dispose());
        });

        let back = context.clone();
        let apply = context.clone();
        let cancel = context.clone();
        let mut root = Element::new()
            .style(LayoutStyle::column().grow(1.0))
            .draw(move |canvas, rect| canvas.fill(rect, ' ', tokens.text, preview_tokens.bg))
            .shortcut(KeyChord::plain(Key::Escape), move |_| {
                companion_view::settings(cx, store, &cancel)
            })
            .shortcut(KeyChord::plain(Key::Char('c')), move |_| {
                let play = !automatic.get_untracked();
                automatic.set(play);
                if play {
                    animated.set(true);
                }
            })
            .shortcut(KeyChord::plain(Key::Left), move |_| {
                automatic.set(false);
                phase.update(|i| *i = (*i + PHASES.len() - 1) % PHASES.len());
            })
            .shortcut(KeyChord::plain(Key::Right), move |_| {
                automatic.set(false);
                phase.update(|i| *i = (*i + 1) % PHASES.len());
            })
            .child(modals::title_row(
                &tokens,
                "Preview companion appearance".into(),
            ));
        for (i, key) in ['1', '2', '3', '4', '5', '6', '7'].into_iter().enumerate() {
            root = root.shortcut(KeyChord::plain(Key::Char(key)), move |_| {
                automatic.set(false);
                phase.set(i);
            });
        }
        let mut styles = Element::new().style(
            LayoutStyle::row()
                .height(Dimension::Cells(1))
                .shrink(0.0)
                .gap(2),
        );
        for (name, choice) in [
            ("Halo orb", AvatarStyle::Halo),
            ("Hover robot", AvatarStyle::Hover),
        ] {
            styles = styles.child(
                Button::new(name)
                    .on_click(move || {
                        style.set_if_changed(choice);
                    })
                    .view(mcx),
            );
        }
        root = root
            .child(styles.build())
            .child(dyn_view(LayoutStyle::line(1), move || {
                modals::hint_row(
                    &tokens,
                    format!(
                        "{} · {}",
                        match style.get() {
                            AvatarStyle::Hover => "Previewing Hover robot · warm glowing eyes",
                            _ => "Previewing Halo orb · a luminous corona",
                        },
                        if style.get() == saved_style {
                            "In use"
                        } else {
                            "Not applied"
                        }
                    ),
                )
            }));
        root = root
            .child(
                Element::new()
                    .style(
                        LayoutStyle::column()
                            .grow(1.0)
                            .basis(Dimension::Cells(0))
                            .min_h(0)
                            .align_items(Align::Center)
                            .justify(Justify::Center),
                    )
                    .child(
                        Avatar::new(pose)
                            .style_signal(style)
                            .level(level)
                            .animated(animated)
                            .show_label(false)
                            .layout(
                                LayoutStyle::default()
                                    .width(Dimension::Percent(1.0))
                                    .max_w(48)
                                    .height(Dimension::Percent(1.0))
                                    .max_h(25)
                                    .min_h(0),
                            )
                            .element(mcx, &preview_tokens)
                            .build(),
                    )
                    .build(),
            )
            .child(dyn_view(LayoutStyle::line(1), move || {
                modals::title_row(
                    &tokens,
                    format!(
                        "{} · {}{}",
                        PHASES[phase.get()].label(),
                        if automatic.get() { "cycling" } else { "held" },
                        if animated.get() {
                            ""
                        } else {
                            " · motion paused"
                        }
                    ),
                )
            }));
        for range in [0..4, 4..7] {
            let mut phases = Element::new().style(
                LayoutStyle::row()
                    .height(Dimension::Cells(1))
                    .shrink(0.0)
                    .gap(1),
            );
            for i in range {
                phases = phases.child(
                    Button::new(PHASES[i].label())
                        .on_click(move || {
                            automatic.set(false);
                            phase.set(i);
                        })
                        .view(mcx),
                );
            }
            root = root.child(phases.build());
        }
        let controls = Element::new()
            .style(
                LayoutStyle::row()
                    .height(Dimension::Cells(1))
                    .shrink(0.0)
                    .gap(2),
            )
            .child(
                Button::new("Use this")
                    .on_click(move || {
                        let choice = if style.get_untracked() == AvatarStyle::Hover {
                            "hover"
                        } else {
                            "halo"
                        };
                        companion_view::save(store, &apply, |p| p.style = choice.into());
                        companion_view::settings(cx, store, &apply);
                    })
                    .view(mcx),
            )
            .child(
                Button::new("Cancel")
                    .on_click(move || companion_view::settings(cx, store, &back))
                    .view(mcx),
            )
            .child(
                Button::new("Auto cycle")
                    .on_click(move || {
                        automatic.set(true);
                        // Explicit preview playback may temporarily override reduced
                        // motion; the persisted preference is never changed.
                        animated.set(true);
                    })
                    .view(mcx),
            )
            .child(
                Button::new("Pause motion")
                    .on_click(move || {
                        automatic.set(false);
                        animated.set(false);
                    })
                    .view(mcx),
            )
            .build();
        root.child(controls)
            .child(modals::hint_row(
                &tokens,
                "Silent preview · 1–7 / ←→ hold phase · c cycles/holds · Esc cancels".into(),
            ))
            .build()
    });
}
