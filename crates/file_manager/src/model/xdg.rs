//! The XDG base directories the file manager needs.
//!
//! GNOME Files (Nautilus) asks GLib for these (`g_get_user_special_dir`), which
//! reads `$XDG_CONFIG_HOME/user-dirs.dirs`
//! (`nautilus-sidebar.c::get_desktop_directory_uri` and the XDG user-dirs
//! specification). Reading the same file keeps the sidebar's quick access list
//! in sync with the rest of the desktop: localized folder names (`桌面`,
//! `下载`, …) and folders the user moved elsewhere are honoured, and pointing a
//! folder at the home directory disables it.

use std::path::{Path, PathBuf};

/// The user's home directory.
pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

/// `$XDG_CONFIG_HOME`, or `~/.config` when it is unset.
pub(crate) fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|config| !config.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".config")))
}

/// One of the user's special folders, i.e. an `XDG_*_DIR` entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UserDir {
    Desktop,
    Downloads,
    Documents,
    Pictures,
    Music,
    Videos,
}

impl UserDir {
    /// The folders of the quick access section, in sidebar order.
    pub(crate) const ALL: [Self; 6] = [
        Self::Desktop,
        Self::Downloads,
        Self::Documents,
        Self::Pictures,
        Self::Music,
        Self::Videos,
    ];

    /// The `user-dirs.dirs` key of the folder.
    pub(crate) fn config_key(self) -> &'static str {
        match self {
            Self::Desktop => "XDG_DESKTOP_DIR",
            Self::Downloads => "XDG_DOWNLOAD_DIR",
            Self::Documents => "XDG_DOCUMENTS_DIR",
            Self::Pictures => "XDG_PICTURES_DIR",
            Self::Music => "XDG_MUSIC_DIR",
            Self::Videos => "XDG_VIDEOS_DIR",
        }
    }

    /// The English folder name, used when `user-dirs.dirs` does not exist.
    pub(crate) fn default_name(self) -> &'static str {
        match self {
            Self::Desktop => "Desktop",
            Self::Downloads => "Downloads",
            Self::Documents => "Documents",
            Self::Pictures => "Pictures",
            Self::Music => "Music",
            Self::Videos => "Videos",
        }
    }
}

/// The existing special folders below `home`, in sidebar order.
///
/// Like Nautilus, folders that do not exist are left out, so a fresh account
/// without a `~/Videos` does not get a row that opens an error.
pub(crate) fn user_dirs_in(home: &Path) -> Vec<(UserDir, PathBuf)> {
    let configured = config_dir()
        .and_then(|config| std::fs::read_to_string(config.join("user-dirs.dirs")).ok())
        .map(|contents| parse_user_dirs(&contents, home))
        .unwrap_or_default();

    resolve_user_dirs(&configured, home, Path::is_dir)
}

/// Maps the parsed `user-dirs.dirs` entries to the folders that exist.
///
/// `exists` is a parameter so the rules can be tested without a real home
/// directory.
fn resolve_user_dirs(
    configured: &[(String, PathBuf)],
    home: &Path,
    exists: impl Fn(&Path) -> bool,
) -> Vec<(UserDir, PathBuf)> {
    UserDir::ALL
        .into_iter()
        .filter_map(|kind| {
            let path = configured
                .iter()
                .find(|(key, _)| key == kind.config_key())
                .map(|(_, path)| path.clone())
                .unwrap_or_else(|| home.join(kind.default_name()));
            // Pointing a folder at the home directory is how XDG disables it.
            if path == home || !exists(&path) {
                return None;
            }
            Some((kind, path))
        })
        .collect()
}

/// Parses the body of `user-dirs.dirs`: one `KEY="$HOME/somewhere"` per line.
fn parse_user_dirs(contents: &str, home: &Path) -> Vec<(String, PathBuf)> {
    let mut dirs = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        dirs.push((
            key.trim().to_string(),
            expand_home(value.trim().trim_matches('"'), home),
        ));
    }
    dirs
}

/// Expands the `$HOME` prefix of a `user-dirs.dirs` value.
fn expand_home(value: &str, home: &Path) -> PathBuf {
    if value == "$HOME" {
        return home.to_path_buf();
    }
    if let Some(rest) = value.strip_prefix("$HOME/") {
        return home.join(rest);
    }
    PathBuf::from(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_dirs_are_parsed_and_expanded() {
        let home = Path::new("/home/alice");
        let contents = "\
# Written by xdg-user-dirs-update
XDG_DESKTOP_DIR=\"$HOME/桌面\"
XDG_DOWNLOAD_DIR=\"$HOME/下载\"

XDG_DOCUMENTS_DIR=\"/data/documents\"
XDG_MUSIC_DIR=\"$HOME\"
";
        let dirs = parse_user_dirs(contents, home);

        assert_eq!(
            dirs,
            vec![
                ("XDG_DESKTOP_DIR".to_string(), home.join("桌面")),
                ("XDG_DOWNLOAD_DIR".to_string(), home.join("下载")),
                (
                    "XDG_DOCUMENTS_DIR".to_string(),
                    PathBuf::from("/data/documents")
                ),
                ("XDG_MUSIC_DIR".to_string(), home.to_path_buf()),
            ]
        );
    }

    #[test]
    fn every_special_folder_has_a_key_and_a_name() {
        for kind in UserDir::ALL {
            assert!(kind.config_key().starts_with("XDG_"));
            assert!(!kind.default_name().is_empty());
        }
    }

    #[test]
    fn existing_folders_are_listed_and_missing_ones_are_dropped() {
        let home = Path::new("/home/alice");
        let configured = vec![
            ("XDG_DESKTOP_DIR".to_string(), home.join("桌面")),
            ("XDG_MUSIC_DIR".to_string(), home.to_path_buf()),
        ];

        // Only the desktop folder and the conventional `Documents` exist.
        let dirs = resolve_user_dirs(&configured, home, |path| {
            path == Path::new("/home/alice/桌面") || path == Path::new("/home/alice/Documents")
        });

        let paths: Vec<&Path> = dirs.iter().map(|(_, path)| path.as_path()).collect();
        assert_eq!(
            dirs.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(),
            vec![UserDir::Desktop, UserDir::Documents],
            "localized and fallback names are both kept, disabled and missing ones dropped"
        );
        assert_eq!(
            paths,
            vec![
                Path::new("/home/alice/桌面"),
                Path::new("/home/alice/Documents")
            ]
        );
    }
}
