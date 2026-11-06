//! The sidebar: the "Places" list and the folder tree.
//!
//! OpenXplorer stacks the two in a vertical paned sidebar: the places list on
//! top, the folder tree below (`src/window/sidebar.rs` and
//! `src/window/folder_tree.rs` in the upstream repository).

use gpui::{Context, MouseButton, MouseDownEvent, SharedString, Window, div, px};
use theme::ActiveTheme;
use ui::{Color, Icon, IconSize, Label, LabelSize, prelude::*};

use crate::model::tree;
use crate::ui::ox_theme::{metrics, palette};
use crate::ui::panel::FileManagerPanel;
use crate::ui::places::PlaceSection;

/// Renders the sidebar: places list on top, folder tree below, split by a
/// draggable divider.
///
/// The split is proportional, like OpenXplorer's vertical `GtkPaned`: the two
/// panes get `sidebar_split` and `1 - sidebar_split` of the height, so the
/// divider behaves the same when the panel is resized.
pub(crate) fn render_sidebar(
    this: &mut FileManagerPanel,
    window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());
    let split = this.sidebar_split.clamp(0.12, 0.88);

    v_flex()
        .id("file-manager-sidebar")
        .w(px(this.effective_sidebar_width()))
        .h_full()
        .min_h(px(0.))
        .flex_shrink_0()
        .bg(palette.sidebar)
        .border_r_1()
        .border_color(palette.divider)
        // The splitter computes the ratio from the pointer position against
        // the sidebar's own bounds; GPUI owns the drag, so releasing the mouse
        // always ends it (same pattern as the width resizer).
        .on_drag_move::<crate::ui::drag::SidebarSplitDrag>(cx.listener(
            |this, event: &gpui::DragMoveEvent<crate::ui::drag::SidebarSplitDrag>, _, cx| {
                let height = f32::from(event.bounds.size.height);
                if height <= 0. {
                    return;
                }
                let y = f32::from(event.event.position.y - event.bounds.top());
                this.set_sidebar_split(y / height, cx);
            },
        ))
        .child(
            v_flex()
                .id("file-manager-places-pane")
                .w_full()
                .flex_grow(split)
                .flex_basis(px(0.))
                .min_h(px(0.))
                .overflow_hidden()
                .child(render_places(this, cx)),
        )
        .child(render_splitter(this, split, cx))
        .child(
            v_flex()
                .id("file-manager-tree-pane")
                .w_full()
                .flex_grow(1. - split)
                .flex_basis(px(0.))
                .min_h(px(0.))
                .overflow_hidden()
                .child(render_tree(this, window, cx)),
        )
        .into_any_element()
}

/// The divider between the places list and the folder tree.
fn render_splitter(
    this: &FileManagerPanel,
    split: f32,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());
    let _ = (this, split);

    div()
        .id("file-manager-sidebar-splitter")
        .w_full()
        .h(px(7.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .cursor(gpui::CursorStyle::ResizeUpDown)
        .on_drag(crate::ui::drag::SidebarSplitDrag, |_, _, _, cx| {
            cx.stop_propagation();
            cx.new(|_| crate::ui::drag::SidebarSplitDrag)
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
            }),
        )
        // Double click restores the even split.
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                if event.click_count == 2 {
                    this.reset_sidebar_split(cx);
                    cx.stop_propagation();
                }
            }),
        )
        .child(
            div()
                .w_full()
                .h(px(1.))
                .bg(palette.divider)
                .hover(|style| style.bg(palette.accent)),
        )
        .occlude()
        .into_any_element()
}

