//! The "Places" list of the sidebar.
//!
//! The structure follows GNOME Files' places sidebar
//! (`nautilus-sidebar.c::update_places`), which is the widget GTK's file
//! chooser uses too:
//!
//! 1. the default locations — Home, the quick-access folders, the Trash;
//! 2. the mounts — volumes, drives and network shares;
//! 3. the user's bookmarks.
//!
//! Sections are separated by a hairline rule, not by a heading: Nautilus hangs
//! a `GtkSeparator` off the first row of every new section
//! (`add_place` → `gtk_list_box_row_set_header`). Rows are therefore flat, with
//! no indentation.
//!
//! Two extensions are Zed-specific: the worktrees of the open project ("This
//! PC") and the network shares read straight from `/proc/mounts`.

use std::path::{Path, PathBuf};

use gpui::SharedString;
use ui::IconName;

use crate::model::bookmarks::Bookmark;
use crate::model::xdg::{self, UserDir};

/// The sections of the places list, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaceSection {
    /// The home directory.
    Home,
    /// Desktop, Downloads, Documents, Pictures, Music, Videos.
    QuickAccess,
    /// The system recycle bin (Nautilus keeps it with the default locations).
    RecycleBin,
    /// The roots of the open project worktrees (Zed-specific).
    ThisPc,
    /// Mounted volumes (the root, removable drives).
    Drives,
    /// Network shares mounted through CIFS/NFS/SSHFS.
    Network,
    /// Folders the user pinned through the bookmark file.
    Bookmarks,
}

impl PlaceSection {
    /// The visual group of the section. A hairline rule is drawn between two
    /// neighbouring entries of different groups.
    pub(crate) fn group(self) -> u8 {
        match self {
            PlaceSection::Home | PlaceSection::QuickAccess | PlaceSection::RecycleBin => 0,
            PlaceSection::ThisPc => 1,
            // Nautilus appends the network volumes to the same "mounts" block.
            PlaceSection::Drives | PlaceSection::Network => 2,
            PlaceSection::Bookmarks => 3,
        }
    }
}

/// One row of the places list.
#[derive(Clone, Debug)]
pub(crate) struct PlaceEntry {
    /// The label shown in the row.
    pub(crate) label: SharedString,
    /// The directory the entry points at.
    pub(crate) path: PathBuf,
    /// The icon of the row.
    pub(crate) icon: IconName,
    /// The section the entry belongs to.
    pub(crate) section: PlaceSection,
    /// Whether the entry is a bookmark, so it can be unpinned from the row.
    pub(crate) is_bookmark: bool,
}

/// Builds the places list for the given home directory, project roots and
/// bookmarks.
pub(crate) fn build_places(
    home: Option<&PathBuf>,
    roots: Vec<PathBuf>,
    bookmarks: &[Bookmark],
) -> Vec<PlaceEntry> {
    let mut entries = Vec::new();

    // ---- default locations: home, quick access, trash ----
    if let Some(home) = home {
        entries.push(PlaceEntry {
            label: "Home".into(),
            path: home.clone(),
            icon: IconName::FolderOpen,
            section: PlaceSection::Home,
            is_bookmark: false,
        });

        // Read from `user-dirs.dirs` so localized names and moved folders are
        // picked up. Nautilus only surfaces Desktop in its sidebar; listing the
        // whole set is the OpenXplorer behaviour we keep.
        for (kind, path) in xdg::user_dirs_in(home) {
            let label = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| kind.default_name().to_string());
            entries.push(PlaceEntry {
                label: label.into(),
                path,
                icon: user_dir_icon(kind),
                section: PlaceSection::QuickAccess,
                is_bookmark: false,
            });
        }
    }

    if let Some(trash) = recycle_bin_path(home) {
        entries.push(PlaceEntry {
            label: "Recycle Bin".into(),
            path: trash,
            icon: IconName::Trash,
            section: PlaceSection::RecycleBin,
            is_bookmark: false,
        });
    }

    // ---- the worktrees of the open project (Zed-specific) ----
    for root in roots {
        if entries.iter().any(|entry| entry.path == root) {
            continue;
        }
        let label = root
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| root.display().to_string());
        entries.push(PlaceEntry {
            label: label.into(),
            path: root,
            icon: IconName::Folder,
            section: PlaceSection::ThisPc,
            is_bookmark: false,
        });
    }

    // ---- mounts: volumes, then network shares ----
    let mounts = read_mounts();
    for mount in &mounts {
        if mount.is_network {
            continue;
        }
        // Only show the root, removable media and the usual mount points;
        // system mounts (/boot, /nix/store, …) would only add noise.
        if !is_user_visible_mount(&mount.path) {
            continue;
        }
        if entries.iter().any(|entry| entry.path == mount.path) {
            continue;
        }
        entries.push(PlaceEntry {
            label: mount_label(&mount.path).into(),
            path: mount.path.clone(),
            icon: IconName::Server,
            section: PlaceSection::Drives,
            is_bookmark: false,
        });
    }

    for mount in mounts.iter().filter(|mount| mount.is_network) {
        if entries.iter().any(|entry| entry.path == mount.path) {
            continue;
        }
        entries.push(PlaceEntry {
            label: mount_label(&mount.path).into(),
            path: mount.path.clone(),
            icon: IconName::Public,
            section: PlaceSection::Network,
            is_bookmark: false,
        });
    }

    // ---- bookmarks ----
    for bookmark in bookmarks {
        if entries
            .iter()
            .any(|entry| entry.path == bookmark.path && !entry.is_bookmark)
        {
            // Already reachable through Home, quick access or a worktree.
            continue;
        }
        entries.push(PlaceEntry {
            label: bookmark.label.clone().into(),
            path: bookmark.path.clone(),
            icon: IconName::Bookmark,
            section: PlaceSection::Bookmarks,
            is_bookmark: true,
        });
    }

    entries
}

