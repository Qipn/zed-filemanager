//! The icon grid view: one tile per entry.

use gpui::{ClickEvent, Context, ExternalPaths, IntoElement, Window, div, px, uniform_list};
use theme::ActiveTheme;
use ui::{Color, Icon, IconName, IconSize, Label, LabelSize, prelude::*, rems_from_px};

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
    let rows = this.visible_rows();
    uniform_list(
        "file-manager-grid",
        rows.len(),
        cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
            range
                .filter_map(|index| {
                    rows.get(index)
                        .cloned()
                        .map(|(entry, _)| render_tile(this, index, entry, window, cx))
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
    _window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());
    let selected = this.is_selected(&entry.path);
    let icon = if entry.is_dir {
        IconName::Folder
    } else {
        IconName::File
    };
    let clicked_entry = entry.clone();
    let drag_path = entry.path.clone();
    let drag_name = entry.name.clone();
    // The tile grows with the icon, like OpenXplorer's grid view.
    let icon_size = this.icon_size();
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
                .child(
                    Icon::new(icon)
                        .size(IconSize::Custom(rems_from_px(icon_size)))
                        .color(if entry.is_dir {
                            Color::Accent
                        } else {
                            Color::Muted
                        }),
                ),
        )
        .child(
            Label::new(entry.name.clone())
                .size(LabelSize::Small)
                .line_clamp(2)
                .text_center()
                .when(selected, |label| label.color(Color::Custom(palette.text))),
        )
        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            if this.rename_state.is_some() {
                this.cancel_rename(cx);
            }
            let modifiers = match event {
                ClickEvent::Mouse(click) => click.up.modifiers,
                ClickEvent::Keyboard(_) | ClickEvent::Touch(_) => Default::default(),
            };
            if modifiers.control {
                this.toggle_select(index, cx);
            } else if modifiers.shift {
                this.select_range(index, cx);
            } else {
                this.select_only(index, cx);
            }
            if event.click_count() >= 2 && !modifiers.control && !modifiers.shift {
                this.open_entry(clicked_entry.clone(), window, cx);
            }
            cx.notify();
        }))
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
