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

/// The search entry accepts typing when the panel lives in a dock, not as the
/// root view of the window.
///
/// The other tests make the panel the window's root view, which never exercises
/// the dock's focus handling: the dock owns the key context and the panel can be
/// focused through it instead of through the root element. This mirrors the real
/// layout (`workspace.add_panel` + dock) so "clicking search does not let me
/// type" is covered.
#[gpui::test]
async fn test_typing_in_the_search_field_inside_a_dock(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    fs.insert_tree("/home/alice", serde_json::json!({ "projects": {} }))
        .await;
    let project = Project::test(fs.clone(), ["/home/alice".as_ref()], cx).await;

    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        let panel = FileManagerPanel::new(workspace, window, cx);
        workspace.add_panel(panel.clone(), window, cx);
        panel
    });
    let window = cx.update(|_window, cx| {
        cx.open_window(WindowOptions::default(), |_window, _cx| workspace.clone())
            .expect("failed to open the test window")
    });
    let window = window.into();

    // Open and focus the dock, the way clicking its tab does.
    cx.update_window(window, |_, window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.open_panel::<FileManagerPanel>(window, cx);
            workspace.focus_panel::<FileManagerPanel>(window, cx);
        });
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        let _ = window.draw(cx);
    })
    .unwrap();

    // The search button focuses the entry; the panel may also hold the focus
    // itself (its root element does that on every click).
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| panel.toggle_search(window, cx));
    })
    .unwrap();
    TestAppContext::simulate_keystrokes(&mut *cx, window, "a b");

    panel.read_with(cx, |panel, cx| {
        assert_eq!(
            panel.search_field.read(cx).content(),
            "ab",
            "typing reaches the search entry inside a dock"
        );
        assert_eq!(panel.search_query.as_deref(), Some("ab"));
    });
}

/// The location entry can be cancelled and confirmed by the panel's actions.
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

/// The sidebar keeps following the content area after the tree root loaded.
///
/// Regression test for "the tree follows only once": `refresh_tree` returned
/// early whenever the current folder was still inside the sticky root, so every
/// navigation after the initial load skipped `reveal_in_tree` — no lazy
/// expansion down to the folder, and no scrolling into view.
#[gpui::test]
async fn test_tree_follows_the_content_area_after_the_root_loads(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    let mut tree = serde_json::Map::new();
    // Enough siblings that the target row sits below the fold of the tree pane.
    for index in 0..30 {
        tree.insert(format!("dir{index:02}"), serde_json::json!({}));
    }
    tree.insert(
        "projects".into(),
        serde_json::json!({ "demo": { "src": {} } }),
    );
    tree.insert("other".into(), serde_json::json!({ "nested": {} }));
    fs.insert_tree("/home/alice", serde_json::Value::Object(tree))
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
    // Let the tree root finish loading: this is the moment after which the tree
    // used to stop following the content area.
    cx.background_executor.run_until_parked();
    panel.read_with(cx, |panel, _cx| {
        assert!(!panel.tree_nodes.is_empty(), "the tree root must be loaded");
    });

    // Open a nested folder from the content area.
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

    panel.read_with(cx, |panel, _cx| {
        let rows = crate::model::tree::visible_rows(&panel.tree_nodes);
        let paths: Vec<&std::path::Path> = rows.iter().map(|node| node.path.as_path()).collect();
        assert!(
            paths.contains(&std::path::Path::new("/home/alice/projects/demo")),
            "the current folder is revealed in the tree, rows: {paths:?}"
        );
        let projects = rows
            .iter()
            .find(|node| node.path == std::path::Path::new("/home/alice/projects"))
            .expect("the ancestor of the current folder is in the tree");
        assert!(
            projects.expanded,
            "the ancestors of the current folder are expanded, rows: {paths:?}"
        );
        assert_eq!(
            panel.last_revealed.as_deref(),
            Some(std::path::Path::new("/home/alice/projects/demo")),
            "the reveal ran for the folder that was opened"
        );
    });

    // The scroll is applied while the rows are painted.
    for _ in 0..2 {
        cx.update_window(window, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
        cx.background_executor.run_until_parked();
    }

    panel.read_with(cx, |panel, _cx| {
        assert!(
            f32::from(panel.tree_scroll.offset().y) < 0.,
            "the current row is scrolled into view, offset: {:?}",
            panel.tree_scroll.offset()
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

/// `Tab` in the path editor completes a unique entry and appends the separator
/// for a folder.
#[gpui::test]
async fn test_path_completion(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/home/alice",
        serde_json::json!({ "projects": {}, "pictures": {}, "README.md": "x" }),
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

    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.begin_path_edit(window, cx);
            panel.path_field.update(cx, |field, cx| {
                field.set_text("/home/alice/pro", window, cx);
            });
            panel.complete_path(window, cx);
        });
    })
    .unwrap();
    cx.background_executor.run_until_parked();

    panel.read_with(cx, |panel, cx| {
        assert_eq!(
            panel.path_field.read(cx).content(),
            "/home/alice/projects/",
            "a unique folder is completed with a trailing separator"
        );
    });

    // Two matches with a common prefix: only the prefix is added. `projects` and
    // `pictures` share nothing beyond the `p` that was typed, so the text stays.
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.path_field.update(cx, |field, cx| {
                field.set_text("/home/alice/p", window, cx);
            });
            panel.complete_path(window, cx);
        });
    })
    .unwrap();
    cx.background_executor.run_until_parked();

    panel.read_with(cx, |panel, cx| {
        assert_eq!(panel.path_field.read(cx).content(), "/home/alice/p");
    });
}

