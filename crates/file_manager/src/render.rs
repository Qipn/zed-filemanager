//! Rendering for `FileManagerPanel`, laid out like OpenXplorer.
//!
//! The band order follows the upstream application
//! (`native/docs/ui-spec.md` §1.1):
//!
//! ```text
//! navigation row (back, forward, up, refresh, address bar)
//! command bar   (New | Cut Copy Paste Rename Delete | Sort View More)
//! body          (sidebar: places + folder tree | folder view)
//! status bar    (count, selection, … , view toggles)
//! ```
//!
//! The view layer lives in `views.rs` (details and grid views) and
//! `sidebar.rs` (places and folder tree).

use gpui::{
    AnyElement, Context, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, Window, canvas,
    div, px,
};
use theme::ActiveTheme;
use ui::{
    Button, ButtonSize, ButtonStyle, Color, Icon, IconButton, IconName, IconSize, Label, LabelSize,
    TintColor, Tooltip, prelude::*,
};

use crate::clipboard::Clipboard;
use crate::conflict::ConflictPolicy;
use crate::ox_theme::{metrics, palette};
use crate::panel::FileManagerPanel;
use crate::views::{ViewMode, render_view};
use crate::{
    Copy, Cut, Delete, NewFile, NewFolder, Open, OpenContextMenu, Paste, Redo, Refresh, Rename,
    ToggleSidebar, ToggleViewMode, Trash, Undo,
};

