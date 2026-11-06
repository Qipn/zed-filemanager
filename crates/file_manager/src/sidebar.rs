//! The sidebar: the "Places" list and the folder tree.
//!
//! OpenXplorer stacks the two in a vertical paned sidebar: the places list on
//! top, the folder tree below (`src/window/sidebar.rs` and
//! `src/window/folder_tree.rs` in the upstream repository).

use gpui::{Context, MouseButton, MouseDownEvent, SharedString, Window, div, px};
use theme::ActiveTheme;
use ui::{Color, Icon, IconSize, Label, LabelSize, prelude::*};

use crate::ox_theme::{metrics, palette};
use crate::panel::FileManagerPanel;
use crate::places::PlaceSection;
use crate::tree;

/// Renders the sidebar: places list on top, folder tree below.
pub(crate) fn render_sidebar(
    this: &mut FileManagerPanel,
    window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());

    v_flex()
        .id("file-manager-sidebar")
        .w(px(this.effective_sidebar_width()))
        .h_full()
        .flex_shrink_0()
        .bg(palette.sidebar)
        .border_r_1()
        .border_color(palette.divider)
        .child(render_places(this, cx))
        .child(
            // A hairline between the places list and the tree, using the same
            // colour as Zed's `ui::Divider`.
            div().h(px(1.)).w_full().bg(palette.divider),
        )
        .child(render_tree(this, window, cx))
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
        .overflow_y_scroll()
        .child(list)
        .into_any_element()
}