/// Batch rename applies one `#` pattern to the whole selection and keeps each
/// file's extension when the pattern has none.
#[gpui::test]
async fn test_batch_rename_with_a_pattern(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/home/alice",
        serde_json::json!({ "a.txt": "1", "b.txt": "2" }),
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

    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.selected = vec![
                std::path::PathBuf::from("/home/alice/a.txt"),
                std::path::PathBuf::from("/home/alice/b.txt"),
            ];
            panel.begin_batch_rename(window, cx);
            panel.batch_field.update(cx, |field, cx| {
                field.set_text("photo-#", window, cx);
            });
            panel.confirm_batch_rename(window, cx);
        });
    })
    .unwrap();
    cx.background_executor.run_until_parked();

    assert!(fs.path_exists(std::path::Path::new("/home/alice/photo-1.txt")));
    assert!(fs.path_exists(std::path::Path::new("/home/alice/photo-2.txt")));
    assert!(!fs.path_exists(std::path::Path::new("/home/alice/a.txt")));

    panel.read_with(cx, |panel, _cx| {
        assert!(panel.error.is_none(), "error: {:?}", panel.error);
    });
}

/// The search filters: scope, file kind and content matching.
#[gpui::test]
async fn test_search_filters(cx: &mut TestAppContext) {
    use crate::model::entries::SortMode;
    use crate::model::search::{SearchKind, SearchOptions, search_directory};

    let fake = fs::FakeFs::new(cx.background_executor.clone());
    fake.insert_tree(
        "/home/alice",
        serde_json::json!({
            "alpha.txt": "this file contains a needle",
            "photo.png": "png",
            "sub": { "beta.txt": "nothing here", "gamma.rs": "needle again" },
        }),
    )
    .await;
    let fs: std::sync::Arc<dyn fs::Fs> = fake.clone();

    let search = |query: &str, options: &SearchOptions| {
        let fs = fs.clone();
        let query = query.to_string();
        let options = options.clone();
        async move {
            search_directory(
                fs,
                std::path::PathBuf::from("/home/alice"),
                query,
                false,
                SortMode::default(),
                &options,
            )
            .await
            .expect("search")
            .entries
            .into_iter()
            .map(|entry| entry.name.to_string())
            .collect::<Vec<_>>()
        }
    };

    // Recursive by default: a hit in a subfolder is found.
    let recursive = search("beta", &SearchOptions::default()).await;
    assert_eq!(recursive, vec!["beta.txt".to_string()]);

    // Limiting the scope to the open folder hides it.
    let flat = search(
        "beta",
        &SearchOptions {
            recursive: false,
            ..SearchOptions::default()
        },
    )
    .await;
    assert!(flat.is_empty(), "got {flat:?}");

    // Only images. Folders are kept whatever the kind filter says: they are how
    // the reached files are navigated to.
    let images = search(
        "",
        &SearchOptions {
            kind: SearchKind::Images,
            ..SearchOptions::default()
        },
    )
    .await;
    assert_eq!(images, vec!["sub".to_string(), "photo.png".to_string()]);

    // Matching the file contents finds the two files whose text contains it.
    let contents = search(
        "needle",
        &SearchOptions {
            contents: true,
            ..SearchOptions::default()
        },
    )
    .await;
    assert_eq!(
        contents,
        vec!["alpha.txt".to_string(), "gamma.rs".to_string()]
    );
}

