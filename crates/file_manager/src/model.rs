//! The file manager's data layer: reading directories, searching, the folder
//! tree model, the clipboard state, conflict resolution and the undo history.
//!
//! Everything here is UI-independent apart from `gpui::SharedString`, which the
//! entry names use to avoid copying when they reach a `Label`. Nothing in this
//! module knows about the panel, the views or the theme, so the rules can be
//! reused (and unit-tested) without a window.

pub(crate) mod clipboard;
pub(crate) mod conflict;
pub(crate) mod entries;
pub(crate) mod search;
pub(crate) mod tree;
pub(crate) mod undo;
