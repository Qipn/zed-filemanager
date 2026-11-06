//! The conflict prompt shown when a transfer would overwrite existing entries.

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
    let count = pending.conflicts.len();
    let names: Vec<String> = pending
        .conflicts
        .iter()
        .filter_map(|index| pending.targets.get(*index))
        .filter_map(|target| target.file_name())
        .map(|name| name.to_string_lossy().to_string())
        .take(3)
        .collect();
    let mut summary = format!("{count} item(s) already exist");
    if !names.is_empty() {
        summary.push_str(": ");
        summary.push_str(&names.join(", "));
        if count > names.len() {
            summary.push_str("…");
        }
    }

    Some(
        h_flex()
            .id("file-manager-conflict-banner")
            .flex_1()
            .gap_2()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(palette.divider)
            .child(
                Label::new(summary)
                    .single_line()
                    .size(LabelSize::Small)
                    .flex_1(),
            )
            .child(
                Button::new(
                    "file-manager-conflict-replace",
                    ConflictPolicy::Replace.label(),
                )
                .size(ButtonSize::Compact)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.resolve_conflicts(ConflictPolicy::Replace, window, cx);
                })),
            )
            .child(
                Button::new("file-manager-conflict-skip", ConflictPolicy::Skip.label())
                    .size(ButtonSize::Compact)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.resolve_conflicts(ConflictPolicy::Skip, window, cx);
                    })),
            )
            .child(
                Button::new(
                    "file-manager-conflict-keep-both",
                    ConflictPolicy::KeepBoth.label(),
                )
                .size(ButtonSize::Compact)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.resolve_conflicts(ConflictPolicy::KeepBoth, window, cx);
                })),
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
