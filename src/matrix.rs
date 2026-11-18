//! The conversion matrix, transcribed from Tangerine v1.4.
//!
//! Tangerine's landing bundle (`home-BU_Sezi2.js`) derives the wheel sectors with
//! `z(ext)`; this module is the same table expressed as data, so Mandarine's
//! registry can be checked against it. `candidate_targets` returns exactly what
//! Tangerine would offer (Mac-only outputs included); `engines::targets_for`
//! then drops anything the local machine cannot actually produce.
//!
//! See `docs/tangerine-teardown.md` for the evidence and hashes.

/// Image sources (`se` in the bundle).
pub const IMAGE_SOURCES: &[&str] = &["jpg", "jpeg", "png", "webp", "heic", "heif", "tiff", "svg", "avif", "bmp"];

/// Image targets (`qe`). Tangerine emits no `gif` from an image — `gif` is a video source.
pub const IMAGE_TARGETS: &[&str] = &["jpg", "png", "webp", "heic", "tiff", "avif", "bmp", "pdf"];

/// Audio formats (`q`), all-to-all.
pub const AUDIO_FORMATS: &[&str] = &["mp3", "m4a", "wav", "flac", "ogg", "opus", "aiff", "wma"];

/// Video containers (`T`), all-to-all plus `gif` and `mp3`.
pub const VIDEO_FORMATS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi", "wmv"];

/// Subtitle formats (`X`).
pub const SUBTITLE_FORMATS: &[&str] = &["srt", "vtt"];

/// Document sources (`re`), minus the Apple-only `pages`/`rtfd`/`textClipping`.
pub const DOCUMENT_SOURCES: &[&str] = &["docx", "doc", "odt", "rtf", "md"];

/// Document targets (`Xe`), minus the Apple-only `pages`.
pub const DOCUMENT_TARGETS: &[&str] = &["docx", "odt", "pdf", "txt", "md", "html", "epub", "png", "jpg"];

/// Spreadsheet formats (`H`).
pub const SPREADSHEET_FORMATS: &[&str] = &["xlsx", "xls", "ods", "csv", "tsv"];

/// Archive inputs that can be repacked.
pub const ARCHIVE_SOURCES: &[&str] = &["zip", "tar", "tgz", "gz", "bz2", "xz", "zst", "7z", "rar"];

/// Archive containers Tangerine can write.
pub const ARCHIVE_TARGETS: &[&str] = &["zip", "tar", "gz"];

/// PDF conversion targets, minus the macOS-only `pages` and the iWork bridges
/// (`pptx`/`xlsx`, which need Quick Look on macOS).
pub const PDF_TARGETS: &[&str] = &["docx", "jpg", "png", "epub", "txt", "md"];

/// Formats Tangerine never offers as an output (`He` in the bundle).
pub const NEVER_OUTPUT: &[&str] = &["rar", "ppt", "xls", "rtf", "rtfd", "textClipping"];

/// `z(ext)` — the candidate target list for a source extension, in Tangerine's order.
///
/// Returns an empty list for unknown extensions. The source itself is filtered out,
/// as are the `NEVER_OUTPUT` formats.
pub fn candidate_targets(source_ext: &str) -> Vec<&'static str> {
    let ext = source_ext.to_ascii_lowercase();
    let ext = ext.as_str();
    let raw: Vec<&'static str> = if IMAGE_SOURCES.contains(&ext) {
        IMAGE_TARGETS.to_vec()
    } else if AUDIO_FORMATS.contains(&ext) {
        AUDIO_FORMATS.to_vec()
    } else if VIDEO_FORMATS.contains(&ext) {
        let mut v = VIDEO_FORMATS.to_vec();
        v.extend(["gif", "mp3"]);
        v
    } else if ext == "gif" {
        VIDEO_FORMATS.to_vec()
    } else if ext == "pdf" {
        PDF_TARGETS.to_vec()
    } else if matches!(ext, "ppt" | "pptx") {
        vec!["pdf", "jpg", "png"]
    } else if SPREADSHEET_FORMATS.contains(&ext) {
        let mut v = SPREADSHEET_FORMATS.to_vec();
        v.push("pdf");
        v
    } else if DOCUMENT_SOURCES.contains(&ext) || ext == "txt" || ext == "html" {
        DOCUMENT_TARGETS.to_vec()
    } else if ext == "epub" {
        vec!["pdf", "docx", "txt", "md", "html"]
    } else if SUBTITLE_FORMATS.contains(&ext) {
        let mut v = SUBTITLE_FORMATS.to_vec();
        v.push("txt");
        v
    } else if ARCHIVE_SOURCES.contains(&ext) {
        ARCHIVE_TARGETS.to_vec()
    } else {
        vec![]
    };
    raw.into_iter()
        .filter(|t| *t != ext && !NEVER_OUTPUT.contains(t))
        .collect()
}

/// Human label for a target id (`gz` reads as GZIP, `targz` as TAR.GZ).
pub fn label(id: &str) -> String {
    match id {
        "gz" => "GZIP".into(),
        "targz" => "TAR.GZ".into(),
        "jpg" => "JPG".into(),
        other => other.to_ascii_uppercase(),
    }
}

// --- Exact bundle re-derivation -------------------------------------------
//
// `count_paths` reproduces Tangerine's `z(ext)` with the bundle's own sets,
// including the Apple-only sources Mandarine never offers. It is the number to
// compare against the press page's advertised total.

/// `se` — image sources exactly as shipped (no jpeg/jfif/heif aliases).
pub const T_IMAGE_SOURCES: &[&str] = &["jpg", "png", "webp", "heic", "tiff", "svg", "avif", "bmp"];
/// `re` — document sources exactly as shipped (Apple formats included).
pub const T_DOCUMENT_SOURCES: &[&str] = &["docx", "pages", "md", "rtf", "rtfd", "textClipping"];