/// Renders the whole panel.
pub(super) fn render(
    this: &mut FileManagerPanel,
    window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> impl IntoElement {
    v_flex()
        .size_full()
        .key_context("FileManagerPanel")
        .bg(cx.theme().colors().panel_background)
        .track_focus(&this.focus_handle)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                window.focus(&this.focus_handle, cx);
            }),
        )
        // The sidebar resizer drags through GPUI's drag-and-drop system, like
        // Zed's dock (`crates/workspace/src/dock.rs`): the handle only starts a
        // drag and the panel resizes on every drag move, so releasing the mouse
        // always ends the resize.
        .on_drag_move::<crate::panel::SidebarResizeDrag>(cx.listener(
            |this, event: &gpui::DragMoveEvent<crate::panel::SidebarResizeDrag>, _, cx| {
                let width = f32::from(event.event.position.x - event.bounds.left());
                this.set_sidebar_width(width, cx);
            },
        ))
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
            this.handle_key_down(event, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Open, window, cx| {
            this.open_selected(window, cx);
        }))
        .on_action(cx.listener(|this, _: &OpenContextMenu, window, cx| {
            this.deploy_context_menu(window.mouse_position(), window, cx);
        }))
        .on_action(cx.listener(|this, _: &NewFolder, window, cx| {
            this.create_folder(window, cx);
        }))
        .on_action(cx.listener(|this, _: &NewFile, window, cx| {
            this.create_file(window, cx);
        }))
        .on_action(cx.listener(|this, _: &Refresh, window, cx| {
            this.refresh(window, cx);
        }))
        .on_action(cx.listener(|this, _: &Rename, window, cx| {
            this.rename_selected(window, cx);
        }))
        .on_action(cx.listener(|this, _: &Copy, _, cx| {
            this.copy_selected(cx);
        }))
        .on_action(cx.listener(|this, _: &Cut, _, cx| {
            this.cut_selected(cx);
        }))
        .on_action(cx.listener(|this, _: &Paste, window, cx| {
            this.paste_clipboard(window, cx);
        }))
        .on_action(cx.listener(|this, _: &Trash, window, cx| {
            this.confirm_remove(true, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Delete, window, cx| {
            this.confirm_remove(false, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Undo, window, cx| {
            this.undo_last(window, cx);
        }))
        .on_action(cx.listener(|this, _: &Redo, window, cx| {
            this.redo_last(window, cx);
        }))
        .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
            this.toggle_sidebar(cx);
        }))
        .on_action(cx.listener(|this, _: &ToggleViewMode, _, cx| {
            this.set_view_mode(
                match this.view_mode {
                    crate::views::ViewMode::Details => crate::views::ViewMode::Grid,
                    crate::views::ViewMode::Grid => crate::views::ViewMode::Details,
                },
                cx,
            );
        }))
        .child(render_navigation_row(this, window, cx))
        .child(render_command_bar(this, window, cx))
        .children(render_conflict_banner(this, cx))
        .child(render_body(this, window, cx))
        .child(render_status_bar(this, cx))
        .children(this.render_context_menu())
        // Measure the panel itself: a Zed dock can be much narrower than the
        // window, so the layout breakpoints follow the panel's own width.
        .child({
            let panel = cx.entity().downgrade();
            let measured = this.content_width;
            canvas(
                move |bounds, window, cx| {
                    let width = f32::from(bounds.size.width);
                    if measured == Some(width.round()) {
                        return;
                    }
                    window.defer(cx, move |_window, cx| {
                        panel
                            .update(cx, |this, cx| this.set_content_width(width, cx))
                            .ok();
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full()
        })
}

/// The navigation row: back, forward, up, refresh and the address bar.
fn render_navigation_row(
    this: &mut FileManagerPanel,
    _window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());

    h_flex()
        .id("file-manager-navigation-row")
        .h(px(metrics::NAV_ROW))
        .w_full()
        .flex_shrink_0()
        .gap(px(8.))
        .px(px(15.))
        .bg(palette.chrome)
        .border_b_1()
        .border_color(palette.divider)
        .child(
            IconButton::new("file-manager-back", IconName::ArrowLeft)
                .icon_size(IconSize::Small)
                .disabled(!this.can_go_back())
                .on_click(cx.listener(|this, _, window, cx| this.go_back(window, cx))),
        )
        .child(
            IconButton::new("file-manager-forward", IconName::ArrowRight)
                .icon_size(IconSize::Small)
                .disabled(!this.can_go_forward())
                .on_click(cx.listener(|this, _, window, cx| this.go_forward(window, cx))),
        )
        .child(
            IconButton::new("file-manager-up", IconName::ArrowUp)
                .icon_size(IconSize::Small)
                .disabled(this.current_dir.is_none())
                .on_click(cx.listener(|this, _, window, cx| this.go_up(window, cx))),
        )
        .child(
            IconButton::new("file-manager-refresh", IconName::RotateCw)
                .icon_size(IconSize::Small)
                .disabled(this.current_dir.is_none())
                .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
        )
        .child(render_address_bar(this, cx))
        .children(if this.is_search_box_shown() {
            Some(render_search_box(this, cx))
        } else {
            None
        })
        .into_any_element()
}

/// The search box at the right of the navigation row.
///
/// OpenXplorer searches the whole location tree; the panel filters the
/// contents of the current directory instead.
fn render_search_box(this: &FileManagerPanel, cx: &mut Context<FileManagerPanel>) -> AnyElement {
    let palette = palette(cx.theme());

    match &this.search_query {
        Some(query) => div()
            .id("file-manager-search-box")
            .h(px(32.))
            .w(px(metrics::SEARCH_WIDTH))
            .flex_shrink_0()
            .px(px(8.))
            .items_center()
            .gap(px(6.))
            .rounded_xs()
            .bg(palette.field_bg)
            .border_1()
            .border_color(palette.border)
            .child(
                Icon::new(IconName::MagnifyingGlass)
                    .size(IconSize::XSmall)
                    .color(Color::Custom(palette.muted)),
            )
            .child(
                Label::new(SharedString::from(format!("{query}|")))
                    .single_line()
                    .truncate()
                    .flex_1(),
            )
            .child(
                IconButton::new("file-manager-search-close", IconName::Close)
                    .icon_size(IconSize::XSmall)
                    .tooltip(Tooltip::text("Close the search"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_search(cx);
                    })),
            )
            .into_any_element(),
        None => IconButton::new("file-manager-search", IconName::MagnifyingGlass)
            .icon_size(IconSize::Small)
            .tooltip(Tooltip::text("Search"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_search(cx);
            }))
            .into_any_element(),
    }
}

/// The address bar: the breadcrumb path, or the edit field.
fn render_address_bar(this: &FileManagerPanel, cx: &mut Context<FileManagerPanel>) -> AnyElement {
    let palette = palette(cx.theme());

    if let Some(buffer) = &this.path_edit {
        return div()
            .id("file-manager-address-edit")
            .h(px(32.))
            .flex_1()
            .min_w(px(140.))
            .px(px(11.))
            .items_center()
            .rounded_xs()
            .bg(palette.field_bg)
            .border_1()
            .border_color(palette.border)
            .child(
                Label::new(SharedString::from(format!("{buffer}|")))
                    .single_line()
                    .truncate(),
            )
            .into_any_element();
    }

    let segments = this.breadcrumb_targets();
    let mut crumbs = h_flex().gap(px(2.)).flex_1().min_w(px(0.));

    for (index, (target, name)) in segments.iter().enumerate() {
        let is_last = index + 1 == segments.len();
        if index > 0 {
            // The separator sits in front of every crumb but the first.
            crumbs = crumbs.child(
                Label::new("/")
                    .size(LabelSize::Small)
                    .color(Color::Custom(palette.text_tertiary)),
            );
        }
        let target = target.clone();
        let name = name.clone();
        crumbs =
            crumbs.child(
                h_flex()
                    .id(("file-manager-crumb", index))
                    .h(px(26.))
                    .px(px(9.))
                    .rounded_xs()
                    .hover(|style| style.bg(palette.hover))
                    .child(Label::new(name).single_line().size(LabelSize::Small).color(
                        if is_last {
                            Color::Custom(palette.text)
                        } else {
                            Color::Custom(palette.muted)
                        },
                    ))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.deploy_breadcrumb_menu(target.clone(), event.position, window, cx);
                        }),
                    ),
            );
    }

    div()
        .id("file-manager-address")
        .h(px(32.))
        .flex_1()
        .min_w(px(140.))
        .px(px(8.))
        .py(px(2.))
        .items_center()
        .rounded_xs()
        .bg(palette.field_bg)
        .border_1()
        .border_color(palette.border)
        .child(
            Icon::new(IconName::Folder)
                .size(IconSize::Small)
                .color(Color::Custom(palette.muted)),
        )
        .child(crumbs)
        .child(
            IconButton::new("file-manager-edit-path", IconName::ChevronDown)
                .icon_size(IconSize::XSmall)
                .tooltip(Tooltip::text("Edit location (Ctrl+L)"))
                .on_click(cx.listener(|this, _, _, cx| this.begin_path_edit(cx))),
        )
        .into_any_element()
}

