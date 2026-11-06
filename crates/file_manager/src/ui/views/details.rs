//! The details view: one row per entry, with the OpenXplorer columns.

use std::path::PathBuf;

use gpui::{
    ClickEvent, Context, DragMoveEvent, ExternalDragPayload, ExternalPaths, FileDragPaths,
    IntoElement, MouseButton, MouseDownEvent, SharedString, Window, div, px, uniform_list,
};
use theme::ActiveTheme;
use ui::{Color, Icon, IconName, IconSize, Label, LabelSize, prelude::*};

use crate::model::entries::DirEntry;
use crate::ui::drag::DragGhost;
use crate::ui::ox_theme::{metrics, palette};
use crate::ui::panel::{DraggedPaths, FileManagerPanel};
use crate::ui::views::format::{format_modified, type_label};
pub(super) fn render_details_list(
    this: &mut FileManagerPanel,
    _window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let rows = this.visible_rows();
    uniform_list(
        "file-manager-entries",
        rows.len(),
        cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
            range
                .filter_map(|index| {
                    rows.get(index).cloned().map(|(entry, depth)| {
                        render_details_row(this, index, entry, depth, window, cx)
                    })
                })
                .collect()
        }),
    )
    // Same shape as `extensions_ui` / `agent_registry_ui`: the flex parent
    // gives the list a definite height and the list scrolls its own overflow.
    // `min_h(0)` lets it shrink below its content — without it the automatic
    // minimum size keeps the list at content height, so it overflows the pane
    // and the wheel has nothing to scroll.
    .flex_grow_1()
    .min_h(px(0.))
    .into_any_element()
}

