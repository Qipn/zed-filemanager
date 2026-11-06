//! Render smoke tests for the file manager panel.
//!
//! They build a workspace with a temporary directory and draw the panel, so
//! that the layout (header bar, sidebar with the places list and the folder
//! tree, details view, details pane, status bar) is exercised without opening
//! the editor.

use fs::Fs as _;
use gpui::{AppContext, TestAppContext, WindowOptions};
use project::Project;
use settings::SettingsStore;
use workspace::Workspace;

use crate::FileManagerPanel;
use crate::ui::views::ViewMode;

/// Sets up the globals the panel needs while it renders.
fn init_globals(cx: &mut TestAppContext) {
    cx.update(|cx| {
        if !cx.has_global::<SettingsStore>() {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
        }
        theme_settings::init(theme::LoadThemes::JustBase, cx);
    });
}

/// Draws the panel in a window and fails on any panic.
#[gpui::test]
async fn test_render_panel(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    let project = Project::test(fs.clone(), ["/".as_ref()], cx).await;

    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        FileManagerPanel::new(workspace, window, cx)
    });

    // The panel is the root view of a second window, so opening that window
    // draws the whole layout.
    cx.update(|_window, cx| {
        cx.open_window(WindowOptions::default(), |_window, _cx| panel.clone())
            .expect("failed to open the test window");
    });
}

/// The search entry accepts typed characters once it is open.
///
/// Regression test for "clicking the search box does not let you type": the
/// entry must get the focus when the search opens, and the keystrokes must end
/// up in its text (not only in the platform's text-input path, which needs a
/// registered input handler).
#[gpui::test]
async fn test_typing_in_the_search_field(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    let project = Project::test(fs.clone(), ["/".as_ref()], cx).await;

    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        FileManagerPanel::new(workspace, window, cx)
    });

    let window = cx.update(|_window, cx| {
        cx.open_window(WindowOptions::default(), |_window, _cx| panel.clone())
            .expect("failed to open the test window")
    });
    let window = window.into();

    // Opening the search focuses the entry (GNOME swaps the toolbar to its
    // search page and grabs the focus).
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| panel.toggle_search(window, cx));
    })
    .unwrap();

    // The two-argument form dispatches into the given window (the panel's own),
    // through the same `Window::dispatch_keystroke` the platform uses.
    TestAppContext::simulate_keystrokes(&mut *cx, window, "a b");

    panel.read_with(cx, |panel, cx| {
        assert_eq!(
            panel.search_field.read(cx).content(),
            "ab",
            "keystrokes reach the search entry"
        );
        assert_eq!(panel.search_query.as_deref(), Some("ab"));
    });
}

/// The location entry can be cancelled and confirmed by the panel's actions.
///
/// Escape and Enter never reach the text fields directly: the default keymap
/// binds them (`menu::Cancel` / `menu::Confirm`) and key bindings are dispatched
/// before the fields' `on_key_down` listeners. The `FileManagerPanel` context
/// therefore routes both to `CancelInput` / `ConfirmInput`.
#[gpui::test]
async fn test_cancel_and_confirm_the_location_entry(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    let project = Project::test(fs.clone(), ["/".as_ref()], cx).await;

    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        FileManagerPanel::new(workspace, window, cx)
    });

    let window = cx.update(|_window, cx| {
        cx.open_window(WindowOptions::default(), |_window, _cx| panel.clone())
            .expect("failed to open the test window")
    });
    let window = window.into();

    // Opening the editor fills it with the current path and selects it.
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| panel.begin_path_edit(window, cx));
    })
    .unwrap();
    panel.read_with(cx, |panel, cx| {
        assert!(panel.path_editing, "the editor is open");
        assert!(
            !panel.path_field.read(cx).content().is_empty(),
            "the entry is prefilled with the current path"
        );
    });

    // `CancelInput` closes it without navigating.
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| panel.cancel_active_input(window, cx));
    })
    .unwrap();
    panel.read_with(cx, |panel, _cx| {
        assert!(!panel.path_editing, "CancelInput closes the editor");
    });

    // `ConfirmInput` goes through `confirm_path_edit`; an empty entry keeps the
    // editor open instead of navigating somewhere undefined.
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.begin_path_edit(window, cx);
            panel
                .path_field
                .update(cx, |field, cx| field.set_text("", window, cx));
            panel.confirm_active_input(window, cx);
        });
    })
    .unwrap();
    panel.read_with(cx, |panel, _cx| {
        assert!(panel.path_editing, "an empty path keeps the editor open");
    });
}

