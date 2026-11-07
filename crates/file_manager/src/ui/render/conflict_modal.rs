//! The name-conflict prompt, rendered as a modal card.
//!
//! Modelled on `nautilus-file-conflict-dialog.c` for the content, and on Zed's
//! own modal layer (`crates/workspace/src/modal_layer.rs` + `ui::AlertModal`)
//! for the chrome: an absolutely positioned overlay with an elevated card, so
//! the prompt never takes part in the panel's layout. It used to be an inline
//! banner, which pushed the file list down and broke the view while it was open.

use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, px};
use theme::ActiveTheme;
use ui::{Button, ButtonSize, Headline, HeadlineSize, Label, LabelSize, prelude::*};

use crate::model::conflict::ConflictPolicy;
use crate::ui::ox_theme::palette;
use crate::ui::panel::FileManagerPanel;

/// Renders the conflict prompt as a modal over the panel, if a transfer is
/// waiting for an answer.
pub(super) fn render_conflict_modal(
    this: &FileManagerPanel,
    cx: &mut Context<FileManagerPanel>,
) -> Option<AnyElement> {
    let pending = this.pending_transfer.as_ref()?;
    let palette = palette(cx.theme());
    let total = pending.conflicts.len();
    let position = pending.current.min(total.saturating_sub(1));
    let existing = pending
        .conflicts
        .get(position)
        .and_then(|index| pending.targets.get(*index))
        .map(|target| {
            target
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default()
        })
        .unwrap_or_default();

    let mut summary = format!("“{existing}” already exists");
    if total > 1 {
        summary.push_str(&format!(" ({} of {total})", position + 1));
    }

    let apply_to_all = this.conflict_apply_to_all;
    let panel = cx.entity();

    Some(
        // The backdrop: covers the whole panel, swallows clicks so the list
        // underneath cannot be interacted with. Unlike Zed's modal layer it does
        // not dismiss on click — this prompt sits in the middle of a transfer and
        // a stray click must not abort it.
        div()
            .id("file-manager-conflict-overlay")
            .absolute()
            .size_full()
            .inset_0()
            .occlude()
            .child(
                // Zero-height column anchored near the top, mirroring
                // `ModalLayer::render`, so the card hangs from a fixed offset.
                v_flex().h(px(0.0)).top_20().items_center().child(
                    // The card itself: same recipe as `ui::AlertModal`.
                    v_flex()
                        .id("file-manager-conflict-modal")
                        .occlude()
                        .elevation_3(cx)
                        .w(px(440.0))
                        .bg(cx.theme().colors().elevated_surface_background)
                        .overflow_hidden()
                        .child(
                            v_flex()
                                .p_3()
                                .gap_2()
                                .child(Headline::new(summary).size(HeadlineSize::Small))
                                .child(
                                    Label::new(
                                        "Keep both under a different name, or replace or \
                                             skip the existing entry.",
                                    )
                                    .size(LabelSize::Small),
                                )
                                // The suggested name, editable: typing another one and
                                // pressing `Use name` renames the incoming entry instead of
                                // touching the existing one.
                                .child(
                                    div()
                                        .flex()
                                        .h(px(28.0))
                                        .px(px(6.0))
                                        .items_center()
                                        .rounded_xs()
                                        .bg(palette.field_bg)
                                        .border_1()
                                        .border_color(palette.accent)
                                        .child(this.conflict_field.clone()),
                                )
                                .child(
                                    Button::new(
                                        "file-manager-conflict-apply-all",
                                        if apply_to_all {
                                            "Apply to all ✓"
                                        } else {
                                            "Apply to all"
                                        },
                                    )
                                    .size(ButtonSize::Compact)
                                    .on_click(
                                        move |_, _, cx| {
                                            panel.update(cx, |this, cx| {
                                                this.conflict_apply_to_all =
                                                    !this.conflict_apply_to_all;
                                                cx.notify();
                                            });
                                        },
                                    ),
                                ),
                        )
                        .child(
                            h_flex()
                                .justify_end()
                                .gap_1()
                                .p_3()
                                .border_t_1()
                                .border_color(palette.divider)
                                .child(
                                    Button::new("file-manager-conflict-cancel", "Cancel")
                                        .size(ButtonSize::Compact)
                                        .on_click(cx.listener(|this, _, _window, cx| {
                                            this.cancel_conflicts(cx);
                                        })),
                                )
                                .child(
                                    Button::new(
                                        "file-manager-conflict-skip",
                                        ConflictPolicy::Skip.label(),
                                    )
                                    .size(ButtonSize::Compact)
                                    .on_click(cx.listener(
                                        |this, _, window, cx| {
                                            this.resolve_current_conflict(
                                                ConflictPolicy::Skip,
                                                false,
                                                window,
                                                cx,
                                            );
                                        },
                                    )),
                                )
                                .child(
                                    Button::new(
                                        "file-manager-conflict-replace",
                                        ConflictPolicy::Replace.label(),
                                    )
                                    .size(ButtonSize::Compact)
                                    .on_click(cx.listener(
                                        |this, _, window, cx| {
                                            this.resolve_current_conflict(
                                                ConflictPolicy::Replace,
                                                false,
                                                window,
                                                cx,
                                            );
                                        },
                                    )),
                                )
                                .child(
                                    Button::new("file-manager-conflict-use-name", "Use name")
                                        .size(ButtonSize::Compact)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.resolve_current_conflict(
                                                ConflictPolicy::KeepBoth,
                                                true,
                                                window,
                                                cx,
                                            );
                                        })),
                                ),
                        ),
                ),
            )
            .into_any_element(),
    )
}
