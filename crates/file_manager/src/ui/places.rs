//! The "Places" list of the sidebar.
//!
//! OpenXplorer groups its sidebar into sections (Home, Quick access, This PC,
//! Network, Recent and Recycle Bin) separated by hairline rules
//! (`src/window/sidebar/entries.rs` in the upstream repository). The sections
//! are modelled here as a flat list of entries that carry their section, so
//! the renderer can draw the separators.

use std::path::PathBuf;

use gpui::SharedString;
use ui::IconName;

/// The sections of the places list, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaceSection {
    /// Home directory.
    Home,
    /// Desktop, Downloads, Documents, Pictures, Music, Videos.
    QuickAccess,
    /// The roots of the open project worktrees.
    ThisPc,
    /// Mounted volumes (the root, removable drives and network shares).
    Drives,
    /// Network shares mounted through CIFS/NFS/SSHFS.
    Network,
    /// The system recycle bin.
    RecycleBin,
}

impl PlaceSection {
    /// The heading shown above the section, if any.
    pub(crate) fn heading(self) -> Option<&'static str> {
        match self {
            PlaceSection::Home => None,
            PlaceSection::QuickAccess => Some("Quick access"),
            PlaceSection::ThisPc => Some("This PC"),
            PlaceSection::Drives => Some("Drives"),
            PlaceSection::Network => Some("Network"),
            PlaceSection::RecycleBin => None,
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
    /// Whether the entry is nested under a section heading.
    pub(crate) is_child: bool,
}

/// Builds the places list for the given home directory and project roots.
pub(crate) fn build_places(home: Option<&PathBuf>, roots: Vec<PathBuf>) -> Vec<PlaceEntry> {
    let mut entries = Vec::new();

    if let Some(home) = home {
        entries.push(PlaceEntry {
            label: "Home".into(),
            path: home.clone(),
            icon: IconName::FolderOpen,
            section: PlaceSection::Home,
            is_child: false,
        });

        // The quick-access folders, in OpenXplorer's order.
        for (name, icon) in [
            ("Desktop", IconName::Folder),
            ("Downloads", IconName::Download),
            ("Documents", IconName::FileDoc),
            ("Pictures", IconName::Image),
            ("Music", IconName::AudioOn),
            ("Videos", IconName::PlayOutlined),
        ] {
            let path = home.join(name);
            entries.push(PlaceEntry {
                label: name.into(),
                path,
                icon,
                section: PlaceSection::QuickAccess,
                is_child: true,
            });
        }
    }

    // The worktrees of the open project behave like OpenXplorer's "This PC".
    for root in roots {
        let label = root
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| root.display().to_string());
        entries.push(PlaceEntry {
            label: label.into(),
            path: root,
            icon: IconName::Folder,
            section: PlaceSection::ThisPc,
            is_child: true,
        });
    }

    // Local volumes and removable drives.
    let mounts = read_mounts();
    let mut seen: Vec<PathBuf> = Vec::new();
    for mount in &mounts {
        if mount.is_network || seen.contains(&mount.path) {
            continue;
        }
        // Only show the root, removable media and the usual mount points;
        // system mounts (/boot, /nix/store, …) would only add noise.
        if !is_user_visible_mount(&mount.path) {
            continue;
        }
        seen.push(mount.path.clone());
        entries.push(PlaceEntry {
            label: mount_label(&mount.path).into(),
            path: mount.path.clone(),
            icon: IconName::Folder,
            section: PlaceSection::Drives,
            is_child: true,
        });
    }

    // Network shares.
    for mount in mounts.iter().filter(|mount| mount.is_network) {
        entries.push(PlaceEntry {
            label: mount_label(&mount.path).into(),
            path: mount.path.clone(),
            icon: IconName::Folder,
            section: PlaceSection::Network,
            is_child: true,
        });
    }

    if let Some(trash) = recycle_bin_path(home) {
        entries.push(PlaceEntry {
            label: "Recycle Bin".into(),
            path: trash,
            icon: IconName::Trash,
            section: PlaceSection::RecycleBin,
            is_child: false,
        });
    }

    entries
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
fn is_user_visible_mount(path: &std::path::Path) -> bool {
    if path == std::path::Path::new("/") {
        return true;
    }
    // Removable media and administratively mounted volumes.
    const PREFIXES: [&str; 4] = ["/media/", "/run/media/", "/mnt/", "/srv/"];
    PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

/// The label of a mount point: the root, or the last path component.
fn mount_label(path: &std::path::Path) -> String {
    if path == std::path::Path::new("/") {
        return "File system".into();
    }
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// The user's home directory.
pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
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
        assert!(is_user_visible_mount(std::path::Path::new("/")));
        assert!(is_user_visible_mount(std::path::Path::new(
            "/media/alice/USB"
        )));
        assert!(is_user_visible_mount(std::path::Path::new(
            "/run/media/alice/USB"
        )));
        assert!(is_user_visible_mount(std::path::Path::new("/mnt/data")));
        assert!(!is_user_visible_mount(std::path::Path::new("/nix/store")));
        assert!(!is_user_visible_mount(std::path::Path::new("/boot")));
    }

    #[test]
    fn place_labels_use_the_last_component() {
        assert_eq!(mount_label(std::path::Path::new("/")), "File system");
        assert_eq!(mount_label(std::path::Path::new("/mnt/data")), "data");
    }

    #[test]
    fn places_start_with_home_and_end_with_the_recycle_bin() {
        let home = PathBuf::from("/home/alice");
        let places = build_places(Some(&home), vec![PathBuf::from("/work/project")]);

        let first = places.first().expect("a first place");
        assert_eq!(first.label.as_ref(), "Home");
        assert_eq!(first.path, home);

        let last = places.last().expect("a last place");
        assert_eq!(last.label.as_ref(), "Recycle Bin");

        assert!(
            places
                .iter()
                .any(|place| place.path == std::path::Path::new("/work/project"))
        );
        // The project root is listed under "This PC".
        assert!(
            places
                .iter()
                .any(|place| place.section == PlaceSection::ThisPc)
        );
    }
}