/// Breadcrumb targets must be the exact ancestor paths, and the sibling menu
/// must read the *parent* of the clicked crumb (the earlier bug read the
/// crumb's own children and joined them to the parent, so every jump landed on
/// a non-existent path).
#[gpui::test]
async fn test_breadcrumb_targets_are_exact_ancestors(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    let project = Project::test(fs.clone(), ["/".as_ref()], cx).await;

    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        FileManagerPanel::new(workspace, window, cx)
    });

    let targets = panel.update(cx, |panel, _cx| {
        panel.current_dir = Some(std::path::PathBuf::from("/home/alice/projects"));
        panel.breadcrumb_targets()
    });

    let paths: Vec<std::path::PathBuf> = targets.iter().map(|(path, _)| path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            std::path::PathBuf::from("/"),
            std::path::PathBuf::from("/home"),
            std::path::PathBuf::from("/home/alice"),
            std::path::PathBuf::from("/home/alice/projects"),
        ]
    );
    // Every segment is inside the previous one, so a click always navigates.
    for pair in paths.windows(2) {
        assert!(
            pair[1].starts_with(&pair[0]),
            "{} inside {}",
            pair[1].display(),
            pair[0].display()
        );
    }
}

/// Navigating through the breadcrumbs must reload the view immediately.
///
/// Regression test for "clicking the home segment only refreshes after Enter":
/// clicking a crumb calls `navigate_to`, which has to bump the loading
/// generation (i.e. start a new directory read) rather than returning early.
#[gpui::test]
async fn test_breadcrumb_navigation_reloads_the_view(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/home/alice",
        serde_json::json!({
            "projects": { "demo": {} },
            "notes.txt": "hi",
        }),
    )
    .await;
    let project = Project::test(fs.clone(), ["/home/alice".as_ref()], cx).await;

    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        FileManagerPanel::new(workspace, window, cx)
    });

    let window = cx.update(|_window, cx| {
        cx.open_window(WindowOptions::default(), |_window, _cx| panel.clone())
            .expect("failed to open the test window")
    });
    let window = window.into();

    // Start deep inside the tree, like `/home/alice/projects/demo`.
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.navigate_to(
                std::path::PathBuf::from("/home/alice/projects/demo"),
                window,
                cx,
            );
        });
    })
    .unwrap();
    cx.background_executor.run_until_parked();

    let before = panel.read_with(cx, |panel, _cx| panel.loading_generation);

    // Clicking the `alice` crumb navigates up one level.
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.navigate_to(std::path::PathBuf::from("/home/alice"), window, cx);
        });
    })
    .unwrap();

    panel.read_with(cx, |panel, _cx| {
        assert_eq!(
            panel.current_dir.as_deref(),
            Some(std::path::Path::new("/home/alice")),
            "the crumb click updates the current directory"
        );
        assert!(
            panel.loading_generation > before,
            "the crumb click starts a new directory load (was {}, now {})",
            before,
            panel.loading_generation
        );
    });
}

/// The sidebar tree stays rooted while the user browses.
///
/// Regression test for "clicking /bin makes the tree disappear": the tree used
/// to re-root at the current folder whenever it was outside the home directory,
/// so `/` → `/bin` rooted the tree at `/bin` (a symlink with no sub-directories
/// on NixOS), leaving an empty tree.
#[gpui::test]
async fn test_tree_root_does_not_follow_the_current_directory(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    let project = Project::test(fs.clone(), ["/home/alice".as_ref()], cx).await;

    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        FileManagerPanel::new(workspace, window, cx)
    });

    let window = cx.update(|_window, cx| {
        cx.open_window(WindowOptions::default(), |_window, _cx| panel.clone())
            .expect("failed to open the test window")
    });
    let window = window.into();

    // Outside the home directory the tree spans the whole filesystem.
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.navigate_to(std::path::PathBuf::from("/"), window, cx);
        });
    })
    .unwrap();
    cx.background_executor.run_until_parked();
    panel.read_with(cx, |panel, _cx| {
        assert_eq!(
            panel.tree_root.as_deref(),
            Some(std::path::Path::new("/")),
            "the tree roots at the filesystem root outside home"
        );
    });

    // Navigating into a directory of the root must not re-root the tree.
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.navigate_to(std::path::PathBuf::from("/bin"), window, cx);
        });
    })
    .unwrap();
    cx.background_executor.run_until_parked();
    panel.read_with(cx, |panel, _cx| {
        assert_eq!(
            panel.tree_root.as_deref(),
            Some(std::path::Path::new("/")),
            "browsing /bin keeps the tree rooted at /"
        );
    });

    // The root is sticky: going into the home directory (which is inside `/`)
    // must not rebuild the tree either — that rebuild is what made clicking
    // `qipn` in the tree look like it needed two clicks.
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.navigate_to(std::path::PathBuf::from("/home/alice"), window, cx);
        });
    })
    .unwrap();
    cx.background_executor.run_until_parked();
    panel.read_with(cx, |panel, _cx| {
        assert_eq!(
            panel.tree_root.as_deref(),
            Some(std::path::Path::new("/")),
            "entering home keeps the / root while it still contains us"
        );
    });
}

