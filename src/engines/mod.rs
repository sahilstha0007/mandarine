pub mod archive;
pub mod av;
pub mod image;
pub mod office;
pub mod pdf;
pub mod spreadsheet;
pub mod subtitle;
pub mod tools;

use crate::category::{Category, FileInfo};
use crate::config::Config;
use crate::matrix;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetKind {
    /// Convert to a format id ("png", "mp4", ...).
    Format,
    Merge,
    Split,
    Extract,
    Zip,
    TarGz,
    /// Convert one archive container into another (extract, then repack).
    Repack,
    /// User-defined command steps from config.
    Custom,
    /// Parameter-light editing tool (strip, rotate, resize, mute, normalize).
    Tool,
}

#[derive(Clone, Debug)]
pub struct Target {
    pub id: String,
    pub label: String,
    pub kind: TargetKind,
}

#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub binaries: HashMap<&'static str, bool>,
    /// ImageMagick format name (uppercase, e.g. "AVIF") -> (can_read, can_write).
    pub im_formats: HashMap<String, (bool, bool)>,
    /// ffmpeg encoder names (`libx264`, `libmp3lame`, ...). Empty when the probe
    /// ran without ffmpeg.
    pub ff_encoders: HashSet<String>,
}

impl Capabilities {
    pub fn has(&self, name: &str) -> bool {
        self.binaries.get(name).copied().unwrap_or(false)
    }
    pub fn im_read(&self, fmt: &str) -> bool {
        self.im_formats.get(&fmt.to_ascii_uppercase()).map(|(r, _)| *r).unwrap_or(false)
    }
    pub fn im_write(&self, fmt: &str) -> bool {
        self.im_formats.get(&fmt.to_ascii_uppercase()).map(|(_, w)| *w).unwrap_or(false)
    }

    /// Is an ffmpeg encoder available? Falls back to "ffmpeg exists" when the
    /// encoder probe produced nothing (older builds, parse drift).
    pub fn ff_encoder(&self, name: &str) -> bool {
        if !self.has("ffmpeg") {
            return false;
        }
        self.ff_encoders.is_empty() || self.ff_encoders.contains(name)
    }

    /// Image targets gated purely on ImageMagick being present.
    pub fn base_image(&self) -> bool {
        self.has("magick")
    }

    fn ebook_bridge(&self) -> bool {
        self.has("pandoc") || self.has("ebook-convert")
    }
}

const PROBED_BINARIES: &[&str] = &[
    "ffmpeg", "ffprobe", "magick", "vips", "pdftoppm", "pdftohtml", "pdfseparate", "pdfunite",
    "pdftotext", "soffice", "zip", "unzip", "tar", "gzip", "unrar", "7z", "pandoc",
    "ebook-convert",
];

/// Probe the system once per process.
pub fn capabilities() -> &'static Capabilities {
    static CAPS: OnceLock<Capabilities> = OnceLock::new();
    CAPS.get_or_init(probe)
}

pub fn probe() -> Capabilities {
    let mut caps = Capabilities::default();
    for name in PROBED_BINARIES {
        caps.binaries.insert(name, find_in_path(name).is_some());
    }
    if caps.has("magick")
        && let Ok(out) = std::process::Command::new("magick").args(["-list", "format"]).output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        for (fmt, rw) in parse_magick_formats(&text) {
            caps.im_formats.insert(fmt, rw);
        }
    }
    if caps.has("ffmpeg")
        && let Ok(out) = std::process::Command::new("ffmpeg").args(["-hide_banner", "-encoders"]).output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        caps.ff_encoders = parse_ff_encoders(&text);
    }
    caps
}

/// Find an executable on PATH without spawning it.
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.is_file()
        && std::fs::metadata(p)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// Parse `magick -list format` output lines like:
/// `     AVIF* AVIF      rw+   AV1 Image Format (1.1.0)`
/// into (FORMAT, (read, write)).
pub fn parse_magick_formats(text: &str) -> Vec<(String, (bool, bool))> {
    let mut out = vec![];
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 3 {
            continue;
        }
        let flags = fields[2];
        let bytes = flags.as_bytes();
        if bytes.len() < 2 || (bytes[0] != b'r' && bytes[0] != b'-') || (bytes[1] != b'w' && bytes[1] != b'-') {
            continue;
        }
        let name = fields[0].trim_end_matches('*').to_ascii_uppercase();
        out.push((name, (bytes[0] == b'r', bytes[1] == b'w')));
    }
    out
}

