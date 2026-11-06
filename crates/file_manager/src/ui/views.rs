//! The folder view: OpenXplorer's details view and its icon grid.
//!
//! The details view shows the columns `Name`, `Date modified`, `Type` and
//! `Size`, with the column widths of the upstream application
//! (`native/crates/ox-app/src/folder_view/column_widths.rs`). The icon grid
//! shows one tile per entry, with the icon above the (wrapped) name.

use std::path::PathBuf;

use gpui::{
    ClickEvent, Context, DragMoveEvent, ExternalDragPayload, ExternalPaths, FileDragPaths,
    IntoElement, MouseButton, MouseDownEvent, SharedString, Window, div, px, uniform_list,
};
use theme::ActiveTheme;
use ui::{Color, Icon, IconName, IconSize, Label, LabelSize, prelude::*, rems_from_px};

use crate::model::entries::DirEntry;
use crate::ui::ox_theme::{metrics, palette};
use crate::ui::panel::{DragGhost, DraggedPaths, FileManagerPanel};

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

/// The details view: one row per entry, with the OpenXplorer columns.
fn render_details_list(
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
fn render_details_row(
    this: &FileManagerPanel,
    index: usize,
    entry: DirEntry,
    depth: usize,
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
    // folders.
    let is_expanded = entry.is_dir && this.is_entry_expanded(&entry.path);
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
                // the 16 px placeholder so the icons stay aligned.
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "file-manager-expander-{}",
                            entry.path.display()
                        )))
                        .w(px(metrics::TREE_INDENT * depth as f32))
                        .flex_shrink_0()
                        .when(entry.is_dir, |expander| {
                            expander.child(
                                h_flex()
                                    .id(SharedString::from(format!(
                                        "file-manager-expander-hit-{}",
                                        entry.path.display()
                                    )))
                                    .w(px(16.))
                                    .h(px(16.))
                                    .flex_shrink_0()
                                    .items_center()
                                    .justify_center()
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
                                                this.toggle_entry_expanded(
                                                    &expand_path,
                                                    window,
                                                    cx,
                                                );
                                                cx.stop_propagation();
                                            },
                                        ),
                                    ),
                            )
                        })
                        .when(!entry.is_dir, |expander| {
                            expander.child(div().w(px(16.)).h(px(16.)).flex_shrink_0())
                        }),
                )
                .child(
                    Icon::new(icon)
                        .size(IconSize::Small)
                        .color(if entry.is_dir {
                            Color::Accent
                        } else {
                            Color::Muted
                        }),
                )
                .child(
                    Label::new(entry.name.clone())
                        .single_line()
                        .truncate()
                        .when(is_clipboard_source, |label| label.color(Color::Muted)),
                ),
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
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.selected.clear();
                this.selected.push(right_click_path.clone());
                if this.rename_state.is_some() {
                    this.cancel_rename(cx);
                }
                this.deploy_context_menu(event.position, window, cx);
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

/// The icon grid view: one tile per entry.
fn render_icon_grid(
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
fn render_tile(
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

/// The "Type" column: a human readable name for the entry.
fn type_label(entry: &DirEntry) -> String {
    if entry.is_dir {
        return "Folder".into();
    }
    let Some(extension) = entry.path.extension().and_then(|ext| ext.to_str()) else {
        return "File".into();
    };
    match extension.to_ascii_lowercase().as_str() {
        "txt" | "md" | "log" | "rs" | "toml" | "json" | "yaml" | "yml" | "js" | "ts" | "tsx"
        | "jsx" | "py" | "rb" | "go" | "c" | "h" | "cpp" | "hpp" | "java" | "css" | "html"
        | "sh" => "Text Document".into(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico" => "Image".into(),
        "mp3" | "wav" | "flac" | "ogg" => "Audio".into(),
        "mp4" | "mkv" | "mov" | "webm" | "avi" => "Video".into(),
        "zip" | "tar" | "gz" | "xz" | "bz2" | "7z" => "Archive".into(),
        "pdf" => "PDF Document".into(),
        other => other.to_uppercase(),
    }
}

/// The "Date modified" column: OpenXplorer shows today's files with a time,
/// older ones with the date.
pub(crate) fn format_modified(mtime: Option<fs::MTime>) -> String {
    let Some(mtime) = mtime else {
        return String::new();
    };
    let system_time = mtime.timestamp_for_user();
    let Ok(duration) = system_time.duration_since(std::time::UNIX_EPOCH) else {
        return String::new();
    };
    let Some(datetime) = chrono::DateTime::from_timestamp(duration.as_secs() as i64, 0) else {
        return String::new();
    };
    let local = datetime.with_timezone(&chrono::Local);
    let now = chrono::Local::now();
    if local.date_naive() == now.date_naive() {
        local.format("%-I:%M %p").to_string()
    } else {
        local.format("%m/%d/%Y").to_string()
    }
}