/// The "Places" list.
fn render_places(this: &FileManagerPanel, cx: &mut Context<FileManagerPanel>) -> AnyElement {
    let palette = palette(cx.theme());
    let current_dir = this.current_dir.clone();

    let mut list = v_flex()
        .id("file-manager-places")
        .w_full()
        .px(px(7.))
        .py(px(13.));

    let mut last_section: Option<PlaceSection> = None;
    for place in &this.places {
        if let Some(heading) = place.section.heading() {
            if last_section != Some(place.section) {
                list = list.child(
                    Label::new(heading)
                        .size(LabelSize::XSmall)
                        .color(Color::Custom(palette.muted))
                        .mt(px(8.))
                        .mb(px(4.)),
                );
            }
        }
        last_section = Some(place.section);

        let is_current = current_dir.as_ref() == Some(&place.path);
        let path = place.path.clone();
        let label = place.label.clone();
        let icon = place.icon;

        list = list.child(
            h_flex()
                .id(SharedString::from(format!(
                    "file-manager-place-{}",
                    place.path.display()
                )))
                .h(px(metrics::SIDEBAR_ROW))
                .w_full()
                .pl(px(if place.is_child { 24. } else { 11. }))
                .pr(px(11.))
                .rounded_xs()
                .gap(px(8.))
                .when(is_current, |this| this.bg(palette.selected))
                .hover(|style| style.bg(palette.hover))
                .child(Icon::new(icon).size(IconSize::Small).color(if is_current {
                    Color::Custom(palette.accent)
                } else {
                    Color::Custom(palette.muted)
                }))
                .child(
                    Label::new(label)
                        .single_line()
                        .truncate()
                        .size(LabelSize::Small)
                        .color(Color::Custom(palette.text)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                        this.navigate_to(path.clone(), window, cx);
                        cx.stop_propagation();
                    }),
                ),
        );
    }

    div()
        .id("file-manager-sidebar-scroll")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .child(list)
        .into_any_element()
}

/// The folder tree.
fn render_tree(
    this: &FileManagerPanel,
    _window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());
    let rows = tree::visible_rows(&this.tree_nodes);
    let current_dir = this.current_dir.clone();

    let mut list = v_flex()
        .id("file-manager-tree")
        .w_full()
        .px(px(7.))
        .py(px(6.));
    if rows.is_empty() {
        list = list.child(
            Label::new("No folder tree")
                .size(LabelSize::XSmall)
                .color(Color::Custom(palette.muted)),
        );
    }

    for node in rows {
        let is_current = current_dir.as_ref() == Some(&node.path);
        let path = node.path.clone();
        let chevron_path = node.path.clone();
        let label = node.name.clone();
        let expanded = node.expanded;

        list = list.child(
            h_flex()
                .id(SharedString::from(format!(
                    "file-manager-tree-node-{}",
                    node.path.display()
                )))
                .h(px(metrics::TREE_ROW))
                .w_full()
                .pl(px(metrics::TREE_INDENT * node.depth as f32 + 7.))
                .pr(px(7.))
                .rounded_xs()
                .gap(px(8.))
                .when(is_current, |this| this.bg(palette.selected))
                .hover(|style| style.bg(palette.hover))
                // The chevron expands and collapses the node.
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "file-manager-tree-chevron-{}",
                            node.path.display()
                        )))
                        // A block container would ignore `items_center`.
                        .flex()
                        .w(px(16.))
                        .h(px(16.))
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .child(
                            Icon::new(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(IconSize::XSmall)
                            .color(Color::Custom(palette.muted)),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                                this.toggle_tree_node(&chevron_path, window, cx);
                                cx.stop_propagation();
                            }),
                        ),
                )
                .child(
                    Icon::new(IconName::Folder)
                        .size(IconSize::Small)
                        .color(Color::Custom(palette.accent)),
                )
                .child(
                    Label::new(label)
                        .single_line()
                        .truncate()
                        .size(LabelSize::Small)
                        .color(Color::Custom(palette.text)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                        this.navigate_to(path.clone(), window, cx);
                        cx.stop_propagation();
                    }),
                ),
        );
    }

    div()
        .id("file-manager-tree-scroll")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .child(list)
        .into_any_element()
}