/// Parse `ffmpeg -encoders` lines like
/// ` V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10`
/// into the encoder name column.
pub fn parse_ff_encoders(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 2 || fields[0].len() < 6 {
            continue;
        }
        let flags = fields[0];
        // Encoder lines start with six capability flags such as `V.....`.
        if !flags.chars().all(|c| c == '.' || c.is_ascii_uppercase()) {
            continue;
        }
        if !flags.starts_with(['V', 'A', 'S']) {
            continue;
        }
        out.insert(fields[1].to_string());
    }
    out
}

/// ImageMagick's format name for an output extension.
fn im_name(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "JPEG",
        "png" => "PNG",
        "webp" => "WEBP",
        "tif" | "tiff" => "TIFF",
        "bmp" => "BMP",
        "heic" => "HEIC",
        "avif" => "AVIF",
        "gif" => "GIF",
        _ => "PNG",
    }
}

/// ffmpeg encoder required for an audio/video container.
fn ff_encoder_for(ext: &str) -> &'static str {
    match ext {
        "mp3" => "libmp3lame",
        "m4a" | "aac" => "aac",
        "wav" => "pcm_s16le",
        "flac" => "flac",
        "ogg" => "libvorbis",
        "opus" => "libopus",
        "aiff" | "aif" => "pcm_s16be",
        "wma" => "wmav2",
        "mp4" | "mov" | "mkv" | "avi" => "libx264",
        "webm" => "libvpx-vp9",
        "wmv" => "wmv2",
        "gif" => "gif",
        _ => "libx264",
    }
}

/// Can this machine produce `target` from a `cat` source?
pub fn format_supported(target: &str, cat: Category, caps: &Capabilities) -> bool {
    use Category::*;
    match cat {
        Image => {
            if target == "pdf" {
                caps.base_image()
            } else {
                caps.base_image() && caps.im_write(im_name(target))
            }
        }
        Audio => caps.ff_encoder(ff_encoder_for(target)),
        Video => match target {
            "webm" => caps.ff_encoder("libvpx-vp9") && caps.ff_encoder("libopus"),
            "wmv" => caps.ff_encoder("wmv2") && caps.ff_encoder("wmav2"),
            "gif" | "mp3" => caps.ff_encoder(ff_encoder_for(target)),
            _ => caps.ff_encoder("libx264") && caps.ff_encoder("aac"),
        },
        Pdf => match target {
            "png" | "jpg" => caps.has("pdftoppm"),
            "txt" | "md" => caps.has("pdftotext"),
            "html" => caps.has("pdftohtml"),
            "docx" | "epub" => caps.ebook_bridge(),
            _ => false,
        },
        Office => match target {
            "pdf" | "txt" | "html" | "docx" | "odt" | "png" | "jpg" => caps.has("soffice"),
            "md" | "epub" => caps.ebook_bridge(),
            _ => false,
        },
        Spreadsheet => caps.has("soffice"),
        Text => {
            matches!(target, "docx" | "pdf" | "txt" | "md" | "html" | "epub") && caps.ebook_bridge()
        }
        Subtitle => (matches!(target, "srt" | "vtt") && caps.has("ffmpeg")) || target == "txt",
        Archive => match target {
            "zip" => caps.has("zip"),
            "tar" => caps.has("tar"),
            "gz" => caps.has("gzip"),
            _ => false,
        },
        Other => false,
    }
}