/// One row of the details view.
pub(super) fn render_details_row(
    this: &FileManagerPanel,
    index: usize,
    entry: DirEntry,
    depth: usize,
    _window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());
    let selected = this.is_selected(&entry.path);
    let right_click_path = entry.path.clone();
    let clicked_entry = entry.clone();
    let drag_path = entry.path.clone();
    let drag_name = entry.name.clone();
    // Dragging a row that is part of the selection drags the whole selection.
    let drag_paths: Vec<PathBuf> = if selected && this.selected.len() > 1 {
        this.selected.clone()
    } else {
        vec![drag_path.clone()]
    };
    let drag_labels: Vec<SharedString> = if drag_paths.len() > 1 {
        drag_paths
            .iter()
            .filter_map(|path| {
                this.entries
                    .iter()
                    .find(|candidate| candidate.path == *path)
                    .map(|candidate| candidate.name.clone())
            })
            .collect()
    } else {
        vec![drag_name.clone()]
    };
    // Each drag-related closure needs its own copy of the path.
    let drag_move_path = drag_path.clone();
    let drag_drop_path = drag_path.clone();
    let ext_move_path = drag_path.clone();
    let ext_drop_path = drag_path.clone();

    let is_clipboard_source = this
        .clipboard
        .as_ref()
        .is_some_and(|clipboard| clipboard.paths().contains(&entry.path));
    let is_drag_target = this.drag_target.as_ref() == Some(&entry.path);
    // Directories can be expanded in place, like OpenXplorer's expandable
    // folders. While searching the rows are flat hits, so the expander is
    // hidden (as in OpenXplorer and Nautilus); the 16 px placeholder stays so
    // the icons remain aligned.
    let can_expand = entry.is_dir && !this.is_searching();
    let is_expanded = can_expand && this.is_entry_expanded(&entry.path);
    let expand_path = entry.path.clone();

    h_flex()
        .id(("file-manager-entry", index))
        .h(px(metrics::ROW_PITCH))
        .w_full()
        .px(px(metrics::COLUMN_EDGE_PADDING))
        .border_b_1()
        .border_color(palette.divider)
        .when(selected, |this| this.bg(palette.selected))
        .when(is_drag_target, |this| this.bg(palette.drop_target))
        // Name column.
        .child(
            h_flex()
                .id("file-manager-column-name")
                .w(px(metrics::COLUMN_NAME_MIN))
                .flex_grow(1.)
                .min_w(px(metrics::COLUMN_NAME_MIN))
                .gap(px(11.))
                // The expander, in front of the icon. Like OpenXplorer, the
                // indentation grows by 16 px per level. Non-directories keep
                // the 16 px box so the icons stay aligned.
                //
                // The indent is a spacer *before* the expander box: putting the
                // width on the box itself would leave the chevron pinned to the
                // left edge while only the icon moved right, so the expander of
                // an expanded folder would not follow its parent.
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "file-manager-indent-{}",
                            entry.path.display()
                        )))
                        .w(px(metrics::TREE_INDENT * depth as f32))
                        .flex_shrink_0(),
                )
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "file-manager-expander-{}",
                            entry.path.display()
                        )))
                        .w(px(16.))
                        .h(px(16.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(can_expand, |expander| {
                            expander
                                .child(
                                    Icon::new(if is_expanded {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .size(IconSize::XSmall)
                                    .color(Color::Custom(palette.muted)),
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(
                                        move |this, _event: &MouseDownEvent, window, cx| {
                                            this.toggle_entry_expanded(&expand_path, window, cx);
                                            cx.stop_propagation();
                                        },
                                    ),
                                )
                        }),
                )
                .child(super::entry_icon(&entry, is_expanded, IconSize::Small, cx))
                .child({
                    // While this entry is being renamed the inline editor takes
                    // the place of its name (`project_panel` renders its
                    // filename editor the same way).
                    if this.is_renaming(&entry.path) {
                        this.rename_field.clone().into_any_element()
                    } else {
                        Label::new(entry.name.clone())
                            .single_line()
                            .truncate()
                            .when(is_clipboard_source, |label| label.color(Color::Muted))
                            .into_any_element()
                    }
                }),
        )
        // Date modified column.
        .when(this.is_modified_column_shown(), |row| {
            row.child(
                h_flex().w(px(metrics::COLUMN_MODIFIED)).child(
                    Label::new(format_modified(entry.mtime))
                        .single_line()
                        .size(LabelSize::Small)
                        .color(Color::Custom(palette.muted)),
                ),
            )
        })
        // Type column.
        .when(this.is_type_column_shown(), |row| {
            row.child(
                h_flex().w(px(metrics::COLUMN_TYPE)).child(
                    Label::new(type_label(&entry))
                        .single_line()
                        .size(LabelSize::Small)
                        .color(Color::Custom(palette.muted)),
                ),
            )
        })
        // Size column, right aligned.
        .child(
            h_flex()
                .id("file-manager-column-size")
                .w(px(metrics::COLUMN_SIZE))
                // The trailing padding of the last column.
                .child(div().w(px(metrics::COLUMN_EDGE_PADDING)).flex_shrink_0())
                .justify_end()
                .child(
                    Label::new(
                        entry
                            .len
                            .map(crate::model::entries::format_len)
                            .unwrap_or_default(),
                    )
                    .single_line()
                    .size(LabelSize::Small)
                    .color(Color::Custom(palette.muted)),
                ),
        )
        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            // Clicking elsewhere cancels an active rename.
            if this.rename_state.is_some() {
                this.cancel_rename(window, cx);
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
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                // GNOME keeps a multi-selection when the clicked row is already
                // part of it; right-clicking outside the selection selects just
                // that row.
                if !this.is_selected(&right_click_path) {
                    this.selected.clear();
                    this.selected.push(right_click_path.clone());
                    this.selection_anchor = Some(index);
                }
                if this.rename_state.is_some() {
                    this.cancel_rename(window, cx);
                }
                this.deploy_item_menu(event.position, window, cx);
                // Keep the view's background handler from clearing the item
                // selection and re-opening its own menu.
                cx.stop_propagation();
            }),
        )
        // Internal drag target (directories only).
        .when(entry.is_dir, |row| {
            row.on_drag_move::<DraggedPaths>(cx.listener(
                move |this, event: &DragMoveEvent<DraggedPaths>, _, cx| {
                    let hovered = event.bounds.contains(&event.event.position);
                    if hovered && this.drag_target.as_ref() != Some(&drag_move_path) {
                        this.drag_target = Some(drag_move_path.clone());
                        cx.notify();
                    } else if !hovered && this.drag_target.as_ref() == Some(&drag_move_path) {
                        this.drag_target = None;
                        cx.notify();
                    }
                },
            ))
            .on_drop(
                cx.listener(move |this, dragged: &DraggedPaths, window, cx| {
                    this.drop_dragged(
                        DraggedPaths(dragged.0.clone()),
                        drag_drop_path.clone(),
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
        })
        // External file drop target (directories only).
        .when(entry.is_dir, |row| {
            row.on_drag_move::<ExternalPaths>(cx.listener(
                move |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                    let hovered = event.bounds.contains(&event.event.position);
                    if hovered && this.drag_target.as_ref() != Some(&ext_move_path) {
                        this.drag_target = Some(ext_move_path.clone());
                        cx.notify();
                    } else if !hovered && this.drag_target.as_ref() == Some(&ext_move_path) {
                        this.drag_target = None;
                        cx.notify();
                    }
                },
            ))
            .on_drop(cx.listener(
                move |this, external: &ExternalPaths, window, cx| {
                    let paths: Vec<PathBuf> = external.0.iter().cloned().collect();
                    this.drop_external(paths, ext_drop_path.clone(), window, cx);
                    cx.stop_propagation();
                },
            ))
        })
        // Drag source: offer the entry internally and to the platform.
        .on_drag(DraggedPaths(drag_paths.clone()), {
            let labels = drag_labels.clone();
            move |_, _, _window, cx| {
                cx.new(|_| DragGhost {
                    labels: labels.clone(),
                })
            }
        })
        .external_drag_payload({
            // The platform payload needs the directory flag per path.
            let entries = this.entries.clone();
            move |dragged: &DraggedPaths, _, _| {
                Some(ExternalDragPayload::Files(FileDragPaths::new(
                    dragged
                        .0
                        .iter()
                        .map(|path| {
                            let is_dir = entries
                                .iter()
                                .find(|candidate| candidate.path == *path)
                                .map(|candidate| candidate.is_dir)
                                .unwrap_or(false);
                            (path.clone(), is_dir)
                        })
                        .collect::<Vec<_>>(),
                )))
            }
        })
        .into_any_element()
}
