//! The progress row of a running transfer, with its Cancel button.

use gpui::{AnyElement, Context, IntoElement, div, px};
use theme::ActiveTheme;
use ui::{Button, ButtonSize, Label, LabelSize, prelude::*};

use crate::model::conflict::TransferKind;
use crate::ui::ox_theme::palette;
use crate::ui::panel::FileManagerPanel;

/// Renders the progress of the transfer in flight, if any.
pub(super) fn render_transfer_progress(
    this: &FileManagerPanel,
    cx: &mut Context<FileManagerPanel>,
) -> Option<AnyElement> {
    let progress = this.transfer_progress.as_ref()?;
    let palette = palette(cx.theme());
    let total = progress.total.max(1);
    let ratio = (progress.done as f32 / total as f32).clamp(0., 1.);
    let verb = match progress.kind {
        TransferKind::Copy => "Copying",
        TransferKind::Link => "Linking",
        TransferKind::Move => "Moving",
    };

    let mut summary = format!("{verb} {} of {}", progress.done, progress.total);
    if !progress.current.is_empty() {
        summary.push_str(" — ");
        summary.push_str(&progress.current);
    }

    Some(
        h_flex()
            .id("file-manager-transfer-progress")
            .w_full()
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
            // A 120px bar is enough to see the transfer move; the exact numbers
            // are in the label next to it.
            .child(
                div()
                    .w(px(120.))
                    .h(px(6.))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(palette.divider)
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .w(px(120. * ratio))
                            .rounded_full()
                            .bg(palette.accent),
                    ),
            )
            .child(
                Button::new("file-manager-transfer-cancel", "Cancel")
                    .size(ButtonSize::Compact)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel_transfer(cx);
                    })),
            )
            .into_any_element(),
    )
}
