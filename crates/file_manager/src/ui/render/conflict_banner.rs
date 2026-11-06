//! The conflict prompt shown when a transfer would overwrite existing entries.
//!
//! Modelled on `nautilus-file-conflict-dialog.c`: the prompt names the entry that
//! clashes, offers a suggested name to edit, and answers with replace / skip /
//! use-this-name. One answer can be applied to every remaining conflict.

use gpui::{AnyElement, Context, IntoElement};
use theme::ActiveTheme;
use ui::{Button, ButtonSize, Label, LabelSize, prelude::*};

use crate::model::conflict::ConflictPolicy;
use crate::ui::ox_theme::palette;
use crate::ui::panel::FileManagerPanel;
/// Renders the conflict prompt shown when a transfer would overwrite
/// existing entries.
pub(super) fn render_conflict_banner(
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
        h_flex()
            .id("file-manager-conflict-banner")
            .flex_1()
            .gap_2()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(palette.divider)
            .child(Label::new(summary).single_line().size(LabelSize::Small))
            // The suggested name, editable: typing another one and pressing
            // `Use name` renames the incoming entry instead of touching the
            // existing one.
            .child(
                div()
                    .flex()
                    .h(px(24.))
                    .w(px(200.))
                    .px(px(6.))
                    .items_center()
                    .rounded_xs()
                    .bg(palette.field_bg)
                    .border_1()
                    .border_color(palette.accent)
                    .child(this.conflict_field.clone()),
            )
            .child(
                Button::new("file-manager-conflict-use-name", "Use name")
                    .size(ButtonSize::Compact)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.resolve_current_conflict(ConflictPolicy::KeepBoth, true, window, cx);
                    })),
            )
            .child(
                Button::new(
                    "file-manager-conflict-replace",
                    ConflictPolicy::Replace.label(),
                )
                .size(ButtonSize::Compact)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.resolve_current_conflict(ConflictPolicy::Replace, false, window, cx);
                })),
            )
            .child(
                Button::new("file-manager-conflict-skip", ConflictPolicy::Skip.label())
                    .size(ButtonSize::Compact)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.resolve_current_conflict(ConflictPolicy::Skip, false, window, cx);
                    })),
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
                .on_click(move |_, _, cx| {
                    panel.update(cx, |this, cx| {
                        this.conflict_apply_to_all = !this.conflict_apply_to_all;
                        cx.notify();
                    });
                }),
            )
            .child(
                Button::new("file-manager-conflict-cancel", "Cancel")
                    .size(ButtonSize::Compact)
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.cancel_conflicts(cx);
                    })),
            )
            .into_any_element(),
    )
}
