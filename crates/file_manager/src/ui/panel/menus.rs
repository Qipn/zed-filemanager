//! The context menu and the command bar menus (`New`, `Sort`, `View`, `More`).
//!
//! These builders only construct a `ui::ContextMenu` and hand it to the panel's
//! `context_menu` slot; the panel owns the deployed menu and renders it.

use gpui::{
    Anchor, Context, DismissEvent, Focusable, IntoElement, Pixels, Point, Window, anchored,
    deferred,
};
use ui::prelude::*;

use super::{DeployedContextMenu, FileManagerPanel};
use crate::ui::views::ViewMode;
use crate::{
    Copy, Cut, Delete, NewFile, NewFolder, Open, Paste, Redo, Refresh, Rename, Trash, Undo,
};

impl FileManagerPanel {
    /// Deploys the context menu at the given window position (right-click).
    pub(crate) fn deploy_context_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let has_selection = !self.selected.is_empty();
        let has_clipboard = self.clipboard.is_some();
        let can_undo = self.undo_manager.can_undo();
        let can_redo = self.undo_manager.can_redo();
        let menu = ui::ContextMenu::build(window, cx, |menu, _, _| {
            menu.context(self.focus_handle.clone())
                .action_disabled_when(!has_selection, "Open", Box::new(Open))
                .separator()
                .action("New Folder", Box::new(NewFolder))
                .action("New File", Box::new(NewFile))
                .action("Refresh", Box::new(Refresh))
                .separator()
                .action_disabled_when(!has_selection, "Rename", Box::new(Rename))
                .action_disabled_when(!has_selection, "Copy", Box::new(Copy))
                .action_disabled_when(!has_selection, "Cut", Box::new(Cut))
                .action_disabled_when(!has_clipboard, "Paste", Box::new(Paste))
                .separator()
                .action_disabled_when(!can_undo, "Undo", Box::new(Undo))
                .action_disabled_when(!can_redo, "Redo", Box::new(Redo))
                .separator()
                .action_disabled_when(!has_selection, "Move to Trash", Box::new(Trash))
                .action_disabled_when(!has_selection, "Delete Permanently", Box::new(Delete))
        });
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

    /// The "New" menu of the command bar.
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

    /// The "Sort" menu of the command bar.
    pub(crate) fn deploy_sort_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity();
        let sort_mode = self.sort_mode;
        self.deploy_menu(
            position,
            move |menu, _, _| {
                menu.entry(
                    if sort_mode.is_name() {
                        "Name ✓"
                    } else {
                        "Name"
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
                        "Date modified ✓"
                    } else {
                        "Date modified"
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
                        "Size ✓"
                    } else {
                        "Size"
                    },
                    None,
                    {
                        let panel = panel.clone();
                        move |window, cx| {
                            panel.update(cx, |state, cx| state.sort_by_size(window, cx));
                        }
                    },
                )
            },
            window,
            cx,
        );
    }

    /// The "View" menu of the command bar.
    pub(crate) fn deploy_view_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity();
        let view_mode = self.view_mode;
        let show_hidden = self.show_hidden;
        let sidebar_visible = self.sidebar_visible;
        self.deploy_menu(
            position,
            move |menu, _, _| {
                menu.entry(
                    if view_mode == ViewMode::Details {
                        "Details ✓"
                    } else {
                        "Details"
                    },
                    None,
                    {
                        let panel = panel.clone();
                        move |_, cx| {
                            panel
                                .update(cx, |state, cx| state.set_view_mode(ViewMode::Details, cx));
                        }
                    },
                )
                .entry(
                    if view_mode == ViewMode::Grid {
                        "Large icons ✓"
                    } else {
                        "Large icons"
                    },
                    None,
                    {
                        let panel = panel.clone();
                        move |_, cx| {
                            panel.update(cx, |state, cx| state.set_view_mode(ViewMode::Grid, cx));
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
                .entry(
                    if sidebar_visible {
                        "Hide navigation pane"
                    } else {
                        "Show navigation pane"
                    },
                    None,
                    {
                        let panel = panel.clone();
                        move |_, cx| {
                            panel.update(cx, |state, cx| state.toggle_sidebar(cx));
                        }
                    },
                )
            },
            window,
            cx,
        );
    }

    /// The "More" menu of the command bar.
    pub(crate) fn deploy_more_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let has_selection = !self.selected.is_empty();
        let can_undo = self.undo_manager.can_undo();
        let can_redo = self.undo_manager.can_redo();
        self.deploy_menu(
            position,
            move |menu, _, _| {
                menu.action_disabled_when(!has_selection, "Rename", Box::new(Rename))
                    .action_disabled_when(!has_selection, "Copy", Box::new(Copy))
                    .action_disabled_when(!has_selection, "Cut", Box::new(Cut))
                    .separator()
                    .action_disabled_when(!can_undo, "Undo", Box::new(Undo))
                    .action_disabled_when(!can_redo, "Redo", Box::new(Redo))
                    .separator()
                    .action_disabled_when(!has_selection, "Move to Trash", Box::new(Trash))
                    .action_disabled_when(!has_selection, "Delete Permanently", Box::new(Delete))
            },
            window,
            cx,
        );
    }
}