/// `z(ext)`, byte-for-byte semantics of the bundle (filter self and `He`).
pub fn tangerine_targets(source_ext: &str) -> Vec<&'static str> {
    let ext = source_ext.to_ascii_lowercase();
    let ext = ext.as_str();
    let raw: Vec<&'static str> = if T_IMAGE_SOURCES.contains(&ext) {
        IMAGE_TARGETS.to_vec()
    } else if AUDIO_FORMATS.contains(&ext) {
        AUDIO_FORMATS.to_vec()
    } else if VIDEO_FORMATS.contains(&ext) {
        let mut v = VIDEO_FORMATS.to_vec();
        v.extend(["gif", "mp3"]);
        v
    } else if ext == "gif" {
        VIDEO_FORMATS.to_vec()
    } else if ext == "pdf" {
        vec!["docx", "pages", "pptx", "xlsx", "jpg", "png", "epub", "txt", "md"]
    } else if matches!(ext, "ppt" | "pptx") {
        vec!["pdf", "jpg", "png"]
    } else if SPREADSHEET_FORMATS.contains(&ext) {
        let mut v = SPREADSHEET_FORMATS.to_vec();
        v.push("pdf");
        v
    } else if T_DOCUMENT_SOURCES.contains(&ext) {
        DOCUMENT_TARGETS.to_vec()
    } else if ext == "epub" {
        vec!["pdf", "docx", "pages", "txt", "md", "html"]
    } else if ext == "txt" {
        DOCUMENT_TARGETS.to_vec()
    } else if SUBTITLE_FORMATS.contains(&ext) {
        let mut v = SUBTITLE_FORMATS.to_vec();
        v.push("txt");
        v
    } else if ["zip", "tar", "gz", "rar"].contains(&ext) {
        ARCHIVE_TARGETS.to_vec()
    } else {
        vec![]
    };
    raw.into_iter()
        .filter(|t| *t != ext && !NEVER_OUTPUT.contains(t))
        .collect()
}

/// Conversion paths re-derived from the bundle's own sets.
///
/// Three numbers matter here:
/// * **267** — faithful reproduction, including the bundle's own case bug: the
///   document source set spells `textClipping` in camelCase, while a real file's
///   extension is lower-cased, so those 9 paths never match (`z()` is
///   case-sensitive). 267 is what the landing-page table actually yields.
/// * **276** — the same table with that one typo corrected.
/// * **275** — what the `/press` page advertises.
///
/// None of the three agree, which is why the press figure is treated as marketing
/// and the table itself as the source of truth.
pub fn count_paths() -> usize {
    let mut sources: Vec<&str> = T_IMAGE_SOURCES.to_vec();
    sources.extend(AUDIO_FORMATS);
    sources.extend(VIDEO_FORMATS);
    sources.push("gif");
    sources.push("pdf");
    sources.extend(["ppt", "pptx"]);
    sources.extend(SPREADSHEET_FORMATS);
    sources.extend(T_DOCUMENT_SOURCES);
    sources.extend(["txt", "epub"]);
    sources.extend(SUBTITLE_FORMATS);
    sources.extend(["zip", "tar", "gz", "rar"]);
    sources.iter().map(|s| tangerine_targets(s).len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_matches_tangerine_table() {
        assert_eq!(
            candidate_targets("png"),
            vec!["jpg", "webp", "heic", "tiff", "avif", "bmp", "pdf"]
        );
        // svg is a source that is not itself a target.
        assert_eq!(candidate_targets("svg"), IMAGE_TARGETS.to_vec());
        // No gif from an image, and the source format is dropped.
        assert!(!candidate_targets("png").contains(&"gif"));
    }

    #[test]
    fn video_and_gif() {
        assert_eq!(candidate_targets("mp4"), vec!["mov", "mkv", "webm", "avi", "wmv", "gif", "mp3"]);
        assert_eq!(candidate_targets("gif"), VIDEO_FORMATS.to_vec());
    }

    #[test]
    fn documents_drop_apple_only_targets() {
        assert!(!candidate_targets("docx").contains(&"pages"));
        assert!(candidate_targets("docx").contains(&"pdf"));
        // txt is itself a target, so it is filtered out of its own list.
        assert_eq!(candidate_targets("txt").len(), DOCUMENT_TARGETS.len() - 1);
        assert!(candidate_targets("txt").contains(&"jpg"));
    }

    #[test]
    fn archives_never_offer_rar() {
        assert_eq!(candidate_targets("zip"), vec!["tar", "gz"]);
        assert_eq!(candidate_targets("rar"), vec!["zip", "tar", "gz"]);
        assert!(!candidate_targets("zip").contains(&"rar"));
    }

    #[test]
    fn spreadsheet_targets_drop_xls_output() {
        // `xls` is in Tangerine's never-output set, so spreadsheets lose it.
        assert_eq!(tangerine_targets("xlsx"), vec!["ods", "csv", "tsv", "pdf"]);
        assert_eq!(tangerine_targets("xls"), vec!["xlsx", "ods", "csv", "tsv", "pdf"]);
    }

    #[test]
    fn tangerine_count_reproduces_the_bundle_exactly() {
        // 267 faithful; 276 if the bundle's `textClipping` case bug were fixed;
        // 275 is the advertised number. See `count_paths`.
        assert_eq!(count_paths(), 267);
    }

    #[test]
    fn unknown_extension_is_empty() {
        assert!(candidate_targets("xyz").is_empty());
        assert!(candidate_targets("").is_empty());
    }


}
