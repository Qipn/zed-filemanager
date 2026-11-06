//! The user's bookmarks: the folders pinned to the sidebar's bookmark section.
//!
//! GNOME Files (Nautilus) stores them in the freedesktop bookmark file that
//! GTK's file chooser shares: one `uri label` pair per line in
//! `$XDG_CONFIG_HOME/gtk-3.0/bookmarks`
//! (`nautilus-bookmark-list.c::load_callback` / `save_io_thread`). Using the
//! same file and format means a folder pinned here also shows up in Nautilus
//! and in every GTK file chooser, and vice versa.

use std::io;
use std::path::{Path, PathBuf};

use super::xdg::config_dir;

/// A folder pinned to the sidebar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Bookmark {
    /// The folder the bookmark points at.
    pub(crate) path: PathBuf,
    /// The label shown in the sidebar.
    pub(crate) label: String,
}

impl Bookmark {
    /// Creates a bookmark, falling back to the folder name when `label` is
    /// missing or blank (Nautilus does the same with a `NULL` label).
    pub(crate) fn new(path: PathBuf, label: Option<&str>) -> Self {
        let label = label
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| default_label(&path));
        Self { path, label }
    }
}

/// The label of a bookmark without an explicit one: the folder name, or
/// "Home" for the home directory (as `nautilus_file_get_display_name` does).
fn default_label(path: &Path) -> String {
    if super::xdg::home_dir().as_deref() == Some(path) {
        return "Home".to_string();
    }
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

/// The graph that holds the bookmarks.
pub(crate) fn bookmarks_file() -> Option<PathBuf> {
    Some(config_dir()?.join("gtk-3.0").join("bookmarks"))
}

/// Reads the bookmarks of the user, skipping duplicates and invalid lines.
pub(crate) fn load() -> Vec<Bookmark> {
    let Some(file) = bookmarks_file() else {
        return Vec::new();
    };
    let Ok(contents) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    parse(&contents)
}

/// Parses the contents of a bookmarks file.
fn parse(contents: &str) -> Vec<Bookmark> {
    let mut bookmarks: Vec<Bookmark> = Vec::new();
    for line in contents.lines() {
        let Some(bookmark) = parse_line(line) else {
            continue;
        };
        if bookmarks.iter().any(|known| known.path == bookmark.path) {
            continue;
        }
        bookmarks.push(bookmark);
    }
    bookmarks
}

/// Parses one `uri label` line.
fn parse_line(line: &str) -> Option<Bookmark> {
    // Nautilus skips empty lines and lines that start with whitespace.
    if line.is_empty() || line.starts_with(char::is_whitespace) {
        return None;
    }
    // The URI and the label are separated by the first space; the label may
    // contain spaces.
    let (uri, label) = match line.split_once(' ') {
        Some((uri, label)) => (uri, Some(label)),
        None => (line, None),
    };
    Some(Bookmark::new(path_from_uri(uri)?, label))
}

/// Adds a folder to the bookmarks; existing pins keep their label.
pub(crate) fn add(path: &Path, label: Option<&str>) -> io::Result<Vec<Bookmark>> {
    let mut bookmarks = load();
    if bookmarks.iter().any(|bookmark| bookmark.path == path) {
        return Ok(bookmarks);
    }
    bookmarks.push(Bookmark::new(path.to_path_buf(), label));
    save(&bookmarks)?;
    Ok(bookmarks)
}

/// Removes a folder from the bookmarks.
pub(crate) fn remove(path: &Path) -> io::Result<Vec<Bookmark>> {
    let mut bookmarks = load();
    let count = bookmarks.len();
    bookmarks.retain(|bookmark| bookmark.path != path);
    if bookmarks.len() != count {
        save(&bookmarks)?;
    }
    Ok(bookmarks)
}

/// Writes all bookmarks, creating the configuration directories if needed.
fn save(bookmarks: &[Bookmark]) -> io::Result<()> {
    let Some(file) = bookmarks_file() else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no configuration directory",
        ));
    };
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut contents = String::new();
    for bookmark in bookmarks {
        contents.push_str(&to_line(bookmark));
        contents.push('\n');
    }
    std::fs::write(file, contents)
}