/// The command bar: New, the file commands, Sort, View and More.
fn render_command_bar(
    this: &mut FileManagerPanel,
    _window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());
    let has_selection = !this.selected.is_empty();

    let separator = || div().w(px(1.)).h(px(20.)).mx(px(10.)).bg(palette.divider);

    h_flex()
        .id("file-manager-command-bar")
        .h(px(metrics::COMMAND_BAR))
        .w_full()
        .flex_shrink_0()
        .items_center()
        .gap(px(4.))
        .px(px(15.))
        .bg(palette.chrome)
        .border_b_1()
        .border_color(palette.divider)
        // New ▾
        .child(command_button(
            "file-manager-new",
            "New",
            "New".into(),
            this.current_dir.is_none(),
            &palette,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.deploy_new_menu(event.position, window, cx);
            }),
        ))
        .child(separator())
        // Cut, Copy, Paste, Rename, Delete
        .child(
            IconButton::new("file-manager-cut", IconName::Scissors)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Cut (Ctrl+X)"))
                .disabled(!has_selection)
                .on_click(cx.listener(|this, _, _, cx| this.cut_selected(cx))),
        )
        .child(
            IconButton::new("file-manager-copy", IconName::Copy)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Copy (Ctrl+C)"))
                .disabled(!has_selection)
                .on_click(cx.listener(|this, _, _, cx| this.copy_selected(cx))),
        )
        .child(
            IconButton::new("file-manager-paste", IconName::Download)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Paste files (Ctrl+V)"))
                .disabled(this.clipboard.is_none() || this.current_dir.is_none())
                .on_click(cx.listener(|this, _, window, cx| this.paste_clipboard(window, cx))),
        )
        .child(
            IconButton::new("file-manager-rename", IconName::Pencil)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Rename (F2)"))
                .disabled(!has_selection)
                .on_click(cx.listener(|this, _, window, cx| this.rename_selected(window, cx))),
        )
        .child(
            IconButton::new("file-manager-trash", IconName::Trash)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Move to Trash"))
                .disabled(!has_selection)
                .on_click(cx.listener(|this, _, window, cx| this.confirm_remove(true, window, cx))),
        )
        .child(separator())
        // Sort ▾
        .child(command_button(
            "file-manager-sort",
            "Sort",
            format!("Sort: {}", this.sort_mode.label()).into(),
            false,
            &palette,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.deploy_sort_menu(event.position, window, cx);
            }),
        ))
        // View ▾
        .child(command_button(
            "file-manager-view",
            "View",
            "View".into(),
            false,
            &palette,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.deploy_view_menu(event.position, window, cx);
            }),
        ))
        // More ···
        .child(
            IconButton::new("file-manager-more", IconName::Ellipsis)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("More options"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.deploy_more_menu(window.mouse_position(), window, cx);
                })),
        )
        .child(div().flex_1())
        // The details pane toggle, like OpenXplorer's command bar.
        .child(
            IconButton::new("file-manager-details-toggle", IconName::Split)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("Details pane"))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_details_pane(cx))),
        )
        // The sidebar toggle lives on the right of the command bar.
        .child(
            IconButton::new(
                "file-manager-sidebar-toggle",
                if this.sidebar_visible {
                    IconName::ThreadsSidebarLeftOpen
                } else {
                    IconName::ThreadsSidebarLeftClosed
                },
            )
            .icon_size(IconSize::Small)
            .tooltip(Tooltip::text("Navigation pane (F9)"))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
        )
        .into_any_element()
}