/// Typing keeps feeding the query even when the panel, not the entry, holds the
/// focus.
///
/// Regression test for "clicking the search button does not accept input":
/// opening the mode focuses the entry, but any click on the list moves the focus
/// back to the panel, and the panel used to drop every key while the search was
/// open.
#[gpui::test]
async fn test_typing_while_the_panel_holds_the_focus(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    let project = Project::test(fs.clone(), ["/".as_ref()], cx).await;

    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        FileManagerPanel::new(workspace, window, cx)
    });

    let window = cx.update(|_window, cx| {
        cx.open_window(WindowOptions::default(), |_window, _cx| panel.clone())
            .expect("failed to open the test window")
    });
    let window = window.into();

    // Open the search, then hand the focus back to the panel, like a click on
    // the file list does.
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.toggle_search(window, cx);
            window.focus(&panel.focus_handle, cx);
        });
    })
    .unwrap();

    TestAppContext::simulate_keystrokes(&mut *cx, window, "x y");

    panel.read_with(cx, |panel, cx| {
        assert_eq!(
            panel.search_field.read(cx).content(),
            "xy",
            "the panel forwards typed characters to the search entry"
        );
    });
}

/// Creating an entry opens the inline rename and the typed name is committed.
///
/// Regression test for "new file/folder naming does not work": the entry is
/// created before the row exists, so the editor has to keep working while the
/// panel holds the focus, and Enter must commit the typed name.
#[gpui::test]
async fn test_creating_a_folder_starts_and_commits_a_rename(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    fs.insert_tree("/home/alice", serde_json::json!({ "projects": {} }))
        .await;
    let project = Project::test(fs.clone(), ["/home/alice".as_ref()], cx).await;

    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        FileManagerPanel::new(workspace, window, cx)
    });

    let window = cx.update(|_window, cx| {
        cx.open_window(WindowOptions::default(), |_window, _cx| panel.clone())
            .expect("failed to open the test window")
    });
    let window = window.into();

    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| panel.create_folder(window, cx));
    })
    .unwrap();
    cx.background_executor.run_until_parked();

    // The suggested name is in the editor, with the whole name selected.
    panel.read_with(cx, |panel, cx| {
        assert_eq!(
            panel.rename_state.as_deref(),
            Some(std::path::Path::new("/home/alice/New Folder")),
            "creating a folder starts renaming it"
        );
        assert_eq!(panel.rename_field.read(cx).content(), "New Folder");
    });

    // Type over the selection and commit with Enter.
    TestAppContext::simulate_keystrokes(&mut *cx, window, "d e m o enter");
    cx.background_executor.run_until_parked();

    panel.read_with(cx, |panel, _cx| {
        assert!(panel.rename_state.is_none(), "Enter closes the editor");
    });
    assert!(
        fs.is_dir(std::path::Path::new("/home/alice/demo")).await,
        "the typed name was committed"
    );
}

