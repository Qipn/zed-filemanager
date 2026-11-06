//! The details pane on the right of the folder view.
//!
//! OpenXplorer shows a 262 px wide pane with a preview, the name and kind of
//! the selected entry, and a table of its properties
//! (`native/crates/ox-app/resources/ui/details-pane.ui`).

use gpui::{AnyElement, Context, IntoElement, Window, div, px};
use theme::ActiveTheme;
use ui::{Button, ButtonSize, Color, Icon, IconName, IconSize, Label, LabelSize, prelude::*};

use crate::model::entries::DirEntry;
use crate::ui::ox_theme::{metrics, palette};
use crate::ui::panel::FileManagerPanel;
use crate::ui::views::format_modified;

/// Renders the details pane, if an entry is selected.
pub(crate) fn render_details_pane(
    this: &FileManagerPanel,
    _window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> Option<AnyElement> {
    if !this.details_pane_visible {
        return None;
    }
    let entry = this.selected_entry()?;
    let palette = palette(cx.theme());

    Some(
        v_flex()
            .id("file-manager-details-pane")
            .w(px(metrics::DETAILS_PANE_WIDTH))
            .h_full()
            .flex_shrink_0()
            .bg(palette.bg)
            .border_l_1()
            .border_color(palette.divider)
            .p_3()
            .gap_2()
            // The header: "Details" and the close button.
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .child(
                        Label::new("Details")
                            .size(LabelSize::Small)
                            .color(Color::Custom(palette.text)),
                    )
                    .child(
                        Button::new("file-manager-details-close", "×")
                            .size(ButtonSize::Compact)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.details_pane_visible = false;
                                cx.notify();
                            })),
                    ),
            )
            // The preview.
            .child(
                div()
                    // `.flex_col()` only sets the direction; the display mode
                    // has to be flex as well, or the centering is ignored.
                    .flex()
                    .flex_1()
                    .min_h(px(120.))
                    .w_full()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(crate::ui::views::entry_icon(&entry, IconSize::XLarge, cx)),
            )
            // The name and the kind of the entry.
            .child(
                Label::new(entry.name.clone())
                    .size(LabelSize::Small)
                    .color(Color::Custom(palette.text)),
            )
            .child(
                Label::new(kind_label(&entry))
                    .size(LabelSize::XSmall)
                    .color(Color::Custom(palette.muted)),
            )
            // Open, like OpenXplorer's action button (§4.8).
            .child({
                let open_entry = entry.clone();
                h_flex().w_full().pt(px(6.)).child(
                    Button::new("file-manager-details-open", "Open")
                        .size(ButtonSize::Compact)
                        .start_icon(Icon::new(IconName::ArrowRight).size(IconSize::Small))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_entry(open_entry.clone(), window, cx);
                        })),
                )
            })
            // The properties.
            .child(
                Label::new("Properties")
                    .size(LabelSize::XSmall)
                    .color(Color::Custom(palette.muted))
                    .mt(px(4.)),
            )
            .child(property_row("Size", size_label(&entry), palette.muted))
            .child(property_row(
                "Modified",
                format_modified(entry.mtime),
                palette.muted,
            ))
            .child(property_row(
                "Location",
                entry
                    .path
                    .parent()
                    .map(|parent| parent.display().to_string())
                    .unwrap_or_default(),
                palette.muted,
            ))
            .into_any_element(),
    )
}

/// One row of the properties table.
fn property_row(name: &str, value: String, muted: gpui::Hsla) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_2()
        .child(
            div().w(px(73.)).flex_shrink_0().child(
                Label::new(name.to_string())
                    .size(LabelSize::XSmall)
                    .color(Color::Custom(muted)),
            ),
        )
        .child(
            Label::new(value)
                .size(LabelSize::XSmall)
                .single_line()
                .truncate()
                .flex_1(),
        )
}

/// The "kind" of the entry, shown under its name.
fn kind_label(entry: &DirEntry) -> String {
    if entry.is_dir {
        return "Folder".into();
    }
    let Some(extension) = entry.path.extension().and_then(|ext| ext.to_str()) else {
        return "File".into();
    };
    extension.to_uppercase()
}

/// The size of the entry, or a dash for folders.
fn size_label(entry: &DirEntry) -> String {
    entry
        .len
        .map(crate::model::entries::format_len)
        .unwrap_or_else(|| "—".into())
}