/// A command bar button with a text label and a chevron, like OpenXplorer's
/// "New ▾", "Sort ▾" and "View ▾".
fn command_button(
    id: &'static str,
    label: impl Into<SharedString>,
    tooltip: SharedString,
    disabled: bool,
    palette: &crate::ox_theme::OxPalette,
    on_click: impl Fn(&MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .tooltip(Tooltip::text(tooltip))
        .h(px(34.))
        .px(px(11.))
        .items_center()
        .gap(px(4.))
        .rounded_xs()
        .opacity(if disabled { 0.5 } else { 1. })
        .hover(|style| style.bg(palette.hover))
        .child(Label::new(label).size(LabelSize::Small))
        .child(
            Icon::new(IconName::ChevronDown)
                .size(IconSize::XSmall)
                .color(Color::Custom(palette.muted)),
        )
        .on_mouse_down(MouseButton::Left, on_click)
        .into_any_element()
}

/// The body: the sidebar and the folder view.
fn render_body(
    this: &mut FileManagerPanel,
    window: &mut Window,
    cx: &mut Context<FileManagerPanel>,
) -> AnyElement {
    let palette = palette(cx.theme());

    let content = v_flex()
        .id("file-manager-content")
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden()
        .child(
            v_flex()
                .id("file-manager-entries-wrapper")
                .flex_1()
                .overflow_hidden()
                // Like Zed's project_panel, the whole pane lights up with
                // `drop_target_background` when files are dragged over it.
                .drag_over::<crate::panel::DraggedPaths>(|style, _, _, cx| {
                    style.bg(cx.theme().colors().drop_target_background)
                })
                .drag_over::<gpui::ExternalPaths>(|style, _, _, cx| {
                    style.bg(cx.theme().colors().drop_target_background)
                })
                .on_drag_move::<crate::panel::DraggedPaths>(cx.listener(
                    move |this, event: &gpui::DragMoveEvent<crate::panel::DraggedPaths>, _, cx| {
                        let hovered = event.bounds.contains(&event.event.position);
                        if hovered && this.drag_target.is_none() {
                            if let Some(dir) = this.current_dir.clone() {
                                this.drag_target = Some(dir);
                                cx.notify();
                            }
                        }
                    },
                ))
                .on_drag_move::<gpui::ExternalPaths>(cx.listener(
                    move |this, event: &gpui::DragMoveEvent<gpui::ExternalPaths>, _, cx| {
                        let hovered = event.bounds.contains(&event.event.position);
                        if hovered && this.drag_target.is_none() {
                            if let Some(dir) = this.current_dir.clone() {
                                this.drag_target = Some(dir);
                                cx.notify();
                            }
                        }
                    },
                ))
                .on_drop(cx.listener(
                    move |this, dragged: &crate::panel::DraggedPaths, window, cx| {
                        if let Some(dir) = this.current_dir.clone() {
                            this.drop_dragged(
                                crate::panel::DraggedPaths(dragged.0.clone()),
                                dir,
                                window,
                                cx,
                            );
                        }
                    },
                ))
                .on_drop(
                    cx.listener(move |this, external: &gpui::ExternalPaths, window, cx| {
                        if let Some(dir) = this.current_dir.clone() {
                            let paths: Vec<std::path::PathBuf> =
                                external.0.iter().cloned().collect();
                            this.drop_external(paths, dir, window, cx);
                        }
                    }),
                )
                .child(render_view(this, window, cx)),
        );

    if !this.is_sidebar_shown() {
        return content.into_any_element();
    }

    h_flex()
        .flex_1()
        .min_h(px(0.))
        .child(crate::sidebar::render_sidebar(this, window, cx))
        // The resizer between the sidebar and the content. It starts a drag
        // instead of tracking the mouse itself, so GPUI ends the resize when
        // the button is released (see Zed's dock resize handle).
        .child(
            div()
                .id("file-manager-sidebar-resizer")
                .w(px(metrics::SIDEBAR_RESIZER))
                .h_full()
                .flex_shrink_0()
                .cursor(gpui::CursorStyle::ResizeLeftRight)
                .hover(|style| style.bg(palette.drop_target))
                .on_drag(crate::panel::SidebarResizeDrag, |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| crate::panel::SidebarResizeDrag)
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                    }),
                )
                // Double click restores OpenXplorer's default width.
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                        if event.click_count == 2 {
                            this.reset_sidebar_width(cx);
                            cx.stop_propagation();
                        }
                    }),
                )
                .occlude(),
        )
        .child(content)
        .when(this.is_details_pane_shown(), |body| {
            body.children(crate::details_pane::render_details_pane(this, window, cx))
        })
        .into_any_element()
}

