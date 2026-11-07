//! Bridge to the desktop's *file* clipboard.
//!
//! The panel keeps its own clipboard (copy / cut / hard-link) so that paste can
//! move, duplicate or link without a round trip through the system. That keeps
//! it fast, but it also means files copied here cannot be pasted into another
//! application and vice versa.
//!
//! GNOME Files solves this by publishing a "file list" on the clipboard
//! (`nautilus-clipboard.c`): a union provider of `x-special/gnome-copied-files`
//! (a `copy`/`cut` first line followed by one `file://` URI per line) and
//! `text/uri-list`. Paste reads them in that order, falling back to a plain text
//! paste for images.
//!
//! `gpui_linux`'s clipboard only carries text and images — `ClipboardEntry::ExternalPaths`
//! exists but the Wayland backend never offers or reads `text/uri-list` — so this
//! module talks to the selection through `wl-copy` / `wl-paste` instead. That
//! keeps the change inside this crate; the trade-offs are documented on
//! [`publish`] and [`read_file_list`].

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The MIME type GNOME uses for a file list that also carries copy-vs-cut.
const GNOME_COPIED_FILES: &str = "x-special/gnome-copied-files";
/// The standard file list type (RFC 2483).
const URI_LIST: &str = "text/uri-list";

/// A file list read back from the desktop clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SystemFileList {
    pub(crate) paths: Vec<PathBuf>,
    /// The source expects the files to be *moved* (GNOME's `cut`).
    pub(crate) cut: bool,
}

/// Publishes `paths` as the desktop's file clipboard.
///
/// `wl-copy` can only offer one MIME type per call, so:
///
/// - a **copy** publishes `text/uri-list`, which GNOME Files, other file
///   managers and most toolkits understand;
/// - a **cut** publishes `x-special/gnome-copied-files`, because the
///   copy/cut distinction only exists in that format. Applications that do not
///   know it show no file paste instead of silently copying instead of moving,
///   which is the safer failure.
///
/// Only Wayland is supported; without `wl-copy` (or on X11) this is a no-op and
/// the panel keeps working with its internal clipboard.
pub(crate) fn publish(paths: &[PathBuf], cut: bool) -> anyhow::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let (mime, payload) = if cut {
        let mut payload = String::from("cut");
        for path in paths {
            payload.push('\n');
            payload.push_str(&uri_for(path));
        }
        (GNOME_COPIED_FILES, payload)
    } else {
        let mut payload = String::new();
        for (index, path) in paths.iter().enumerate() {
            if index > 0 {
                payload.push_str("\r\n");
            }
            payload.push_str(&uri_for(path));
        }
        (URI_LIST, payload)
    };
    run_wl_copy(mime, &payload)
}

/// Clears the desktop's file clipboard.
///
/// Used after a cut has been pasted (in this panel or elsewhere): the source
/// moved, so offering it again would move a path that no longer exists. GNOME
/// Files clears its own selection for the same reason.
pub(crate) fn clear() -> anyhow::Result<()> {
    let status = Command::new("wl-copy")
        .arg("--clear")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        anyhow::bail!("wl-copy --clear exited with {status}");
    }
    Ok(())
}

/// Reads a file list from the desktop clipboard, if it holds one.
///
/// Mirrors GNOME's paste priority: `x-special/gnome-copied-files` first (it
/// carries copy-vs-cut), then `text/uri-list`.
pub(crate) fn read_file_list() -> Option<SystemFileList> {
    let types = wl_paste(&["--list-types"])?;
    let offered: Vec<&str> = types.lines().map(str::trim).collect();

    if offered.contains(&GNOME_COPIED_FILES) {
        let payload = wl_paste(&["--type", GNOME_COPIED_FILES, "--no-newline"])?;
        return parse_payload(&payload, true);
    }
    if offered.contains(&URI_LIST) {
        let payload = wl_paste(&["--type", URI_LIST, "--no-newline"])?;
        return parse_payload(&payload, false);
    }
    None
}

fn run_wl_copy(mime: &str, payload: &str) -> anyhow::Result<()> {
    use std::io::Write as _;

    let mut child = Command::new("wl-copy")
        .arg("--type")
        .arg(mime)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes())?;
    }
    // `wl-copy` forks an owner process and returns; waiting keeps the pipe
    // drained so it cannot block on a full buffer.
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("wl-copy --type {mime} exited with {status}");
    }
    Ok(())
}