/// The icon of a quick-access folder.
/// Whether `path` is the Trash folder shown in the sidebar.
///
/// The Trash is a place like any other, but nothing in it should be turned into
/// a workspace root, so the sidebar menus need to tell it apart.
pub(crate) fn is_trash_path(path: &Path) -> bool {
    path.ends_with(".local/share/Trash/files")
}

fn user_dir_icon(kind: UserDir) -> IconName {
    match kind {
        UserDir::Desktop => IconName::Folder,
        UserDir::Downloads => IconName::Download,
        UserDir::Documents => IconName::FileDoc,
        UserDir::Pictures => IconName::Image,
        UserDir::Music => IconName::AudioOn,
        UserDir::Videos => IconName::PlayOutlined,
    }
}

/// The system recycle bin, following the XDG base directory layout.
fn recycle_bin_path(home: Option<&PathBuf>) -> Option<PathBuf> {
    let home = home?;
    Some(home.join(".local/share/Trash/files"))
}

/// One entry of `/proc/mounts`.
struct Mount {
    path: PathBuf,
    /// Whether the mount comes from the network (CIFS, NFS, SSHFS, …).
    is_network: bool,
}

/// Reads the mount table of the running system.
fn read_mounts() -> Vec<Mount> {
    let Ok(table) = std::fs::read_to_string("/proc/mounts") else {
        return Vec::new();
    };

    table
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let _device = fields.next()?;
            let path = fields.next()?;
            let fs_type = fields.next()?;
            // `\040` is how /proc/mounts escapes a space in a path.
            let path = PathBuf::from(path.replace("\\040", " "));
            Some(Mount {
                path,
                is_network: is_network_fs(fs_type),
            })
        })
        .collect()
}

/// The file-system types that represent a network share.
fn is_network_fs(fs_type: &str) -> bool {
    matches!(
        fs_type,
        "cifs" | "smbfs" | "nfs" | "nfs4" | "sshfs" | "fuse.sshfs" | "fuse.rclone" | "9p"
    )
}

