//! Render smoke tests for the file manager panel.
//!
//! They build a workspace with a temporary directory and draw the panel, so
//! that the layout (header bar, sidebar with the places list and the folder
//! tree, details view, details pane, status bar) is exercised without opening
//! the editor.

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