/// Renders the conflict prompt shown when a transfer would overwrite
/// existing entries.
fn render_conflict_banner(
    this: &FileManagerPanel,
    cx: &mut Context<FileManagerPanel>,
) -> Option<AnyElement> {
    let pending = this.pending_transfer.as_ref()?;
    let palette = palette(cx.theme());
    let count = pending.conflicts.len();
    let names: Vec<String> = pending
        .conflicts
        .iter()
        .filter_map(|index| pending.targets.get(*index))
        .filter_map(|target| target.file_name())
        .map(|name| name.to_string_lossy().to_string())
        .take(3)
        .collect();
    let mut summary = format!("{count} item(s) already exist");
    if !names.is_empty() {
        summary.push_str(": ");
        summary.push_str(&names.join(", "));
        if count > names.len() {
            summary.push_str("…");
        }
    }

    Some(
        h_flex()
            .id("file-manager-conflict-banner")
            .flex_1()
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
            .child(
                Button::new(
                    "file-manager-conflict-replace",
                    ConflictPolicy::Replace.label(),
                )
                .size(ButtonSize::Compact)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.resolve_conflicts(ConflictPolicy::Replace, window, cx);
                })),
            )
            .child(
                Button::new("file-manager-conflict-skip", ConflictPolicy::Skip.label())
                    .size(ButtonSize::Compact)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.resolve_conflicts(ConflictPolicy::Skip, window, cx);
                    })),
            )
            .child(
                Button::new(
                    "file-manager-conflict-keep-both",
                    ConflictPolicy::KeepBoth.label(),
                )
                .size(ButtonSize::Compact)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.resolve_conflicts(ConflictPolicy::KeepBoth, window, cx);
                })),
            )
            .child(
                Button::new("file-manager-conflict-cancel", "Cancel")
                    .size(ButtonSize::Compact)
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.cancel_conflicts(cx);
                    })),
            )
            .into_any_element(),
    )
}

