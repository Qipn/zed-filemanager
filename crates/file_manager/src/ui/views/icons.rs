//! Entry icons, resolved through Zed's `file_icons` crate.
//!
//! Files and directories are looked up separately, because the two lookups are
//! not interchangeable:
//!
//! - `FileIcons::get_icon` matches a path by name and by suffixes (`main.rs` →
//!   `rust.svg`, `main.c` → `c.svg`). Run on a *directory* it answers with the
//!   generic file glyph, since directory names have no suffix.
//! - `FileIcons::get_folder_icon` matches a directory by name (`Downloads`,
//!   `.git`, `node_modules`) and falls back to the theme's folder glyph, with
//!   the `open` variant while the folder is expanded.
//!
//! Both go through the active icon theme, so the panel inherits every icon
//! theme Zed ships instead of drawing two generic glyphs. When the theme has no
//! icon at all we fall back to `IconName` glyphs with the panel's own
//! accent/muted colours.

use file_icons::FileIcons;
use gpui::{AnyElement, App, SharedString};
use ui::prelude::*;
use ui::{Color, Icon, IconName, IconSize};

use crate::model::entries::DirEntry;

/// The theme glyph of `entry`, if the active icon theme has one.
///
/// `expanded` only matters for directories; the folder tree and the expandable
/// rows of the details view pass their current state so an opened folder gets
/// the `folder_open` glyph.
fn theme_icon(entry: &DirEntry, expanded: bool, cx: &App) -> Option<SharedString> {
    if entry.is_dir {
        FileIcons::get_folder_icon(expanded, &entry.path, cx)
    } else {
        FileIcons::get_icon(&entry.path, cx)
    }
}

/// The icon of `entry` at the given size.
pub(crate) fn entry_icon(entry: &DirEntry, expanded: bool, size: IconSize, cx: &App) -> AnyElement {
    match theme_icon(entry, expanded, cx) {
        // Theme icons carry their own colour (and the folder variant differs
        // between directories), so they are drawn without a tint.
        Some(glyph) => Icon::from_path(glyph).size(size).into_any_element(),
        None => Icon::new(if entry.is_dir {
            IconName::Folder
        } else {
            IconName::File
        })
        .size(size)
        .color(if entry.is_dir {
            Color::Accent
        } else {
            Color::Muted
        })
        .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui::TestAppContext;
    use settings::SettingsStore;

    use super::*;

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            if !cx.has_global::<SettingsStore>() {
                let store = SettingsStore::test(cx);
                cx.set_global(store);
            }
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        });
    }

    fn entry(path: &str, is_dir: bool) -> DirEntry {
        let name = path.rsplit('/').next().unwrap_or(path);
        DirEntry {
            path: PathBuf::from(path),
            name: name.into(),
            is_dir,
            len: None,
            mtime: None,
        }
    }

    /// Files of different types get their own glyph, and a directory gets a
    /// folder glyph instead of the generic file one (a directory has no suffix,
    /// so a file lookup would answer `file.svg`).
    #[gpui::test]
    async fn test_entries_get_their_type_icon(cx: &mut TestAppContext) {
        init(cx);

        cx.update(|cx| {
            assert_eq!(
                theme_icon(&entry("/work/src/main.rs", false), false, cx).as_deref(),
                Some("icons/file_icons/rust.svg"),
                "a .rs file gets the Rust glyph"
            );
            assert_eq!(
                theme_icon(&entry("/work/src/main.c", false), false, cx).as_deref(),
                Some("icons/file_icons/c.svg"),
                "a .c file gets the C glyph"
            );
            assert_eq!(
                theme_icon(&entry("/work/src/main.h", false), false, cx).as_deref(),
                Some("icons/file_icons/c.svg"),
                "headers share the C glyph"
            );
            assert_eq!(
                theme_icon(&entry("/work/notes.unknownext", false), false, cx).as_deref(),
                Some("icons/file_icons/file.svg"),
                "an unknown extension gets the generic file glyph"
            );

            assert_eq!(
                theme_icon(&entry("/work/src", true), false, cx).as_deref(),
                Some("icons/file_icons/folder.svg"),
                "a directory gets the folder glyph"
            );
            assert_eq!(
                theme_icon(&entry("/work/src", true), true, cx).as_deref(),
                Some("icons/file_icons/folder_open.svg"),
                "an expanded directory gets the open-folder glyph"
            );
            assert_eq!(
                theme_icon(&entry("/work/v1.2", true), false, cx).as_deref(),
                Some("icons/file_icons/folder.svg"),
                "a dotted directory name still gets a folder glyph, not a file one"
            );
        });
    }
}
