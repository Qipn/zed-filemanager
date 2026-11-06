//! Human-readable labels for the details view: the type column and the
//! modification date.

use crate::model::entries::DirEntry;

/// The "Type" column: a human readable name for the entry.
pub(crate) fn type_label(entry: &DirEntry) -> String {
    if entry.is_dir {
        return "Folder".into();
    }
    let Some(extension) = entry.path.extension().and_then(|ext| ext.to_str()) else {
        return "File".into();
    };
    match extension.to_ascii_lowercase().as_str() {
        "txt" | "md" | "log" | "rs" | "toml" | "json" | "yaml" | "yml" | "js" | "ts" | "tsx"
        | "jsx" | "py" | "rb" | "go" | "c" | "h" | "cpp" | "hpp" | "java" | "css" | "html"
        | "sh" => "Text Document".into(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico" => "Image".into(),
        "mp3" | "wav" | "flac" | "ogg" => "Audio".into(),
        "mp4" | "mkv" | "mov" | "webm" | "avi" => "Video".into(),
        "zip" | "tar" | "gz" | "xz" | "bz2" | "7z" => "Archive".into(),
        "pdf" => "PDF Document".into(),
        other => other.to_uppercase(),
    }
}

/// The "Date modified" column: OpenXplorer shows today's files with a time,
/// older ones with the date.
pub(crate) fn format_modified(mtime: Option<fs::MTime>) -> String {
    let Some(mtime) = mtime else {
        return String::new();
    };
    let system_time = mtime.timestamp_for_user();
    let Ok(duration) = system_time.duration_since(std::time::UNIX_EPOCH) else {
        return String::new();
    };
    let Some(datetime) = chrono::DateTime::from_timestamp(duration.as_secs() as i64, 0) else {
        return String::new();
    };
    let local = datetime.with_timezone(&chrono::Local);
    let now = chrono::Local::now();
    if local.date_naive() == now.date_naive() {
        local.format("%-I:%M %p").to_string()
    } else {
        local.format("%m/%d/%Y").to_string()
    }
}