/// The status bar: the item count, the selection and the view toggles.
fn render_status_bar(this: &FileManagerPanel, cx: &mut Context<FileManagerPanel>) -> AnyElement {
    let palette = palette(cx.theme());

    let mut status = format!("{} items", this.entries.len());
    // OpenXplorer shows the total size next to the item count.
    let total: u64 = this.entries.iter().filter_map(|entry| entry.len).sum();
    if total > 0 {
        status.push_str(&format!("  {}", crate::entries::format_len(total)));
    }
    match this.selected.len() {
        0 => {}
        1 => {
            if let Some(entry) = this.selected_entry() {
                let size = entry
                    .len
                    .map(crate::entries::format_len)
                    .unwrap_or_else(|| "—".into());
                status.push_str(&format!(" — {} ({})", entry.name, size));
            }
        }
        count => status.push_str(&format!(" — {count} selected")),
    }
    if this.using_fallback_dir {
        status.push_str(" — no workspace folder");
    }
    let clipboard_hint = match &this.clipboard {
        Some(Clipboard::Copied(_)) => " (copied)",
        Some(Clipboard::Cut(_)) => " (cut)",
        None => "",
    };

    h_flex()
        .id("file-manager-status-bar")
        .h(px(metrics::STATUS_BAR))
        .w_full()
        .flex_shrink_0()
        .items_center()
        .gap(px(18.))
        .px(px(13.))
        .bg(palette.bg)
        .border_t_1()
        .border_color(palette.divider)
        .child(
            Label::new(format!("{status}{clipboard_hint}"))
                .single_line()
                .size(LabelSize::XSmall)
                .color(Color::Custom(palette.muted)),
        )
        .child(div().flex_1())
        // The icon size stepper, only in the icon grid view.
        .when(this.view_mode == ViewMode::Grid, |this| {
            this.child(
                IconButton::new("file-manager-icon-smaller", IconName::SquareMinus)
                    .icon_size(IconSize::XSmall)
                    .tooltip(Tooltip::text("Smaller icons"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.decrease_icon_size(cx);
                    })),
            )
            .child(
                IconButton::new("file-manager-icon-larger", IconName::Plus)
                    .icon_size(IconSize::XSmall)
                    .tooltip(Tooltip::text("Larger icons"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.increase_icon_size(cx);
                    })),
            )
        })
        // The view toggles, at the right of the status bar. The active view
        // is highlighted like OpenXplorer's status mode (§4.7).
        .child(
            IconButton::new("file-manager-view-details", IconName::Table)
                .icon_size(IconSize::XSmall)
                .tooltip(Tooltip::text("Details view"))
                .toggle_state(this.view_mode == ViewMode::Details)
                .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.view_mode != ViewMode::Details {
                        this.set_view_mode(ViewMode::Details, cx);
                    }
                })),
        )
        .child(
            IconButton::new("file-manager-view-icons", IconName::FolderOpen)
                .icon_size(IconSize::XSmall)
                .tooltip(Tooltip::text("Large icons"))
                .toggle_state(this.view_mode == ViewMode::Grid)
                .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.view_mode != ViewMode::Grid {
                        this.set_view_mode(ViewMode::Grid, cx);
                    }
                })),
        )
        .into_any_element()
}
