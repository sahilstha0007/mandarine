//! File actions — the Linux equivalents of Tangerine's Control wheel
//! (AirDrop, Float, Copy, Move, Copy Path, Terminal).
//!
//! AirDrop and Float have no direct Hyprland counterpart and are not claimed.
//! Move needs a destination picker, so it lives in the GUI as a folder chooser;
//! the headless path here covers Copy, Copy Path, Terminal, Compress and Extract.

use crate::category::{Category, FileInfo};
use crate::config::Config;
use crate::convert;
use crate::engines::{self, archive, Capabilities};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Put the selection on the clipboard as `text/uri-list`.
    Copy,
    /// Put the newline-separated absolute paths on the clipboard.
    CopyPath,
    /// Open the file's folder in the configured terminal.
    Terminal,
    /// Zip/tar.gz the selection beside the first file.
    Compress,
    /// Unpack archives into a sibling folder.
    Extract,
}

impl Action {
    pub fn id(self) -> &'static str {
        match self {
            Action::Copy => "copy",
            Action::CopyPath => "copy-path",
            Action::Terminal => "terminal",
            Action::Compress => "compress",
            Action::Extract => "extract",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Copy => "COPY",
            Action::CopyPath => "COPY PATH",
            Action::Terminal => "TERMINAL",
            Action::Compress => "COMPRESS",
            Action::Extract => "EXTRACT",
        }
    }

    pub fn parse(id: &str) -> Option<Action> {
        match id.to_ascii_lowercase().replace('_', "-").as_str() {
            "copy" => Some(Action::Copy),
            "copy-path" | "copypath" | "path" => Some(Action::CopyPath),
            "terminal" | "term" => Some(Action::Terminal),
            "compress" | "zip" => Some(Action::Compress),
            "extract" | "unpack" => Some(Action::Extract),
            _ => None,
        }
    }
}

/// Which actions make sense for this selection.
pub fn available(files: &[FileInfo], caps: &Capabilities) -> Vec<Action> {
    let mut out = vec![Action::Copy, Action::CopyPath, Action::Terminal];
    if caps.has("zip") || caps.has("tar") {
        out.push(Action::Compress);
    }
    if !files.is_empty() && files.iter().all(|f| archive::extractable(&f.path, caps)) {
        out.push(Action::Extract);
    }
    out
}

/// RFC 2483 / XDG `text/uri-list` body: one `file://` URI per line, percent-encoded.
pub fn uri_list(paths: &[PathBuf]) -> String {
    let mut out = String::new();
    for p in paths {
        out.push_str("file://");
        out.push_str(&percent_encode(&p.to_string_lossy()));
        out.push_str("\r\n");
    }
    out
}

/// Percent-encode everything outside the unreserved set, keeping `/` as a separator.
pub fn percent_encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

const TERMINAL_CANDIDATES: &[&str] = &["kitty", "wezterm", "foot", "ghostty", "alacritty", "konsole", "gnome-terminal"];

/// Resolve the terminal to launch: the configured one, else the first installed.
pub fn resolve_terminal(cfg: &Config) -> Option<String> {
    if !cfg.terminal.is_empty() && !cfg.terminal.eq_ignore_ascii_case("auto") {
        return Some(cfg.terminal.clone());
    }
    TERMINAL_CANDIDATES
        .iter()
        .find(|name| engines::find_in_path(name).is_some())
        .map(|name| name.to_string())
}

/// The directory a Terminal action should open, and the argv to open it.
pub fn terminal_cmd(path: &Path) -> Vec<String> {
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
    };
    vec![dir.to_string_lossy().into_owned()]
}

