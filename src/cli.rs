use crate::actions::{self, Action};
use crate::category;
use crate::config::{self, Config};
use crate::convert::{self, BatchResult};
use crate::engines::{self, TargetKind};
use crate::matrix;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "mandarine",
    version,
    about = "Tangerine-style drag-and-drop file converter for Hyprland",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Convert files to a target format (headless; the wheel uses the same engine).
    Convert {
        /// Target: png, jpg, webp, avif, gif, bmp, tiff, pdf, txt, md, html, docx,
        /// odt, epub, mp3, m4a, wav, flac, ogg, opus, aiff, wma, mp4, mov, mkv,
        /// webm, avi, wmv, srt, vtt, csv, tsv, xlsx, xls, ods, zip, tar, gz,
        /// targz, merge, split, extract, strip, rotate90, resize50, mute,
        /// normalize, or a custom target id from config.
        #[arg(short = 't', long)]
        to: String,
        /// Write outputs into this directory instead of next to each source.
        #[arg(long)]
        out_dir: Option<PathBuf>,
        /// Files to process.
        files: Vec<PathBuf>,
    },
    /// Run a file action: copy | copy-path | terminal | compress | extract.
    Action {
        name: String,
        files: Vec<PathBuf>,
    },
    /// List probed engine capabilities and available targets per category.
    List,
    /// Write the default config to ~/.config/mandarine/config.toml (never overwrites).
    InitConfig,
    /// Open the conversion wheel (default when no subcommand is given).
    Wheel {
        /// Override cursor x (for debugging placement).
        #[arg(long)]
        x: Option<i32>,
        /// Override cursor y (for debugging placement).
        #[arg(long)]
        y: Option<i32>,
        /// Files to open the wheel with, skipping the drop. This is what a file
        /// manager integration passes (`mandarine wheel --files "$@"`), and it
        /// is the path that works when a drag cannot be handed to a new surface.
        #[arg(long = "files", value_name = "PATH")]
        files: Vec<std::path::PathBuf>,
    },
    /// Stay resident: open the wheel when a file drag touches a screen edge.
    ///
    /// Wayland has no global drag event, so this keeps a hairline drop target on
    /// the edge you choose. Start it from your compositor's autostart.
    #[cfg(target_os = "linux")]
    Listen {
        /// Edge the standby strip hugs. Pick one your bars and docks leave free.
        #[arg(long, value_enum, default_value = "top")]
        edge: crate::ui::strip::EdgeChoice,
    },
    /// Stay resident and open the wheel on Win+Y (Windows native hotkey).
    #[cfg(target_os = "windows")]
    Daemon,
}

pub fn run(cmd: Command) -> Result<i32> {
    match cmd {
        Command::Convert { to, out_dir, mut files } => {
            if files.is_empty() {
                anyhow::bail!("no input files; pass one or more paths");
            }
            for f in files.iter_mut() {
                *f = absolutize(f);
            }
            let cfg = config::load();
            let caps = engines::capabilities().clone();
            let infos = category::detect(&files);
            let target = engines::canonical_format(&to);
            let kind = kind_for(&target, &infos, &caps, &cfg);
            if out_dir.is_some() && !matches!(kind, Some(TargetKind::Format) | None) {
                anyhow::bail!("--out-dir applies to format conversions only");
            }
            let progress = |i: usize, n: usize, p: &std::path::Path| {
                eprint!("\r[{}/{}] {} ", i + 1, n, p.display());
                let _ = std::io::stderr().flush();
            };
            let result = convert::dispatch(&infos, &target, kind, &cfg, &caps, &progress);
            eprintln!();
            report(&result?)
        }
        Command::Action { name, mut files } => {
            if files.is_empty() {
                anyhow::bail!("no input files; pass one or more paths");
            }
            for f in files.iter_mut() {
                *f = absolutize(f);
            }
            let action = Action::parse(&name)
                .with_context(|| format!("unknown action `{name}` (copy, copy-path, terminal, compress, extract)"))?;
            let cfg = config::load();
            let caps = engines::capabilities().clone();
            let infos = category::detect(&files);
            let note = actions::run(action, &infos, &cfg, &caps, &crate::platform::clipboard_copy)?;
            println!("{note}");
            Ok(0)
        }
        Command::List => {
            list();
            Ok(0)
        }
        Command::InitConfig => {
            let path = config::config_path();
            if path.exists() {
                println!("config already exists: {}", path.display());
                return Ok(0);
            }
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, config::to_toml(&Config::default()))?;
            println!("wrote {}", path.display());
            Ok(0)
        }
        Command::Wheel { x, y, files } => {
            crate::ui::run(x, y, files)?;
            Ok(0)
        }
        #[cfg(target_os = "linux")]
        Command::Listen { edge } => {
            crate::ui::listen(edge)?;
            Ok(0)
        }
        #[cfg(target_os = "windows")]
        Command::Daemon => {
            let exe = std::env::current_exe().context("locating mandarine.exe")?;
            crate::platform::register_hotkey(move || {
                let _ = std::process::Command::new(&exe).arg("wheel").spawn();
            });
            // GetMessageW runs on its own thread; keep the owner alive.
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
    }
}

