//! The context menu and the header bar's menus (`View`, `New`).
//!
//! These builders only construct a `ui::ContextMenu` and hand it to the panel's
//! `context_menu` slot; the panel owns the deployed menu and renders it.

use gpui::{
    Anchor, ClipboardItem, Context, DismissEvent, Focusable, IntoElement, Pixels, Point, Window,
    anchored, deferred,
};
use ui::prelude::*;
use workspace::OpenMode;

use super::{DeployedContextMenu, FileManagerPanel};
use crate::ui::panel::columns::Column;
use crate::ui::places::is_trash_path;
use crate::ui::views::ViewMode;
use crate::{
    Copy, Cut, Delete, NewFile, NewFolder, Open, OpenAsWorkspace, OpenInThisWindow, Paste, Redo,
    Refresh, Rename, SelectAll, ToggleBookmark, Trash, Undo,
};

impl FileManagerPanel {
    /// Deploys the context menu for the current state.
    ///
    /// Used by the keyboard (`OpenContextMenu`): with a selection it opens the
    /// item menu, otherwise the background menu — the same dispatch GNOME Files
    /// does when the menu key is pressed.
    pub(crate) fn deploy_context_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected.is_empty() {
            self.deploy_background_menu(position, window, cx);
        } else {
            self.deploy_item_menu(position, window, cx);
        }
    }

    /// The menu of the selected entries.
    ///
    /// Mirrors GNOME Files' selection menu
    /// (`nautilus-files-view.c::files_view_pop_up_selection_context_menu`):
    /// actions that operate on the selection, and nothing about the folder
    /// itself (no New Folder, Paste, Select All or Refresh).
    pub(crate) fn deploy_item_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let has_selection = !self.selected.is_empty();
        let multiple_selection = self.selected.len() > 1;
        let has_clipboard = self.clipboard.is_some();
        // Only a folder can become a workspace root (a file would silently open
        // its parent, which is not what the menu says).
        let selection_is_dir = self.selected_entry().is_some_and(|entry| entry.is_dir);
        let bookmark_label = self.bookmark_toggle_label();
        let focus_handle = self.focus_handle.clone();
        let panel = cx.entity();

        self.deploy_menu(
            position,
            move |menu, _, _| {
                let mut menu = menu
                    .context(focus_handle.clone())
                    .action_disabled_when(!has_selection, "Open", Box::new(Open))
                    // Two explicit entries instead of one that silently picks a
                    // mode: opening in this window switches workspace (and keeps
                    // the current one in the window's group), opening a new
                    // window leaves this one untouched.
                    .action_disabled_when(
                        !selection_is_dir,
                        "Open in This Window",
                        Box::new(OpenInThisWindow),
                    )
                    .action_disabled_when(
                        !selection_is_dir,
                        "Open in New Window",
                        Box::new(OpenAsWorkspace),
                    )
                    .separator()
                    .action_disabled_when(!has_selection, "Rename", Box::new(Rename));

                // Only meaningful for more than one item.
                if multiple_selection {
                    let panel = panel.clone();
                    menu = menu.entry("Batch rename…", None, move |window, cx| {
                        panel.update(cx, |state, cx| {
                            state.begin_batch_rename(window, cx);
                        });
                    });
                }

                menu.action_disabled_when(!has_selection, "Copy", Box::new(Copy))
                    .action_disabled_when(!has_selection, "Cut", Box::new(Cut))
                    .action_disabled_when(!has_clipboard, "Paste", Box::new(Paste))
                    .separator()
                    .action_disabled_when(!has_selection, "Move to Trash", Box::new(Trash))
                    .action_disabled_when(!has_selection, "Delete Permanently", Box::new(Delete))
                    .separator()
                    .action(bookmark_label, Box::new(ToggleBookmark))
            },
            window,
            cx,
        );
    }

    /// The menu of the view's empty space.
    ///
    /// GNOME Files clears the selection and then shows the folder's own menu
    /// (`nautilus-files-view.c::files_view_clicked_cb` →
    /// `nautilus_files_view_pop_up_background_context_menu`): only actions that
    /// act on the folder, never on an entry.
    pub(crate) fn deploy_background_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_selection(cx);
        if self.rename_state.is_some() {
            self.cancel_rename(window, cx);
        }

        let has_clipboard = self.clipboard.is_some();
        let has_entries = !self.entries.is_empty()
            || self
                .search_results
                .as_ref()
                .is_some_and(|results| !results.entries.is_empty());
        let can_undo = self.undo_manager.can_undo();
        let can_redo = self.undo_manager.can_redo();
        let has_current_dir = self.current_dir.is_some();
        let bookmark_label = self.bookmark_toggle_label();
        let focus_handle = self.focus_handle.clone();

        self.deploy_menu(
            position,
            move |menu, _, _| {
                menu.context(focus_handle.clone())
                    .action("New Folder", Box::new(NewFolder))
                    .action("New File", Box::new(NewFile))
                    .action_disabled_when(!has_clipboard, "Paste", Box::new(Paste))
                    .action_disabled_when(!has_entries, "Select All", Box::new(SelectAll))
                    .action("Refresh", Box::new(Refresh))
                    // The folder being browsed can become a workspace root, in
                    // this window (switching workspace) or in a new one.
                    .action_disabled_when(
                        !has_current_dir,
                        "Open in This Window",
                        Box::new(OpenInThisWindow),
                    )
                    .action_disabled_when(
                        !has_current_dir,
                        "Open in New Window",
                        Box::new(OpenAsWorkspace),
                    )
                    .separator()
                    .action_disabled_when(!can_undo, "Undo", Box::new(Undo))
                    .action_disabled_when(!can_redo, "Redo", Box::new(Redo))
                    .separator()
                    .action(bookmark_label, Box::new(ToggleBookmark))
            },
            window,
            cx,
        );
    }

    /// The label of the bookmark entry: adding or removing the current folder.
    fn bookmark_toggle_label(&self) -> &'static str {
        if self.current_dir_is_bookmarked() {
            "Remove from Bookmarks"
        } else {
            "Add to Bookmarks"
        }
    }

    /// Renders the deployed context menu, if any.
    pub(crate) fn render_context_menu(&self) -> Option<impl IntoElement> {
        let deployed = self.context_menu.as_ref()?;
        Some(
            deferred(
                anchored()
                    .position(deployed.position)
                    .anchor(Anchor::TopLeft)
                    .child(deployed.menu.clone()),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Deploys a menu built by `build` at the given position.
    fn deploy_menu(
        &mut self,
        position: Point<Pixels>,
        build: impl FnOnce(
            ui::ContextMenu,
            &mut Window,
            &mut Context<ui::ContextMenu>,
        ) -> ui::ContextMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = ui::ContextMenu::build(window, cx, build);
        window.focus(&menu.focus_handle(cx), cx);
        let subscription = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.context_menu.take();
            cx.notify();
        });
        self.context_menu = Some(DeployedContextMenu {
            menu,
            position,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// The filters of the search box, like GNOME Files' query editor popover
    /// (`nautilus-query-editor.blp`): scope, file kind, date and content.
    pub(crate) fn deploy_search_filter_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::model::search::SearchKind;

        let panel = cx.entity();
        let focus = self.focus_handle.clone();
        let options = self.search_options.clone();

        self.deploy_menu(
            position,
            move |menu, _, _| {
                let mut menu = menu
                    .context(focus)
                    .entry(
                        if options.recursive {
                            "Search in subfolders ✓"
                        } else {
                            "Search in subfolders"
                        },
                        None,
                        {
                            let panel = panel.clone();
                            move |window, cx| {
                                panel.update(cx, |state, cx| {
                                    state.search_options.recursive =
                                        !state.search_options.recursive;
                                    state.start_search(window, cx);
                                });
                            }
                        },
                    )
                    .entry(
                        if options.contents {
                            "Search file contents ✓"
                        } else {
                            "Search file contents"
                        },
                        None,
                        {
                            let panel = panel.clone();
                            move |window, cx| {
                                panel.update(cx, |state, cx| {
                                    state.search_options.contents = !state.search_options.contents;
                                    state.start_search(window, cx);
                                });
                            }
                        },
                    )
                    .separator();

                for kind in [
                    SearchKind::Any,
                    SearchKind::Images,
                    SearchKind::Documents,
                    SearchKind::Code,
                ] {
                    let panel = panel.clone();
                    let label = if options.kind == kind {
                        format!("{} ✓", kind.label())
                    } else {
                        kind.label().to_string()
                    };
                    menu = menu.entry(label, None, move |window, cx| {
                        panel.update(cx, |state, cx| {
                            state.search_options.kind = kind;
                            state.start_search(window, cx);
                        });
                    });
                }

                menu = menu.separator();
                for (label, days) in [
                    ("Any time", None),
                    ("Modified in the last 7 days", Some(7u32)),
                    ("Modified in the last 30 days", Some(30u32)),
                ] {
                    let panel = panel.clone();
                    let chosen = options.modified_within_days == days;
                    menu = menu.entry(
                        if chosen {
                            format!("{label} ✓")
                        } else {
                            label.to_string()
                        },
                        None,
                        move |window, cx| {
                            panel.update(cx, |state, cx| {
                                state.search_options.modified_within_days = days;
                                state.start_search(window, cx);
                            });
                        },
                    );
                }

                menu
            },
            window,
            cx,
        );
    }

    /// The context menu of a sidebar row.
    ///
    /// GNOME Files offers the same three actions on a places row: open the
    /// folder, pin or unpin it, and copy its location
    /// (`nautilus-sidebar.c::on_row_pressed`).
    pub(crate) fn deploy_place_menu(
        &mut self,
        path: std::path::PathBuf,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity();
        let focus = self.focus_handle.clone();
        let is_bookmark = self.is_bookmarked(&path);
        let location = path.display().to_string();
        // A place is a folder that can become a workspace root; the Trash is not
        // one (`Fs::trash` moves files out of it), so it is left out.
        let can_open_workspace = path.is_dir() && !is_trash_path(&path);

        self.deploy_menu(
            position,
            move |menu, _, _| {
                let mut menu = menu.context(focus).entry("Open", None, {
                    let panel = panel.clone();
                    let path = path.clone();
                    move |window, cx| {
                        panel.update(cx, |state, cx| {
                            state.navigate_to(path.clone(), window, cx);
                        });
                    }
                });

                menu = if is_bookmark {
                    menu.entry("Remove from Bookmarks", None, {
                        let panel = panel.clone();
                        let path = path.clone();
                        move |_, cx| {
                            panel.update(cx, |state, cx| state.remove_bookmark(&path, cx));
                        }
                    })
                } else {
                    menu.entry("Add to Bookmarks", None, {
                        let panel = panel.clone();
                        let path = path.clone();
                        move |_, cx| {
                            panel.update(cx, |state, cx| state.add_bookmark(path.clone(), cx));
                        }
                    })
                };

                if can_open_workspace {
                    menu = menu
                        .separator()
                        .entry("Open in This Window", None, {
                            let panel = panel.clone();
                            let path = path.clone();
                            move |window, cx| {
                                panel.update(cx, |state, cx| {
                                    state.open_path_as_workspace(
                                        path.clone(),
                                        OpenMode::Activate,
                                        window,
                                        cx,
                                    );
                                });
                            }
                        })
                        .entry("Open in New Window", None, {
                            let panel = panel.clone();
                            let path = path.clone();
                            move |window, cx| {
                                panel.update(cx, |state, cx| {
                                    state.open_path_as_workspace(
                                        path.clone(),
                                        OpenMode::NewWindow,
                                        window,
                                        cx,
                                    );
                                });
                            }
                        });
                }

                menu.entry("Copy Path", None, move |_, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(location.clone()));
                })
            },
            window,
            cx,
        );
    }

    /// The context menu of a folder-tree row.
    ///
    /// The tree had no menu at all, so a folder found there could not be opened
    /// as a workspace without first opening it in the folder view.
    pub(crate) fn deploy_tree_menu(
        &mut self,
        path: std::path::PathBuf,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity();
        let focus = self.focus_handle.clone();
        let location = path.display().to_string();
        let is_bookmark = self.is_bookmarked(&path);
        let bookmark_label = if is_bookmark {
            "Remove from Bookmarks"
        } else {
            "Add to Bookmarks"
        };

        self.deploy_menu(
            position,
            move |menu, _, _| {
                menu.context(focus)
                    .entry("Open", None, {
                        let panel = panel.clone();
                        let path = path.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| {
                                state.navigate_to(path.clone(), window, cx);
                            });
                        }
                    })
                    .separator()
                    .entry("Open in This Window", None, {
                        let panel = panel.clone();
                        let path = path.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| {
                                state.open_path_as_workspace(
                                    path.clone(),
                                    OpenMode::Activate,
                                    window,
                                    cx,
                                );
                            });
                        }
                    })
                    .entry("Open in New Window", None, {
                        let panel = panel.clone();
                        let path = path.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| {
                                state.open_path_as_workspace(
                                    path.clone(),
                                    OpenMode::NewWindow,
                                    window,
                                    cx,
                                );
                            });
                        }
                    })
                    .separator()
                    .entry(bookmark_label, None, {
                        let panel = panel.clone();
                        let path = path.clone();
                        move |_, cx| {
                            panel.update(cx, |state, cx| {
                                if is_bookmark {
                                    state.remove_bookmark(&path, cx);
                                } else {
                                    state.add_bookmark(path.clone(), cx);
                                }
                            });
                        }
                    })
                    .entry("Copy Path", None, move |_, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(location.clone()));
                    })
            },
            window,
            cx,
        );
    }

    /// The menu behind the `…` of a truncated breadcrumb: every segment that did
    /// not fit, so the user can still reach any ancestor in a narrow dock.
    pub(crate) fn deploy_ancestor_menu(
        &mut self,
        segments: Vec<(std::path::PathBuf, gpui::SharedString)>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus_handle = self.focus_handle.clone();
        let panel = cx.entity();
        self.deploy_menu(
            position,
            move |menu, _, _| {
                let mut menu = menu.context(focus_handle.clone());
                for (index, (target, name)) in segments.into_iter().enumerate() {
                    // The root's label is `/` already; the rest use their path
                    // component, which is what the breadcrumb shows.
                    let label = if index == 0 && name.is_empty() {
                        gpui::SharedString::from("/")
                    } else {
                        name
                    };
                    menu = menu.entry(label, None, {
                        let panel = panel.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| {
                                state.navigate_to(target.clone(), window, cx);
                            });
                        }
                    });
                }
                menu
            },
            window,
            cx,
        );
    }

    /// The "New" menu of the header bar's new control.
    ///
    /// Mirrors OpenXplorer's `openNewMenu`
    /// (`native/crates/ox-app/src/window/command_bar/menus.rs`): a folder, a
    /// plain text document, a free file, then the typed documents.
    pub(crate) fn deploy_new_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity();

        // Each typed entry creates an empty file with the matching name.
        let typed = [
            ("Markdown document", "New Markdown document.md"),
            ("CSV file", "New CSV file.csv"),
            ("JSON file", "New JSON file.json"),
            ("HTML document", "New HTML document.html"),
        ];

        self.deploy_menu(
            position,
            move |menu, _, _| {
                let mut menu = menu
                    .entry("Folder", None, {
                        let panel = panel.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| state.create_folder(window, cx));
                        }
                    })
                    .entry("Text document", None, {
                        let panel = panel.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| {
                                state.create_file_named("New Text Document.txt", window, cx)
                            });
                        }
                    })
                    .entry("File", None, {
                        let panel = panel.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| state.create_file(window, cx));
                        }
                    })
                    .separator();

                for (label, base_name) in typed {
                    let panel = panel.clone();
                    menu = menu.entry(label, None, move |window, cx| {
                        panel.update(cx, |state, cx| {
                            state.create_file_named(base_name, window, cx)
                        });
                    });
                }

                menu
            },
            window,
            cx,
        );
    }

    /// The view menu of the header bar.
    ///
    /// Follows GNOME Files' `view_menu` (`nautilus-view-controls.blp`): the
    /// icon size first, then the sorting options, then "Show hidden files".
    /// The view-mode entries and the details-pane toggle are ours; the mode
    /// itself is also one click away on the header bar's split button.
    pub(crate) fn deploy_view_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity();
        let view_mode = self.view_mode;
        let sort_mode = self.sort_mode;
        let show_hidden = self.show_hidden;
        let details_pane_visible = self.details_pane_visible;
        // Column visibility is a user choice, the same one Nautilus puts behind
        // its column chooser; the labels carry the check mark like the entries
        // above.
        let column_labels: Vec<(&'static str, Column, bool)> = Column::ALL
            .iter()
            .map(|column| {
                let shown = self.columns_shown[column.index()];
                (column.label(), *column, shown)
            })
            .collect();

        self.deploy_menu(
            position,
            move |menu, _, _| {
                let mut menu = menu
                    .entry("Smaller icons", None, {
                        let panel = panel.clone();
                        move |_, cx| {
                            panel.update(cx, |state, cx| state.decrease_icon_size(cx));
                        }
                    })
                    .entry("Larger icons", None, {
                        let panel = panel.clone();
                        move |_, cx| {
                            panel.update(cx, |state, cx| state.increase_icon_size(cx));
                        }
                    })
                    .separator()
                    .entry(
                        if sort_mode.is_name() {
                            "Sort by name ✓"
                        } else {
                            "Sort by name"
                        },
                        None,
                        {
                            let panel = panel.clone();
                            move |window, cx| {
                                panel.update(cx, |state, cx| state.sort_by_name(window, cx));
                            }
                        },
                    )
                    .entry(
                        if sort_mode.is_modified() {
                            "Sort by date modified ✓"
                        } else {
                            "Sort by date modified"
                        },
                        None,
                        {
                            let panel = panel.clone();
                            move |window, cx| {
                                panel.update(cx, |state, cx| state.sort_by_modified(window, cx));
                            }
                        },
                    )
                    .entry(
                        if sort_mode.is_size() {
                            "Sort by size ✓"
                        } else {
                            "Sort by size"
                        },
                        None,
                        {
                            let panel = panel.clone();
                            move |window, cx| {
                                panel.update(cx, |state, cx| state.sort_by_size(window, cx));
                            }
                        },
                    )
                    .separator()
                    .entry(
                        if show_hidden {
                            "Hide hidden files"
                        } else {
                            "Show hidden files"
                        },
                        None,
                        {
                            let panel = panel.clone();
                            move |window, cx| {
                                panel.update(cx, |state, cx| state.toggle_hidden(window, cx));
                            }
                        },
                    )
                    .separator()
                    .entry(
                        if view_mode == ViewMode::Details {
                            "List view ✓"
                        } else {
                            "List view"
                        },
                        None,
                        {
                            let panel = panel.clone();
                            move |_, cx| {
                                panel.update(cx, |state, cx| {
                                    state.set_view_mode(ViewMode::Details, cx)
                                });
                            }
                        },
                    )
                    .entry(
                        if view_mode == ViewMode::Grid {
                            "Grid view ✓"
                        } else {
                            "Grid view"
                        },
                        None,
                        {
                            let panel = panel.clone();
                            move |_, cx| {
                                panel.update(cx, |state, cx| {
                                    state.set_view_mode(ViewMode::Grid, cx)
                                });
                            }
                        },
                    )
                    .entry(
                        if details_pane_visible {
                            "Hide details pane"
                        } else {
                            "Show details pane"
                        },
                        None,
                        {
                            let panel = panel.clone();
                            move |_, cx| {
                                panel.update(cx, |state, cx| state.toggle_details_pane(cx));
                            }
                        },
                    );

                // Which columns the details view shows. Only meaningful in the
                // list view, so the block is hidden for the grid.
                if view_mode == ViewMode::Details {
                    menu = menu.separator();
                    for (label, column, shown) in column_labels {
                        let panel = panel.clone();
                        menu = menu.entry(
                            if shown {
                                format!("{label} column ✓")
                            } else {
                                format!("{label} column")
                            },
                            None,
                            move |_, cx| {
                                panel.update(cx, |state, cx| state.toggle_column(column, cx));
                            },
                        );
                    }
                    let panel = panel.clone();
                    menu = menu.entry("Reset column widths", None, move |_, cx| {
                        panel.update(cx, |state, cx| state.reset_column_widths(cx));
                    });
                }

                menu
            },
            window,
            cx,
        );
    }
}