fn wl_paste(args: &[&str]) -> Option<String> {
    let output = Command::new("wl-paste").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `file://` URI for `path`, percent-encoding everything that is not allowed
/// unescaped in a path segment.
fn uri_for(path: &Path) -> String {
    let text = path.to_string_lossy();
    let mut uri = String::from("file://");
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(*byte as char)
            }
            other => uri.push_str(&format!("%{other:02X}")),
        }
    }
    uri
}

/// Parses a `text/uri-list` or `x-special/gnome-copied-files` payload.
///
/// `text/uri-list` (RFC 2483) allows `#` comment lines, blank lines and CRLF;
/// the GNOME format puts `copy` / `cut` on the first line.
fn parse_payload(payload: &str, has_mode_line: bool) -> Option<SystemFileList> {
    let mut lines = payload.lines();
    let mut cut = false;
    let first = lines.next()?;
    let rest = if has_mode_line {
        match first.trim() {
            "cut" => cut = true,
            "copy" => {}
            // Some writers omit the mode line even for the GNOME type; treat the
            // line as a URI in that case rather than dropping the whole paste.
            _ => {
                lines = payload.lines();
                let _ = first;
            }
        }
        lines
    } else {
        payload.lines()
    };

    let mut paths = Vec::new();
    for line in rest {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(path) = path_for(line) {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        None
    } else {
        Some(SystemFileList { paths, cut })
    }
}

/// Inverse of [`uri_for`]: accepts only local `file://` URIs.
fn path_for(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file://host/path` — only the local host (empty or `localhost`) is ours.
    let rest = match rest.split_once('/') {
        Some((host, tail)) if host.is_empty() || host.eq_ignore_ascii_case("localhost") => {
            format!("/{tail}")
        }
        Some(_) => return None,
        None => return None,
    };

    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            let byte = u8::from_str_radix(hex, 16).ok()?;
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let text = String::from_utf8(decoded).ok()?;
    if text.is_empty() {
        return None;
    }
    Some(PathBuf::from(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_round_trip_through_parsing() {
        let paths = vec![
            PathBuf::from("/home/alice/notes.txt"),
            PathBuf::from("/home/alice/with space/中文 名称.txt"),
            PathBuf::from("/home/alice/hash#and?question.txt"),
        ];
        let payload: Vec<String> = paths.iter().map(|path| uri_for(path)).collect();
        let payload = payload.join("\r\n");

        let parsed = parse_payload(&payload, false).expect("payload parses");
        assert_eq!(parsed.paths, paths);
        assert!(!parsed.cut);
    }

    #[test]
    fn gnome_payload_carries_the_cut_flag() {
        let payload = "cut\nfile:///home/alice/a.txt\nfile:///home/alice/b.txt";
        let parsed = parse_payload(payload, true).expect("payload parses");
        assert!(parsed.cut);
        assert_eq!(
            parsed.paths,
            vec![
                PathBuf::from("/home/alice/a.txt"),
                PathBuf::from("/home/alice/b.txt")
            ]
        );

        let copied = parse_payload("copy\nfile:///home/alice/a.txt", true).unwrap();
        assert!(!copied.cut);
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let payload = "# copied from somewhere\r\n\r\nfile:///tmp/a.txt\r\n";
        let parsed = parse_payload(payload, false).unwrap();
        assert_eq!(parsed.paths, vec![PathBuf::from("/tmp/a.txt")]);
    }

    #[test]
    fn remote_uris_are_ignored() {
        let payload = "file://example.com/share/a.txt\nfile:///tmp/b.txt";
        let parsed = parse_payload(payload, false).unwrap();
        assert_eq!(parsed.paths, vec![PathBuf::from("/tmp/b.txt")]);
    }

    #[test]
    fn a_mode_line_without_header_is_treated_as_a_uri() {
        // Some writers set the GNOME type but skip the `copy`/`cut` line.
        let payload = "file:///tmp/only.txt";
        let parsed = parse_payload(payload, true).unwrap();
        assert_eq!(parsed.paths, vec![PathBuf::from("/tmp/only.txt")]);
        assert!(!parsed.cut);
    }
}
