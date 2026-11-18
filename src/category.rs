use std::collections::HashSet;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Category {
    Image,
    Audio,
    Video,
    Pdf,
    Office,
    Spreadsheet,
    Text,
    Subtitle,
    Archive,
    Other,
}

impl Category {
    pub fn name(self) -> &'static str {
        match self {
            Category::Image => "image",
            Category::Audio => "audio",
            Category::Video => "video",
            Category::Pdf => "pdf",
            Category::Office => "office",
            Category::Spreadsheet => "spreadsheet",
            Category::Text => "text",
            Category::Subtitle => "subtitle",
            Category::Archive => "archive",
            Category::Other => "other",
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileInfo {
    pub path: std::path::PathBuf,
    pub category: Category,
}

// `gif` is a video source (Tangerine's `z("gif")` returns video containers).
const IMAGE: &[&str] = &["jpg", "jpeg", "jfif", "png", "webp", "bmp", "tif", "tiff", "avif", "heic", "heif", "svg"];
const AUDIO: &[&str] = &["mp3", "m4a", "aac", "wav", "flac", "ogg", "oga", "opus", "aiff", "aif", "wma"];
const VIDEO: &[&str] = &["mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "flv", "gif", "mpg", "mpeg", "ts", "m2ts", "3gp"];
const OFFICE: &[&str] = &["doc", "docx", "ppt", "pptx", "odt", "odp", "rtf"];
const SPREADSHEET: &[&str] = &["xls", "xlsx", "ods", "csv", "tsv"];
const SUBTITLE: &[&str] = &["srt", "vtt", "ass", "ssa", "sub"];
const ARCHIVE: &[&str] = &["zip", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "zst", "7z", "rar"];
const TEXT: &[&str] = &["txt", "md", "html", "htm", "epub"];

pub fn category_of(path: &Path) -> Category {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if IMAGE.contains(&ext.as_str()) {
        Category::Image
    } else if AUDIO.contains(&ext.as_str()) {
        Category::Audio
    } else if VIDEO.contains(&ext.as_str()) {
        Category::Video
    } else if ext == "pdf" {
        Category::Pdf
    } else if OFFICE.contains(&ext.as_str()) {
        Category::Office
    } else if SPREADSHEET.contains(&ext.as_str()) {
        Category::Spreadsheet
    } else if SUBTITLE.contains(&ext.as_str()) {
        Category::Subtitle
    } else if ARCHIVE.contains(&ext.as_str()) {
        Category::Archive
    } else if TEXT.contains(&ext.as_str()) {
        Category::Text
    } else if !ext.is_empty() {
        // Fall back to MIME guessing for unknown extensions.
        let mime = mime_guess::from_path(path).first_raw().unwrap_or("");
        match mime.split('/').next().unwrap_or("") {
            "image" => Category::Image,
            "audio" => Category::Audio,
            "video" => Category::Video,
            "text" => Category::Text,
            _ => Category::Other,
        }
    } else {
        Category::Other
    }
}

pub fn detect(paths: &[std::path::PathBuf]) -> Vec<FileInfo> {
    paths.iter().map(|p| FileInfo { path: p.clone(), category: category_of(p) }).collect()
}

/// The single category shared by every file, if there is one.
pub fn common_category(files: &[FileInfo]) -> Option<Category> {
    let set: HashSet<Category> = files.iter().map(|f| f.category).collect();
    (set.len() == 1).then(|| *set.iter().next().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_common_extensions() {
        for (path, want) in [
            ("/tmp/a.JPG", Category::Image),
            ("/tmp/a.heic", Category::Image),
            ("/tmp/a.mp4", Category::Video),
            ("/tmp/a.MKV", Category::Video),
            ("/tmp/a.pdf", Category::Pdf),
            ("/tmp/a.docx", Category::Office),
            ("/tmp/a.tar.gz", Category::Archive),
            ("/tmp/a.md", Category::Text),
            ("/tmp/a.png", Category::Image),
            ("/tmp/a.gif", Category::Video),
            ("/tmp/a.csv", Category::Spreadsheet),
            ("/tmp/a.srt", Category::Subtitle),
            ("/tmp/a.epub", Category::Text),
        ] {
            assert_eq!(category_of(Path::new(path)), want, "{path}");
        }
    }

    #[test]
    fn unknown_extension_falls_back_to_mime_then_other() {
        // No extension, no mime: Other.
        assert_eq!(category_of(Path::new("/tmp/makefile")), Category::Other);
    }

    #[test]
    fn common_category_requires_uniform_set() {
        let files = detect(&[PathBuf::from("/tmp/a.png"), PathBuf::from("/tmp/b.jpg")]);
        assert_eq!(common_category(&files), Some(Category::Image));
        let mixed = detect(&[PathBuf::from("/tmp/a.png"), PathBuf::from("/tmp/b.mp3")]);
        assert_eq!(common_category(&mixed), None);
    }
}