fn kind_for(target: &str, infos: &[category::FileInfo], caps: &engines::Capabilities, cfg: &Config) -> Option<TargetKind> {
    let targets = engines::targets_for(infos, caps, cfg);
    targets
        .iter()
        .find(|t| t.id == target)
        .map(|t| t.kind)
        .or(match target {
            "merge" => Some(TargetKind::Merge),
            "split" => Some(TargetKind::Split),
            "zip" => Some(TargetKind::Zip),
            "targz" => Some(TargetKind::TarGz),
            "extract" => Some(TargetKind::Extract),
            "strip" | "rotate90" | "resize50" | "mute" | "normalize" => Some(TargetKind::Tool),
            "tar" | "gz" if infos.iter().all(|f| f.category == category::Category::Archive) => {
                Some(TargetKind::Repack)
            }
            _ => None,
        })
}

fn absolutize(p: &PathBuf) -> PathBuf {
    if p.is_absolute() {
        p.clone()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    }
}

fn report(result: &BatchResult) -> Result<i32> {
    for outcome in &result.outcomes {
        match &outcome.error {
            Some(e) => println!("FAILED {} : {e}", outcome.input.display()),
            None => {
                for o in &outcome.outputs {
                    println!("{}", o.display());
                }
            }
        }
    }
    Ok(if result.failed_count() > 0 { 1 } else { 0 })
}

fn list() {
    let caps = engines::capabilities();
    println!("engines:");
    let mut names: Vec<_> = caps.binaries.keys().copied().collect();
    names.sort();
    for name in names {
        println!("  {:<14} {}", name, if caps.has(name) { "✔" } else { "✘" });
    }
    if caps.has("magick") {
        let mut writable: Vec<String> = caps
            .im_formats
            .iter()
            .filter(|(_, (_, w))| *w)
            .map(|(k, _)| k.clone())
            .collect();
        writable.sort();
        println!("  magick writable formats: {}", writable.join(" "));
    }
    if caps.has("ffmpeg") && !caps.ff_encoders.is_empty() {
        let mut enc: Vec<&str> = ["libx264", "libx265", "libmp3lame", "aac", "libvpx-vp9", "libopus", "libvorbis", "flac", "wmav2"]
            .into_iter()
            .filter(|e| caps.ff_encoder(e))
            .collect();
        enc.sort();
        println!("  ffmpeg encoders (tracked): {}", enc.join(" "));
    }

    println!("\nediting tools: {}", engines::tools::EditingTool::ALL.iter().map(|t| t.id()).collect::<Vec<_>>().join(" "));

    println!("\ntangerine parity: {} observable conversion paths", matrix::count_paths());
    println!("\ntargets per category (this machine):");
    let cfg = config::load();
    for (cat, sample) in [
        ("image", "sample.png"),
        ("audio", "sample.mp3"),
        ("video", "sample.mp4"),
        ("gif", "sample.gif"),
        ("pdf", "sample.pdf"),
        ("office", "sample.docx"),
        ("spreadsheet", "sample.xlsx"),
        ("text", "sample.md"),
        ("subtitle", "sample.srt"),
        ("archive", "sample.zip"),
    ] {
        let infos = category::detect(&[PathBuf::from(sample)]);
        let targets = engines::targets_for(&infos, caps, &cfg);
        let ids: Vec<String> = targets.iter().map(|t| t.id.clone()).collect();
        println!("  {:<12} {}", cat, ids.join(" "));
    }
}
