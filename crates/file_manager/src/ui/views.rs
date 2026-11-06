//! The folder view: OpenXplorer's details view and its icon grid.
//!
//! The details view shows the columns `Name`, `Date modified`, `Type` and
//! `Size`, with the column widths of the upstream application
//! (`native/crates/ox-app/src/folder_view/column_widths.rs`). The icon grid
//! shows one tile per entry, with the icon above the (wrapped) name.
//!
//! The two views and their label helpers live in `views/`.

mod details;
mod format;
mod grid;

pub(crate) use format::format_modified;

use details::render_details_list;
use grid::render_icon_grid;

use gpui::{AnyElement, Context, IntoElement, Window, div, px};
use theme::ActiveTheme;
use ui::{Color, Icon, IconName, IconSize, Label, LabelSize, prelude::*};

use crate::ui::ox_theme::{metrics, palette};
use crate::ui::panel::FileManagerPanel;

/// How the contents of the current directory are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum ViewMode {
    /// The table-like list with the OpenXplorer columns.
    #[default]
    Details,
    /// The grid of large icons.
    Grid,
}

/// Renders the column header of the details view.
pub(crate) fn render_column_header(
    this: &FileManagerPanel,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());
    let sort_mode = this.sort_mode;

    let title = |label: &'static str, active: bool, right: bool| {
        let mut header = h_flex()
            .h(px(metrics::COLUMN_HEADER))
            .px(px(12.))
            .gap(px(4.))
            .when(right, |this| this.justify_end())
            .child(
                Label::new(label)
                    .single_line()
                    .size(LabelSize::Small)
                    .color(if active {
                        Color::Custom(palette.text)
                    } else {
                        Color::Custom(palette.muted)
                    }),
            );
        if active {
            // The caret points up for ascending order, down for descending.
            let icon = match sort_mode {
                crate::model::entries::SortMode::NameAscending
                | crate::model::entries::SortMode::ModifiedAscending
                | crate::model::entries::SortMode::SizeAscending => IconName::ChevronUp,
                _ => IconName::ChevronDown,
            };
            header = header.child(
                Icon::new(icon)
                    .size(IconSize::XSmall)
                    .color(Color::Custom(palette.muted)),
            );
        }
        header
    };

    h_flex()
        .id("file-manager-column-header")
        // The header is a fixed-height band. `flex_1()` here would grow along
        // the parent `v_flex`'s vertical axis and swallow the list.
        .h(px(metrics::COLUMN_HEADER))
        .w_full()
        .flex_shrink_0()
        .border_b_1()
        .border_color(palette.divider)
        .child(
            h_flex()
                .id("file-manager-column-name")
                .w(px(metrics::COLUMN_NAME_MIN))
                .flex_grow(1.)
                .min_w(px(metrics::COLUMN_NAME_MIN))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.sort_by_name(window, cx);
                }))
                .child(title("Name", sort_mode.is_name(), false)),
        )
        .when(this.is_modified_column_shown(), |header| {
            header.child(
                h_flex()
                    .id("file-manager-column-modified")
                    .w(px(metrics::COLUMN_MODIFIED))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.sort_by_modified(window, cx);
                    }))
                    .child(title("Date modified", sort_mode.is_modified(), false)),
            )
        })
        .when(this.is_type_column_shown(), |header| {
            header.child(
                h_flex()
                    .id("file-manager-column-type")
                    .w(px(metrics::COLUMN_TYPE))
                    .child(title("Type", false, false)),
            )
        })
        .child(
            h_flex()
                .id("file-manager-column-size")
                .w(px(metrics::COLUMN_SIZE))
                // The trailing padding of the last column.
                .child(div().w(px(metrics::COLUMN_EDGE_PADDING)).flex_shrink_0())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.sort_by_size(window, cx);
                }))
                .child(title("Size", sort_mode.is_size(), true)),
        )
        .into_any_element()
}

/// Renders the folder view: the details list or the icon grid.
pub(crate) fn render_view(
    this: &mut FileManagerPanel,
    window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    if let Some(error) = &this.error {
        return div()
            .p_2()
            .child(Label::new(error.clone()).single_line())
            .into_any_element();
    }

    if this.current_dir.is_none() {
        return div()
            .p_2()
            .child(Label::new("No folder open").single_line())
            .into_any_element();
    }

    let rows = this.visible_rows();
    if rows.is_empty() {
        let message = if this.is_searching() {
            if this.search_running {
                "Searching…"
            } else {
                "No results"
            }
        } else if this.search_query.as_ref().is_some_and(|q| !q.is_empty()) {
            "No matching items"
        } else {
            "This folder is empty"
        };
        return div()
            .p_2()
            .child(Label::new(message).single_line())
            .into_any_element();
    }

    match this.view_mode {
        ViewMode::Details => v_flex()
            .id("file-manager-details")
            .flex_1()
            // Without `min_h(0)` the column cannot shrink below its content,
            // so the list would be clipped instead of scrolled.
            .min_h(px(0.))
            .child(render_column_header(this, cx))
            .child(render_details_list(this, window, cx))
            .into_any_element(),
        ViewMode::Grid => render_icon_grid(this, window, cx),
    }
}
