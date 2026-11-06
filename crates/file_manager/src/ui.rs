//! The file manager's view layer: the dock panel, its renderer, the folder
//! views, the sidebar, the details pane and the OpenXplorer theme tokens.
//!
//! This layer owns the `gpui` entities and only reads from [`crate::model`];
//! the model never calls back into it.

pub(crate) mod details_pane;
pub(crate) mod drag;
pub(crate) mod ox_theme;
pub(crate) mod panel;
pub(crate) mod places;
pub(crate) mod render;
pub(crate) mod sidebar;
pub(crate) mod views;

pub use panel::FileManagerPanel;