/// Serialises one bookmark as `uri label`.
fn to_line(bookmark: &Bookmark) -> String {
    let uri = file_uri(&bookmark.path);
    if bookmark.label.is_empty() {
        uri
    } else {
        format!("{uri} {}", bookmark.label)
    }
}

/// The `file://` URI of an absolute path.
pub(crate) fn file_uri(path: &Path) -> String {
    format!("file://{}", percent_encode(&path.to_string_lossy()))
}

/// The local path of a `file://` URI, or `None` for other schemes and remote
/// hosts, which a local file manager cannot open.
pub(crate) fn path_from_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = match rest.strip_prefix('/') {
        Some(path) => format!("/{path}"),
        None => {
            // `file://host/path`: only the local host is usable here.
            let (host, path) = rest.split_once('/')?;
            if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
                return None;
            }
            format!("/{path}")
        }
    };
    Some(PathBuf::from(percent_decode(&path)))
}

/// Whether `byte` may stay unescaped in a path, matching `g_filename_to_uri`.
fn is_uri_safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'/'
                | b':'
                | b'@'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
        )
}

/// Percent-encodes a path the way `g_filename_to_uri` does.
fn percent_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        if is_uri_safe(*byte) {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// Decodes a percent-encoded path, tolerating malformed escapes.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                decoded.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_split_at_the_first_space() {
        let bookmarks = parse("file:///home/alice/My%20Docs My Documents\n");

        assert_eq!(
            bookmarks,
            vec![Bookmark {
                path: PathBuf::from("/home/alice/My Docs"),
                label: "My Documents".to_string(),
            }]
        );
    }

    #[test]
    fn blank_line_only_and_foreign_schemes_are_skipped() {
        let contents = concat!(
            "\n",
            " leading space\n",
            "not a uri\n",
            "smb://server/share Share\n",
            "file:///srv/data\n",
        );
        let bookmarks = parse(contents);

        assert_eq!(
            bookmarks,
            vec![Bookmark {
                path: PathBuf::from("/srv/data"),
                label: "data".to_string(),
            }]
        );
    }

    #[test]
    fn duplicate_paths_keep_the_first_label() {
        let contents = "file:///a First\nfile:///a Second\n";
        let bookmarks = parse(contents);

        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].label, "First");
    }

    #[test]
    fn uris_round_trip_through_the_path() {
        for path in [
            "/home/alice/桌面/文件",
            "/home/alice/My Documents/a+b&c.txt",
            "/",
        ] {
            let path = PathBuf::from(path);
            let uri = file_uri(&path);
            assert!(uri.starts_with("file:///"), "{uri}");
            assert_eq!(path_from_uri(&uri).as_deref(), Some(path.as_path()));
        }
    }

    #[test]
    fn localhost_uris_are_local_but_remote_hosts_are_not() {
        assert_eq!(
            path_from_uri("file://localhost/srv/data"),
            Some(PathBuf::from("/srv/data"))
        );
        assert_eq!(path_from_uri("file://fileserver/srv/data"), None);
        assert_eq!(path_from_uri("https://example.com/x"), None);
    }

    #[test]
    fn a_blank_label_falls_back_to_the_folder_name() {
        let bookmark = Bookmark::new(PathBuf::from("/srv/data"), Some("   "));
        assert_eq!(bookmark.label, "data");
    }

    #[test]
    fn saving_writes_the_gtk_format() {
        let bookmarks = vec![
            Bookmark::new(PathBuf::from("/srv/data"), None),
            Bookmark::new(PathBuf::from("/home/alice/My Docs"), Some("Docs")),
        ];
        let lines: Vec<String> = bookmarks.iter().map(to_line).collect();

        assert_eq!(
            lines,
            vec![
                "file:///srv/data data".to_string(),
                "file:///home/alice/My%20Docs Docs".to_string(),
            ]
        );
    }
}