/// Whether a local mount point is worth showing in the sidebar.
fn is_user_visible_mount(path: &Path) -> bool {
    if path == Path::new("/") {
        return true;
    }
    // Removable media and administratively mounted volumes.
    const PREFIXES: [&str; 4] = ["/media/", "/run/media/", "/mnt/", "/srv/"];
    PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

/// The label of a mount point: the root, or the last path component.
fn mount_label(path: &Path) -> String {
    if path == Path::new("/") {
        return "File system".into();
    }
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_file_systems_are_recognised() {
        for fs_type in ["cifs", "nfs", "nfs4", "sshfs", "fuse.sshfs", "9p"] {
            assert!(is_network_fs(fs_type), "{fs_type} is a network file system");
        }
        for fs_type in ["ext4", "btrfs", "tmpfs", "overlay"] {
            assert!(!is_network_fs(fs_type), "{fs_type} is local");
        }
    }

    #[test]
    fn only_user_facing_mounts_are_listed() {
        assert!(is_user_visible_mount(Path::new("/")));
        assert!(is_user_visible_mount(Path::new("/media/alice/USB")));
        assert!(is_user_visible_mount(Path::new("/run/media/alice/USB")));
        assert!(is_user_visible_mount(Path::new("/mnt/data")));
        assert!(!is_user_visible_mount(Path::new("/nix/store")));
        assert!(!is_user_visible_mount(Path::new("/boot")));
    }

    #[test]
    fn place_labels_use_the_last_component() {
        assert_eq!(mount_label(Path::new("/")), "File system");
        assert_eq!(mount_label(Path::new("/mnt/data")), "data");
    }

    #[test]
    fn the_list_starts_with_home_and_puts_the_trash_before_the_mounts() {
        let home = PathBuf::from("/home/alice");
        let places = build_places(Some(&home), vec![PathBuf::from("/work/project")], &[]);

        let first = places.first().expect("a first place");
        assert_eq!(first.label.as_ref(), "Home");
        assert_eq!(first.path, home);

        // Like Nautilus, the Trash belongs to the default-locations block, so
        // it comes before the mounts.
        let index_of = |section: PlaceSection| places.iter().position(|p| p.section == section);
        let trash = index_of(PlaceSection::RecycleBin).expect("the recycle bin");
        if let Some(drives) = index_of(PlaceSection::Drives) {
            assert!(trash < drives, "the recycle bin precedes the mounts");
        }
        if let Some(network) = index_of(PlaceSection::Network) {
            assert!(trash < network, "the recycle bin precedes the network");
        }

        // The project root is listed under "This PC".
        assert!(
            places
                .iter()
                .any(|place| place.path == Path::new("/work/project")
                    && place.section == PlaceSection::ThisPc)
        );
    }

    #[test]
    fn bookmarks_are_listed_last_and_deduplicated() {
        let home = PathBuf::from("/home/alice");
        let bookmarks = vec![
            Bookmark::new(PathBuf::from("/srv/projects"), Some("Projects")),
            // Pinning the home directory would duplicate the Home row.
            Bookmark::new(home.clone(), Some("Home again")),
        ];

        let places = build_places(Some(&home), Vec::new(), &bookmarks);
        let pinned: Vec<&PlaceEntry> = places
            .iter()
            .filter(|place| place.section == PlaceSection::Bookmarks)
            .collect();

        assert_eq!(pinned.len(), 1, "the duplicate home bookmark is skipped");
        assert_eq!(pinned[0].label.as_ref(), "Projects");
        assert!(pinned[0].is_bookmark);
        assert_eq!(
            places.last().map(|place| place.section),
            Some(PlaceSection::Bookmarks),
            "bookmarks are the last block, like Nautilus"
        );
    }

    #[test]
    fn a_worktree_root_is_not_repeated() {
        let home = PathBuf::from("/home/alice");
        let places = build_places(Some(&home), vec![home.clone()], &[]);
        let homes = places.iter().filter(|place| place.path == home).count();

        assert_eq!(homes, 1);
    }

    #[test]
    fn sections_are_grouped_like_nautilus() {
        // Default locations share one group, mounts another, bookmarks a third.
        assert_eq!(
            PlaceSection::Home.group(),
            PlaceSection::QuickAccess.group()
        );
        assert_eq!(
            PlaceSection::QuickAccess.group(),
            PlaceSection::RecycleBin.group()
        );
        assert_ne!(
            PlaceSection::RecycleBin.group(),
            PlaceSection::ThisPc.group()
        );
        assert_ne!(PlaceSection::ThisPc.group(), PlaceSection::Drives.group());
        assert_eq!(
            PlaceSection::Drives.group(),
            PlaceSection::Network.group(),
            "Nautilus appends network shares to the mounts block"
        );
        assert_ne!(
            PlaceSection::Network.group(),
            PlaceSection::Bookmarks.group()
        );
    }
}