/// The wheel sectors offered for a selection of files.
pub fn targets_for(files: &[FileInfo], caps: &Capabilities, cfg: &Config) -> Vec<Target> {
    let mut out: Vec<Target> = vec![];
    let cats: Vec<Category> = {
        let mut s: Vec<Category> = files.iter().map(|f| f.category).collect();
        s.sort();
        s.dedup();
        s
    };
    let single = if cats.len() == 1 { Some(cats[0]) } else { None };

    if let Some(cat) = single {
        // Every candidate comes from the same source extension, so a mixed-category
        // selection (handled below) never reaches this branch.
        let source_ext = files
            .first()
            .and_then(|f| f.path.extension())
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        for id in matrix::candidate_targets(&source_ext) {
            if !format_supported(id, cat, caps) {
                continue;
            }
            let kind = match (cat, id) {
                (Category::Archive, "zip" | "tar" | "gz") => TargetKind::Repack,
                _ => TargetKind::Format,
            };
            out.push(Target { id: id.to_string(), label: matrix::label(id), kind });
        }
    }

    // PDF multi-file tools Tangerine files under the Command wheel.
    if single == Some(Category::Pdf) {
        if files.len() >= 2 && caps.has("pdfunite") {
            out.push(Target { id: "merge".into(), label: "MERGE".into(), kind: TargetKind::Merge });
        }
        if files.len() == 1 && caps.has("pdfseparate") {
            out.push(Target { id: "split".into(), label: "SPLIT".into(), kind: TargetKind::Split });
        }
    }

    if single == Some(Category::Archive) && files.iter().all(|f| archive::extractable(&f.path, caps)) && !out.iter().any(|t| t.kind == TargetKind::Repack) {
        out.push(Target { id: "extract".into(), label: "EXTRACT".into(), kind: TargetKind::Extract });
    }

    // Custom targets from config for this category set.
    for t in &cfg.custom_targets {
        let applies = cats.iter().any(|c| t.categories.iter().any(|n| n.eq_ignore_ascii_case(c.name())));
        if applies {
            out.push(Target { id: t.id.clone(), label: t.label.clone(), kind: TargetKind::Custom });
        }
    }

    // Packing works on almost anything, and is the only option for mixed drops.
    if single != Some(Category::Archive) {
        if caps.has("zip") {
            out.push(Target { id: "zip".into(), label: "ZIP".into(), kind: TargetKind::Zip });
        }
        if caps.has("tar") {
            out.push(Target { id: "targz".into(), label: "TAR.GZ".into(), kind: TargetKind::TarGz });
        }
    }
    out
}

/// Editing tools offered for the current selection, on their own wheel page.
pub fn tools_for(files: &[FileInfo], caps: &Capabilities) -> Vec<Target> {
    tools::tools_for(files, caps)
}

/// Resolve a user-typed format id to a canonical target id, accepting aliases.
pub fn canonical_format(id: &str) -> String {
    match id.to_ascii_lowercase().as_str() {
        "jpeg" | "jfif" => "jpg".into(),
        "tif" => "tiff".into(),
        "m4v" => "mp4".into(),
        "aif" => "aiff".into(),
        "htm" => "html".into(),
        "tar.gz" | "tgz" => "targz".into(),
        other => other.to_string(),
    }
}

