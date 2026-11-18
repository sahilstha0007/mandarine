//! Parameter-light editing tools — the slice of Tangerine's Command wheel,
//! DotMorph's TOOLS and twistel's operations that needs no preview dialog.
//!
//! shortcut: fixed, sensible parameters; upgrade to per-tool dialogs with live
//! previews when the wheel gets a settings surface.

use crate::category::Category;
use crate::engines::{Capabilities, Target, TargetKind};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditingTool {
    Strip,
    Rotate90,
    Resize50,
    Mute,
    Normalize,
}

impl EditingTool {
    pub const ALL: &'static [EditingTool] = &[
        EditingTool::Strip,
        EditingTool::Rotate90,
        EditingTool::Resize50,
        EditingTool::Mute,
        EditingTool::Normalize,
    ];

    pub fn id(self) -> &'static str {
        match self {
            EditingTool::Strip => "strip",
            EditingTool::Rotate90 => "rotate90",
            EditingTool::Resize50 => "resize50",
            EditingTool::Mute => "mute",
            EditingTool::Normalize => "normalize",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EditingTool::Strip => "STRIP META",
            EditingTool::Rotate90 => "ROTATE 90",
            EditingTool::Resize50 => "RESIZE 50",
            EditingTool::Mute => "MUTE",
            EditingTool::Normalize => "NORMALIZE",
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            EditingTool::Strip => "-clean",
            EditingTool::Rotate90 => "-rotated",
            EditingTool::Resize50 => "-resized",
            EditingTool::Mute => "-muted",
            EditingTool::Normalize => "-normalized",
        }
    }

    pub fn categories(self) -> &'static [Category] {
        match self {
            EditingTool::Strip => &[Category::Image, Category::Audio, Category::Video],
            EditingTool::Rotate90 | EditingTool::Resize50 => &[Category::Image],
            EditingTool::Mute => &[Category::Video],
            EditingTool::Normalize => &[Category::Audio],
        }
    }

    pub fn available(self, caps: &Capabilities) -> bool {
        match self {
            EditingTool::Strip => caps.has("magick") || caps.has("ffmpeg"),
            EditingTool::Rotate90 | EditingTool::Resize50 => caps.has("magick"),
            EditingTool::Mute | EditingTool::Normalize => caps.has("ffmpeg"),
        }
    }
}

pub fn from_id(id: &str) -> Option<EditingTool> {
    EditingTool::ALL.iter().copied().find(|t| t.id() == id)
}

/// The editing tools offered for a selection, gated on installed engines.
pub fn tools_for(files: &[crate::category::FileInfo], caps: &Capabilities) -> Vec<Target> {
    let mut out = vec![];
    for tool in EditingTool::ALL {
        if files.iter().all(|f| tool.categories().contains(&f.category)) && tool.available(caps) {
            out.push(Target {
                id: tool.id().into(),
                label: tool.label().into(),
                kind: TargetKind::Tool,
            });
        }
    }
    out
}

/// Build the argv steps for one tool. Images go through ImageMagick; audio and
/// video through ffmpeg with stream copy where no re-encode is needed.
pub fn steps(tool: EditingTool, input: &Path, output: &Path, category: Category) -> Vec<Vec<String>> {
    let inp = input.to_string_lossy().into_owned();
    let out = output.to_string_lossy().into_owned();
    let argv: Vec<String> = match (tool, category) {
        (EditingTool::Strip, Category::Image) => vec!["magick".into(), inp, "-strip".into(), out],
        (EditingTool::Strip, _) => vec![
            "ffmpeg".into(), "-y".into(), "-i".into(), inp,
            "-map".into(), "0".into(), "-c".into(), "copy".into(),
            "-map_metadata".into(), "-1".into(), "-map_chapters".into(), "-1".into(), out,
        ],
        (EditingTool::Rotate90, _) => vec!["magick".into(), inp, "-rotate".into(), "90".into(), out],
        (EditingTool::Resize50, _) => vec!["magick".into(), inp, "-resize".into(), "50%".into(), out],
        (EditingTool::Mute, _) => vec![
            "ffmpeg".into(), "-y".into(), "-i".into(), inp,
            "-map".into(), "0".into(), "-map".into(), "-0:a".into(),
            "-c".into(), "copy".into(), out,
        ],
        (EditingTool::Normalize, _) => vec![
            "ffmpeg".into(), "-y".into(), "-i".into(), inp,
            "-af".into(), "loudnorm".into(), out,
        ],
    };
    vec![argv]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn argv(tool: EditingTool, cat: Category) -> Vec<String> {
        steps(tool, Path::new("/a/b.png"), Path::new("/a/b-clean.png"), cat)[0].clone()
    }

    #[test]
    fn tool_ids_round_trip() {
        for t in EditingTool::ALL {
            assert_eq!(from_id(t.id()), Some(*t));
        }
        assert_eq!(from_id("nope"), None);
    }

    #[test]
    fn image_tools_use_magick() {
        assert_eq!(argv(EditingTool::Strip, Category::Image),
            vec!["magick", "/a/b.png", "-strip", "/a/b-clean.png"]);
        assert_eq!(argv(EditingTool::Rotate90, Category::Image),
            vec!["magick", "/a/b.png", "-rotate", "90", "/a/b-clean.png"]);
        assert_eq!(argv(EditingTool::Resize50, Category::Image),
            vec!["magick", "/a/b.png", "-resize", "50%", "/a/b-clean.png"]);
    }

    #[test]
    fn av_tools_drop_metadata_audio_without_reencoding() {
        let strip = argv(EditingTool::Strip, Category::Video);
        assert!(strip.contains(&"-map_metadata".to_string()));
        assert!(strip.contains(&"copy".to_string()));
        let mute = argv(EditingTool::Mute, Category::Video);
        assert!(mute.contains(&"-0:a".to_string()));
        assert!(mute.contains(&"copy".to_string()));
        let norm = argv(EditingTool::Normalize, Category::Audio);
        assert!(norm.contains(&"loudnorm".to_string()));
    }

    #[test]
    fn gating_requires_the_right_engine() {
        let mut caps = Capabilities::default();
        assert!(tools_for(&crate::category::detect(&[PathBuf::from("/a.png")]), &caps).is_empty());
        caps.binaries.insert("magick", true);
        let targets = tools_for(&crate::category::detect(&[PathBuf::from("/a.png")]), &caps);
        let ids: Vec<&str> = targets.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["strip", "rotate90", "resize50"]);
    }
}
