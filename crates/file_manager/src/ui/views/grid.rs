//! The icon grid view: one tile per entry.

use gpui::{
    ClickEvent, Context, ExternalPaths, IntoElement, MouseButton, MouseDownEvent, Window, div, px,
    uniform_list,
};
use theme::ActiveTheme;
use ui::{Color, IconSize, Label, LabelSize, prelude::*, rems_from_px};

use crate::model::entries::DirEntry;
use crate::ui::drag::{DragGhost, DraggedPaths};
use crate::ui::ox_theme::{metrics, palette};
use crate::ui::panel::FileManagerPanel;
/// The icon grid view: one tile per entry.
pub(super) fn render_icon_grid(
    this: &mut FileManagerPanel,
    _window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let rows = std::rc::Rc::new(this.grid_rows());
    uniform_list(
        "file-manager-grid",
        rows.len(),
        cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
            range
                .filter_map(|index| {
                    rows.get(index)
                        .cloned()
                        .map(|(entry, _)| render_tile(this, index, entry, rows.clone(), window, cx))
                })
                .collect()
        }),
    )
    // See `render_details_list`: the flex parent sizes the list and the list
    // scrolls its own overflow.
    .flex_grow_1()
    .min_h(px(0.))
    .into_any_element()
}

/// One tile of the icon grid.
pub(super) fn render_tile(
    this: &FileManagerPanel,
    index: usize,
    entry: DirEntry,
    rows: std::rc::Rc<Vec<(DirEntry, usize)>>,
    _window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());
    let selected = this.is_selected(&entry.path);
    let clicked_entry = entry.clone();
    let right_click_path = entry.path.clone();
    let drag_path = entry.path.clone();
    let drag_name = entry.name.clone();
    // The tile grows with the icon, like OpenXplorer's grid view.
    let icon_size = this.icon_size();
    // The grid lays out the same rows the details view does, so a folder
    // expanded in place shows its children here too; the icon follows along.
    let expanded = this.is_entry_expanded(&entry.path);
    let tile_width = metrics::TILE_WIDTH.max(icon_size + 79.);
    let tile_height = metrics::TILE_HEIGHT + (icon_size - metrics::TILE_ICON);

    v_flex()
        .id(("file-manager-tile", index))
        .w(px(tile_width))
        .h(px(tile_height))
        .gap(px(8.))
        .py(px(12.))
        .px(px(10.))
        .rounded_xs()
        .when(selected, |this| this.bg(palette.selected))
        .hover(|style| style.bg(palette.hover))
        // Zed highlights a drop target with `drop_target_background` through
        // the declarative `drag_over` handler (project_panel / pane).
        .drag_over::<DraggedPaths>(|style, _, _, cx| {
            style.bg(cx.theme().colors().drop_target_background)
        })
        .drag_over::<ExternalPaths>(|style, _, _, cx| {
            style.bg(cx.theme().colors().drop_target_background)
        })
        .child(
            div()
                // The tile centres its icon and label; `flex_col()` alone
                // leaves the display mode at block, which ignores the centring.
                .flex()
                .flex_1()
                .w_full()
                .flex_col()
                .justify_center()
                .items_center()
                .child(super::entry_icon(
                    &entry,
                    expanded,
                    IconSize::Custom(rems_from_px(icon_size)),
                    cx,
                )),
        )
        .child({
            // The inline rename editor replaces the tile name while renaming.
            if this.is_renaming(&entry.path) {
                this.rename_field.clone().into_any_element()
            } else {
                Label::new(entry.name.clone())
                    .size(LabelSize::Small)
                    .line_clamp(2)
                    .text_center()
                    .when(selected, |label| label.color(Color::Custom(palette.text)))
                    .into_any_element()
            }
        })
        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            if this.rename_state.is_some() {
                this.cancel_rename(window, cx);
            }
            let modifiers = match event {
                ClickEvent::Mouse(click) => click.up.modifiers,
                ClickEvent::Keyboard(_) | ClickEvent::Touch(_) => Default::default(),
            };
            // The grid paints `grid_rows()`, which is not the list view's row
            // list once a folder is expanded in place, so the index is resolved
            // against the grid's own rows.
            if modifiers.control {
                this.toggle_select_in(rows.as_slice(), index, cx);
            } else if modifiers.shift {
                this.select_range_in(rows.as_slice(), index, cx);
            } else {
                this.select_only_in(rows.as_slice(), index, cx);
            }
            if event.click_count() >= 2 && !modifiers.control && !modifiers.shift {
                this.open_entry(clicked_entry.clone(), window, cx);
            }
            cx.notify();
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                // Keep a multi-selection that already contains this tile, like
                // the details view does.
                if !this.is_selected(&right_click_path) {
                    this.selected.clear();
                    this.selected.push(right_click_path.clone());
                    // The anchor is an index into the list view's rows (that is
                    // what shift-click and the keyboard use), so it is looked up
                    // by path instead of reusing the grid index.
                    this.selection_anchor = this
                        .visible_rows()
                        .iter()
                        .position(|(entry, _)| entry.path == right_click_path);
                }
                if this.rename_state.is_some() {
                    this.cancel_rename(window, cx);
                }
                this.deploy_item_menu(event.position, window, cx);
                cx.stop_propagation();
            }),
        )
        .on_drag(DraggedPaths(vec![drag_path.clone()]), {
            let label = drag_name;
            move |_, _, _window, cx| {
                cx.new(|_| DragGhost {
                    labels: vec![label.clone()],
                })
            }
        })
        .into_any_element()
}
