//! The batch rename prompt: one pattern for the whole selection.

use gpui::{AnyElement, Context, IntoElement, div, px};
use theme::ActiveTheme;
use ui::{Button, ButtonSize, Label, LabelSize, prelude::*};

use crate::ui::ox_theme::palette;
use crate::ui::panel::FileManagerPanel;

/// Renders the batch rename prompt, if it is open.
pub(super) fn render_batch_rename(
    this: &FileManagerPanel,
    cx: &mut Context<FileManagerPanel>,
) -> Option<AnyElement> {
    if !this.batch_rename_open {
        return None;
    }
    let palette = palette(cx.theme());
    let count = this.selected.len();

    Some(
        h_flex()
            .id("file-manager-batch-rename")
            .w_full()
            .gap_2()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(palette.divider)
            .child(
                Label::new(format!("Rename {count} items to"))
                    .single_line()
                    .size(LabelSize::Small),
            )
            .child(
                div()
                    .flex()
                    .h(px(24.))
                    .w(px(240.))
                    .px(px(6.))
                    .items_center()
                    .rounded_xs()
                    .bg(palette.field_bg)
                    .border_1()
                    .border_color(palette.accent)
                    .child(this.batch_field.clone()),
            )
            .child(
                Label::new("use # for the number")
                    .single_line()
                    .size(LabelSize::XSmall)
                    .color(Color::Custom(palette.muted)),
            )
            .child(div().flex_1())
            .child(
                Button::new("file-manager-batch-confirm", "Rename")
                    .size(ButtonSize::Compact)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.confirm_batch_rename(window, cx);
                    })),
            )
            .child(
                Button::new("file-manager-batch-cancel", "Cancel")
                    .size(ButtonSize::Compact)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_batch_rename(window, cx);
                    })),
            )
            .into_any_element(),
    )
}