/// A name typed in the conflict prompt renames the incoming entry and leaves the
/// existing one alone; a typed name that also collides still gets a unique name.
#[gpui::test]
async fn test_conflict_typed_name(cx: &mut TestAppContext) {
    use crate::model::conflict::{ConflictPolicy, apply_transfer};

    let fake = fs::FakeFs::new(cx.background_executor.clone());
    fake.insert_tree(
        "/home/alice/from",
        serde_json::json!({ "incoming.txt": "new" }),
    )
    .await;
    fake.insert_tree(
        "/home/alice/to",
        serde_json::json!({ "notes.txt": "existing" }),
    )
    .await;
    let fs: std::sync::Arc<dyn fs::Fs> = fake.clone();

    // Type a free name: the existing entry must survive untouched.
    let chosen = std::path::PathBuf::from("/home/alice/to/renamed.txt");
    let written = apply_transfer(
        &fs,
        std::path::Path::new("/home/alice/from/incoming.txt"),
        std::path::Path::new("/home/alice/to/notes.txt"),
        true,
        ConflictPolicy::KeepBoth,
        Some(&chosen),
    )
    .await
    .expect("transfer")
    .expect("the item is written");
    assert_eq!(written.1, chosen);
    assert!(fs.path_exists(std::path::Path::new("/home/alice/to/notes.txt")));
    assert!(fs.path_exists(&chosen));

    // Type a name that already exists: it falls back to a unique name instead of
    // overwriting silently.
    let colliding = std::path::PathBuf::from("/home/alice/to/notes.txt");
    let written = apply_transfer(
        &fs,
        std::path::Path::new("/home/alice/from/incoming.txt"),
        std::path::Path::new("/home/alice/to/notes.txt"),
        true,
        ConflictPolicy::KeepBoth,
        Some(&colliding),
    )
    .await
    .expect("transfer")
    .expect("the item is written");
    assert_ne!(written.1, colliding);
    assert!(
        written
            .1
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("(1)")),
        "got {:?}",
        written.1
    );
}

/// The icon grid never shows the children of a folder expanded in place.
///
/// Nautilus' icon view has no expanders, so an expanded folder in the details
/// view used to spill a tree of tiles into the grid.
#[gpui::test]
async fn test_grid_hides_expanded_children(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/home/alice",
        serde_json::json!({ "projects": { "demo": {}, "other": {} } }),
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
    // Let the initial directory load finish, so `toggle_entry_expanded` has
    // something to expand.
    cx.background_executor.run_until_parked();
    panel.read_with(cx, |panel, _cx| {
        assert!(!panel.entries.is_empty(), "the folder must be loaded");
    });

    // Expand `projects` the way the details view's chevron does.
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| {
            panel.toggle_entry_expanded(std::path::Path::new("/home/alice/projects"), window, cx);
        });
    })
    .unwrap();
    cx.background_executor.run_until_parked();

    panel.read_with(cx, |panel, _cx| {
        let visible = panel.visible_rows();
        assert!(
            visible.iter().any(|(entry, depth)| *depth > 0),
            "the list view shows the children, rows: {:?}",
            visible
                .iter()
                .map(|(entry, depth)| (entry.name.to_string(), *depth))
                .collect::<Vec<_>>()
        );

        let grid = panel.grid_rows();
        assert!(
            grid.iter().all(|(_, depth)| *depth == 0),
            "the grid only shows the top level, rows: {:?}",
            grid.iter()
                .map(|(entry, depth)| (entry.name.to_string(), *depth))
                .collect::<Vec<_>>()
        );
        assert_eq!(grid.len(), panel.entries.len());
    });
}