/// Run an action. `copy` is the clipboard sink (GDK in the GUI, `wl-copy` in the
/// CLI) so this stays unit-testable.
pub fn run(
    action: Action,
    files: &[FileInfo],
    cfg: &Config,
    caps: &Capabilities,
    copy: &dyn Fn(&str, &str) -> Result<()>,
) -> Result<String> {
    let paths: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
    if paths.is_empty() {
        anyhow::bail!("nothing selected");
    }
    match action {
        Action::Copy => {
            copy(&uri_list(&paths), "text/uri-list")?;
            Ok(format!("Copied {} item{}", paths.len(), plural(paths.len())))
        }
        Action::CopyPath => {
            let text = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n");
            copy(&text, "text/plain")?;
            Ok(format!("Copied {} path{}", paths.len(), plural(paths.len())))
        }
        Action::Terminal => {
            let term = resolve_terminal(cfg).context("no terminal found (set `terminal` in config.toml)")?;
            let argv = terminal_cmd(&paths[0]);
            let mut cmd = std::process::Command::new(&term);
            cmd.args(&argv);
            cmd.stdin(std::process::Stdio::null());
            cmd.stdout(std::process::Stdio::null());
            cmd.stderr(std::process::Stdio::null());
            cmd.spawn().with_context(|| format!("launching {term}"))?;
            Ok(format!("Opened {term} in {}", argv[0]))
        }
        Action::Compress => {
            let target = if caps.has("zip") { "zip" } else { "targz" };
            let outputs = convert::pack(files, target, caps)?;
            Ok(format!("Created {}", outputs[0].display()))
        }
        Action::Extract => {
            let mut made = vec![];
            for f in files {
                if f.category != Category::Archive {
                    continue;
                }
                made.extend(convert::extract_archive(&f.path, caps)?);
            }
            if made.is_empty() {
                anyhow::bail!("no extractable archives in the selection");
            }
            Ok(format!("Extracted {} archive{}", made.len(), plural(made.len())))
        }
    }
}

/// Default clipboard sink on Wayland: `wl-copy` with the given MIME type.
pub fn wl_copy(body: &str, mime: &str) -> Result<()> {
    use std::io::Write;
    let mut child = std::process::Command::new("wl-copy")
        .args(["--type", mime])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("wl-copy not found (install wl-clipboard)")?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(body.as_bytes())?;
    }
    drop(child.stdin.take());
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("wl-copy exited with {status}");
    }
    Ok(())
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[&str]) -> Vec<FileInfo> {
        crate::category::detect(&paths.iter().map(PathBuf::from).collect::<Vec<_>>())
    }

    fn caps(present: &[&'static str]) -> Capabilities {
        let mut c = Capabilities::default();
        for p in present {
            c.binaries.insert(*p, true);
        }
        c
    }

    #[test]
    fn uri_list_percent_encodes_and_terminates_lines() {
        let list = uri_list(&[PathBuf::from("/home/sahil/a b.png"), PathBuf::from("/tmp/x.pdf")]);
        assert_eq!(list, "file:///home/sahil/a%20b.png\r\nfile:///tmp/x.pdf\r\n");
    }

    #[test]
    fn copy_path_joins_with_newlines() {
        let got = run(
            Action::CopyPath,
            &files(&["/tmp/a.png", "/tmp/b.png"]),
            &Config::default(),
            &caps(&[]),
            &|text, mime| {
                assert_eq!(text, "/tmp/a.png\n/tmp/b.png");
                assert_eq!(mime, "text/plain");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(got, "Copied 2 paths");
    }

    #[test]
    fn terminal_opens_the_containing_folder() {
        assert_eq!(terminal_cmd(Path::new("/a/b/c.png")), vec!["/a/b"]);
    }

    #[test]
    fn available_actions_gate_on_engines() {
        let f = files(&["/tmp/a.png"]);
        let ids: Vec<&str> = available(&f, &caps(&[])).into_iter().map(Action::id).collect();
        assert_eq!(ids, ["copy", "copy-path", "terminal"]);
        let with = available(&f, &caps(&["zip"]));
        assert!(with.contains(&Action::Compress));
        assert!(!with.contains(&Action::Extract));

        let zipped = files(&["/tmp/a.zip"]);
        let zcaps = caps(&["zip", "tar", "7z"]);
        assert!(available(&zipped, &zcaps).contains(&Action::Extract));
    }

    #[test]
    fn action_ids_round_trip() {
        for a in [Action::Copy, Action::CopyPath, Action::Terminal, Action::Compress, Action::Extract] {
            assert_eq!(Action::parse(a.id()), Some(a));
        }
        assert_eq!(Action::parse("nope"), None);
    }
}