/// Navigating to a deep path expands the tree down to it.
///
/// Regression test for "the tree does not follow the current path": only the
/// already loaded ancestors used to be expanded, so a path whose middle levels
/// had never been opened could not be revealed at all.
#[gpui::test]
async fn test_tree_reveals_the_current_directory(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/home/alice",
        serde_json::json!({
            "projects": { "demo": { "src": {} } },
        }),
    )
    .await;
    let project = Project::test(fs.clone(), ["/home/alice".as_ref()], cx).await;

    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        FileManagerPanel::new(workspace, window, cx)
    });

    let window = cx.update(|_window, cx| {
        cx.open_window(WindowOptions::default(), |_window, _cx| panel.clone())
            .expect("failed to open the test window")
    });
    let window = window.into();

    // Jump straight to a deep directory, like a breadcrumb/place jump does.
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.navigate_to(
                std::path::PathBuf::from("/home/alice/projects/demo/src"),
                window,
                cx,
            );
        });
    })
    .unwrap();
    cx.background_executor.run_until_parked();

    panel.read_with(cx, |panel, _cx| {
        // The tree is rooted at home, so `projects` and `demo` are the ancestors
        // that have to be loaded and expanded.
        let rows = crate::model::tree::visible_rows(&panel.tree_nodes);
        let expanded: Vec<&std::path::Path> = rows
            .iter()
            .filter(|node| node.expanded)
            .map(|node| node.path.as_path())
            .collect();

        assert!(
            expanded.contains(&std::path::Path::new("/home/alice/projects")),
            "projects is expanded, rows: {:?}",
            expanded
        );
        assert!(
            expanded.contains(&std::path::Path::new("/home/alice/projects/demo")),
            "demo is expanded, rows: {:?}",
            expanded
        );
        // The current directory itself stays collapsed (the chevron owns that).
        assert!(
            !expanded.contains(&std::path::Path::new("/home/alice/projects/demo/src")),
            "the current directory is not expanded"
        );
    });
}

/// Navigating folds the branches that are no longer on the current path.
///
/// The tree mirrors the current location: entering a folder expands its chain,
/// going back up folds it again instead of leaving every branch the user (or a
/// previous jump) opened behind.
#[gpui::test]
async fn test_tree_folds_branches_off_the_current_path(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/home/alice",
        serde_json::json!({
            "projects": { "demo": {} },
            "other": { "nested": {} },
        }),
    )
    .await;
    let project = Project::test(fs.clone(), ["/home/alice".as_ref()], cx).await;

    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        FileManagerPanel::new(workspace, window, cx)
    });

    let window = cx.update(|_window, cx| {
        cx.open_window(WindowOptions::default(), |_window, _cx| panel.clone())
            .expect("failed to open the test window")
    });
    let window = window.into();
    cx.background_executor.run_until_parked();

    // Open an unrelated branch by hand, the way the chevron does.
    cx.update_window(window, |_, _window, cx| {
        panel.update(cx, |panel, _cx| {
            if let Some(node) = crate::model::tree::TreeNode::find_mut(
                &mut panel.tree_nodes,
                std::path::Path::new("/home/alice/other"),
            ) {
                node.expanded = true;
            }
        });
    })
    .unwrap();

    // Now open a folder on the other branch.
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.navigate_to(std::path::PathBuf::from("/home/alice/projects"), window, cx);
        });
    })
    .unwrap();
    cx.background_executor.run_until_parked();

    panel.read_with(cx, |panel, _cx| {
        let rows = crate::model::tree::visible_rows(&panel.tree_nodes);
        let expanded: Vec<&std::path::Path> = rows
            .iter()
            .filter(|node| node.expanded)
            .map(|node| node.path.as_path())
            .collect();

        assert!(
            !expanded.contains(&std::path::Path::new("/home/alice/other")),
            "the branch we left is folded again, rows: {:?}",
            expanded
        );
        assert!(
            !expanded.contains(&std::path::Path::new("/home/alice/projects")),
            "the current directory is not expanded, rows: {:?}",
            expanded
        );
    });
}

/// The icon grid view and the details pane render as well.
#[gpui::test]
async fn test_render_grid_and_details_pane(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    let project = Project::test(fs.clone(), ["/".as_ref()], cx).await;

    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        FileManagerPanel::new(workspace, window, cx)
    });

    panel.update(cx, |panel, cx| {
        panel.set_view_mode(ViewMode::Grid, cx);
        panel.details_pane_visible = true;
        panel.sidebar_visible = true;
    });

    cx.update(|_window, cx| {
        cx.open_window(WindowOptions::default(), |_window, _cx| panel.clone())
            .expect("failed to open the test window");
    });
}