/// The details view's columns can be resized and hidden, and both survive a
/// restart.
#[gpui::test]
async fn test_columns_resize_and_persist(cx: &mut TestAppContext) {
    use crate::ui::ox_theme::metrics;
    use crate::ui::panel::columns::Column;
    use gpui::{Bounds, point, px, size};

    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    let project = Project::test(fs.clone(), ["/".as_ref()], cx).await;
    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        FileManagerPanel::new(workspace, window, cx)
    });

    panel.update(cx, |panel, cx| {
        assert_eq!(panel.column_width(Column::Type), metrics::COLUMN_TYPE);

        // A 1000px header: `Name` gets `1000 - (176 + 135 + 90) - 14 = 585`,
        // so the `Type` column starts at 585 + 176 = 761.
        let header = Bounds::new(point(px(0.), px(0.)), size(px(1000.), px(38.)));
        panel.resize_column_at(Column::Type, 961., header, cx);
        assert_eq!(panel.column_width(Column::Type), 200.);

        // Dragging past the limits clamps instead of collapsing the column.
        panel.resize_column_at(Column::Size, 10., header, cx);
        assert_eq!(
            panel.column_width(Column::Size),
            crate::ui::panel::columns::MIN_COLUMN_WIDTH
        );

        // Column visibility is a toggle, and it is part of the persisted state.
        panel.toggle_column(Column::Modified, cx);
        assert!(!panel.columns_shown[Column::Modified.index()]);
        assert!(!panel.is_column_shown(Column::Modified));
    });

    panel.read_with(cx, |panel, _cx| {
        let state = panel.persisted_state();
        assert_eq!(state.column_widths[Column::Type.index()], 200.);
        assert_eq!(
            state.column_widths[Column::Size.index()],
            crate::ui::panel::columns::MIN_COLUMN_WIDTH
        );
        assert!(!state.columns_shown[Column::Modified.index()]);
    });
}

/// `Open as Workspace` acts on the selected folder, and on the folder being
/// browsed when the selection is empty or not a folder.
#[gpui::test]
async fn test_open_as_workspace_targets_the_folder(cx: &mut TestAppContext) {
    init_globals(cx);

    let fs = fs::FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/home/alice",
        serde_json::json!({
            "projects": {},
            "notes.txt": "hello",
        }),
    )
    .await;
    let project = Project::test(fs.clone(), ["/home/alice".as_ref()], cx).await;

    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));
    let panel = workspace.update_in(cx, |workspace, window, cx| {
        FileManagerPanel::new(workspace, window, cx)
    });
    cx.background_executor.run_until_parked();
    panel.read_with(cx, |panel, _cx| {
        assert!(
            !panel.entries.is_empty(),
            "the folder has to be loaded before a selection can be made"
        );
    });

    // Nothing selected: the folder being browsed.
    panel.read_with(cx, |panel, _cx| {
        assert_eq!(
            panel.workspace_target().as_deref(),
            Some(std::path::Path::new("/home/alice"))
        );
    });

    // A selected folder becomes the workspace root.
    panel.update(cx, |panel, cx| {
        panel.selected = vec![std::path::PathBuf::from("/home/alice/projects")];
        cx.notify();
    });
    panel.read_with(cx, |panel, _cx| {
        assert_eq!(
            panel.workspace_target().as_deref(),
            Some(std::path::Path::new("/home/alice/projects")),
            "the selected folder is the target"
        );
    });

    // A selected file does not: opening its parent as a workspace is not what
    // the menu entry says, so the command falls back to the current folder.
    panel.update(cx, |panel, cx| {
        panel.selected = vec![std::path::PathBuf::from("/home/alice/notes.txt")];
        cx.notify();
    });
    panel.read_with(cx, |panel, _cx| {
        assert_eq!(
            panel.workspace_target().as_deref(),
            Some(std::path::Path::new("/home/alice")),
            "a file selection keeps the current folder as the target"
        );
    });
}