/// Run a sequence of argv steps (no shell involved).
pub fn run_steps(steps: &[Vec<String>], cwd: Option<&Path>) -> Result<()> {
    for argv in steps {
        let (cmd, args) = argv.split_first().context("empty command step")?;
        let mut command = std::process::Command::new(cmd);
        command.args(args);
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        let output = command.output().with_context(|| format!("failed to run {cmd}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let tail: String = stderr.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
            anyhow::bail!("`{}` failed: {}", argv.join(" "), tail.trim());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_magick_format_lines() {
        let sample = r#"
   Format  Module    Mode  Description
-------------------------------------------------------------------------------
      AVIF* AVIF      rw+   AV1 Image Format (1.1.0)
       HEIC* HEIC     r--   HEIF Image Format
      JPEG* JPEG      rw-   Joint Photographic Experts Group JFIF format (80)
       PNG* PNG       rw+   Portable Network Graphics (1.6.4)
      noop
"#;
        let m: HashMap<String, (bool, bool)> = parse_magick_formats(sample).into_iter().collect();
        assert_eq!(m.get("AVIF"), Some(&(true, true)));
        assert_eq!(m.get("HEIC"), Some(&(true, false)));
        assert_eq!(m.get("JPEG"), Some(&(true, true)));
        assert_eq!(m.get("PNG"), Some(&(true, true)));
        assert!(!m.contains_key("NOOP"));
    }

    #[test]
    fn parses_ffmpeg_encoder_lines() {
        let sample = r#"
Encoders:
 V..... = Video
 ------
 V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC (codec h264)
 A....D libmp3lame           libmp3lame MP3 (MPEG audio layer 3) (codec mp3)
 A....D aac                  AAC (Advanced Audio Coding)
 A....D pcm_s16le            PCM signed 16-bit little-endian
"#;
        let e = parse_ff_encoders(sample);
        assert!(e.contains("libx264"));
        assert!(e.contains("libmp3lame"));
        assert!(e.contains("aac"));
        assert!(e.contains("pcm_s16le"));
        assert!(!e.contains("Encoders:"));
    }

    fn caps(magick: bool, ffmpeg: bool) -> Capabilities {
        let mut c = Capabilities::default();
        c.binaries.insert("magick", magick);
        c.binaries.insert("ffmpeg", ffmpeg);
        c
    }

    #[test]
    fn registry_gates_on_capabilities() {
        let cfg = Config::default();
        let files = crate::category::detect(&[PathBuf::from("/tmp/a.png")]);

        let mut jpeg_only = caps(true, false);
        jpeg_only.im_formats.insert("JPEG".into(), (true, true));
        let with = targets_for(&files, &jpeg_only, &cfg);
        assert!(with.iter().any(|t| t.id == "jpg"));
        assert!(!with.iter().any(|t| t.id == "avif"), "avif needs IM delegate");

        let without = targets_for(&files, &caps(false, true), &cfg);
        assert!(!without.iter().any(|t| t.id == "jpg"));

        let mut avif_caps = caps(true, false);
        avif_caps.im_formats.insert("AVIF".into(), (true, true));
        let avif = targets_for(&files, &avif_caps, &cfg);
        assert!(avif.iter().any(|t| t.id == "avif"));
    }

    #[test]
    fn image_targets_match_tangerine_and_exclude_source() {
        let cfg = Config::default();
        let mut c = caps(true, false);
        for f in ["JPEG", "PNG", "WEBP", "TIFF", "BMP", "GIF"] {
            c.im_formats.insert(f.into(), (true, true));
        }
        let files = crate::category::detect(&[PathBuf::from("/tmp/a.png")]);
        let ids: Vec<String> = targets_for(&files, &c, &cfg).into_iter().map(|t| t.id).collect();
        // jpg webp tiff bmp pdf, in Tangerine's order, then the packing sectors.
        assert_eq!(ids[..5], ["jpg", "webp", "tiff", "bmp", "pdf"]);
        assert!(!ids.contains(&"png".to_string()), "source format is dropped");
        assert!(!ids.contains(&"gif".to_string()), "images do not convert to gif (parity)");
    }

    #[test]
    fn mixed_selection_offers_packing() {
        let cfg = Config::default();
        let mut c = caps(true, true);
        c.binaries.insert("zip", true);
        c.binaries.insert("tar", true);
        let files = crate::category::detect(&[PathBuf::from("/tmp/a.png"), PathBuf::from("/tmp/b.mp3")]);
        let targets = targets_for(&files, &c, &cfg);
        assert!(!targets.iter().any(|t| t.kind == TargetKind::Format));
        assert!(targets.iter().any(|t| t.id == "zip" && t.kind == TargetKind::Zip));
        assert!(targets.iter().any(|t| t.id == "targz"));
    }

    #[test]
    fn archive_selection_offers_repack_not_packing() {
        let cfg = Config::default();
        let mut c = Capabilities::default();
        c.binaries.insert("zip", true);
        c.binaries.insert("tar", true);
        c.binaries.insert("gzip", true);
        c.binaries.insert("7z", true);
        let files = crate::category::detect(&[PathBuf::from("/tmp/a.zip")]);
        let targets = targets_for(&files, &c, &cfg);
        assert!(targets.iter().any(|t| t.id == "tar" && t.kind == TargetKind::Repack));
        assert!(targets.iter().any(|t| t.id == "gz" && t.kind == TargetKind::Repack));
        assert!(!targets.iter().any(|t| t.id == "zip"), "zip from zip is dropped");
        assert!(!targets.iter().any(|t| t.kind == TargetKind::Zip));
    }

    #[test]
    fn subtitle_targets_need_ffmpeg_for_binary_formats() {
        let cfg = Config::default();
        let files = crate::category::detect(&[PathBuf::from("/tmp/a.srt")]);
        let no_ff = targets_for(&files, &caps(true, false), &cfg);
        assert_eq!(no_ff.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["txt"]);
        let with_ff = targets_for(&files, &caps(false, true), &cfg);
        assert!(with_ff.iter().any(|t| t.id == "vtt"));
    }

    #[test]
    fn canonical_aliases() {
        assert_eq!(canonical_format("JPEG"), "jpg");
        assert_eq!(canonical_format("TAR.GZ"), "targz");
        assert_eq!(canonical_format("AIF"), "aiff");
        assert_eq!(canonical_format("PNG"), "png");
    }
}
