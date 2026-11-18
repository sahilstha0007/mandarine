use crate::category::{Category, FileInfo};
use crate::config::Config;
use crate::engines::{self, archive, av, image, office, pdf, spreadsheet, subtitle, tools, Capabilities, TargetKind};
use crate::naming;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct JobOutcome {
    pub input: PathBuf,
    pub outputs: Vec<PathBuf>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct BatchResult {
    pub outcomes: Vec<JobOutcome>,
}

impl BatchResult {
    pub fn ok_count(&self) -> usize {
        self.outcomes.iter().filter(|o| o.error.is_none()).count()
    }
    pub fn failed_count(&self) -> usize {
        self.outcomes.iter().filter(|o| o.error.is_some()).count()
    }
}

/// Progress callback: (input index, total inputs, current input path).
pub type ProgressFn<'a> = &'a dyn Fn(usize, usize, &Path);

/// Run `files` through `format` (a canonical target id) sequentially.
pub fn run(files: &[FileInfo], target: &str, cfg: &Config, caps: &Capabilities, progress: ProgressFn) -> BatchResult {
    let mut outcomes = vec![];
    for (i, file) in files.iter().enumerate() {
        progress(i, files.len(), &file.path);
        let result = run_one(file, target, cfg, caps);
        let outcome = match result {
            Ok(outputs) => JobOutcome { input: file.path.clone(), outputs, error: None },
            Err(e) => JobOutcome {
                input: file.path.clone(),
                outputs: vec![],
                error: Some(format!("{e:#}")),
            },
        };
        outcomes.push(outcome);
    }
    BatchResult { outcomes }
}

fn run_one(file: &FileInfo, target: &str, cfg: &Config, caps: &Capabilities) -> Result<Vec<PathBuf>> {
    match file.category {
        Category::Image | Category::Other => convert_image(&file.path, target, cfg, caps),
        Category::Audio | Category::Video => convert_av(&file.path, target, cfg, caps),
        Category::Pdf => convert_pdf(&file.path, target, cfg, caps),
        Category::Office => convert_office(&file.path, target, caps),
        Category::Spreadsheet => convert_spreadsheet(&file.path, target, caps),
        Category::Text => convert_text(&file.path, target, caps),
        Category::Subtitle => convert_subtitle(&file.path, target),
        Category::Archive => anyhow::bail!("archives convert through the repack path"),
    }
}

fn convert_image(input: &Path, target: &str, cfg: &Config, caps: &Capabilities) -> Result<Vec<PathBuf>> {
    if target == "pdf" {
        let out = naming::unique_sibling(input, "pdf");
        engines::run_steps(&image::to_pdf_cmd(input, &out), None)?;
        return Ok(vec![out]);
    }
    // PDF -> raster goes through poppler, everything else through ImageMagick.
    if input.extension().and_then(|e| e.to_str()).unwrap_or("").eq_ignore_ascii_case("pdf") {
        return pdf_to_image(input, target, cfg);
    }
    if !caps.has("magick") {
        anyhow::bail!("ImageMagick (magick) not found");
    }
    let ext = target.to_ascii_lowercase();
    let out = naming::unique_sibling(input, &ext);
    engines::run_steps(&image::convert_cmd(input, &out), None)?;
    Ok(vec![out])
}

fn pdf_to_image(input: &Path, target: &str, cfg: &Config) -> Result<Vec<PathBuf>> {
    if !engines::capabilities().has("pdftoppm") {
        anyhow::bail!("pdftoppm not found");
    }
    let ext = target.to_ascii_lowercase();
    let dir = input.parent().unwrap_or(Path::new(".")).to_path_buf();
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("page").to_string();
    // Poppler writes \"stem.mandarine-1.png\"-style pages; give them friendly names.
    let prefix = dir.join(format!("{stem}.mandarine"));
    engines::run_steps(&pdf::to_image_cmd(input, &prefix, &ext, cfg), None)?;
    let pages = pdf::collect_page_outputs(&prefix, &ext);
    if pages.len() == 1 {
        let friendly = naming::unique_named(&dir, &stem, &ext);
        if friendly != pages[0] {
            std::fs::rename(&pages[0], &friendly)?;
            return Ok(vec![friendly]);
        }
        return Ok(pages);
    }
    let mut out = vec![];
    for (i, page) in pages.iter().enumerate() {
        let friendly = naming::unique_named(&dir, &format!("{stem}-page{}", i + 1), &ext);
        std::fs::rename(page, &friendly)?;
        out.push(friendly);
    }
    Ok(out)
}

fn convert_av(input: &Path, target: &str, cfg: &Config, _caps: &Capabilities) -> Result<Vec<PathBuf>> {
    if !engines::capabilities().has("ffmpeg") {
        anyhow::bail!("ffmpeg not found");
    }
    let out = naming::unique_sibling(input, &target.to_ascii_lowercase());
    engines::run_steps(&av::convert_cmd(input, &out, cfg), None)?;
    Ok(vec![out])
}

fn convert_pdf(input: &Path, target: &str, cfg: &Config, caps: &Capabilities) -> Result<Vec<PathBuf>> {
    match target {
        "png" | "jpg" => pdf_to_image(input, target, cfg),
        "txt" | "md" => {
            if !caps.has("pdftotext") {
                anyhow::bail!("pdftotext not found");
            }
            let out = naming::unique_sibling(input, target);
            engines::run_steps(&pdf::to_txt_cmd(input, &out), None)?;
            Ok(vec![out])
        }
        "html" => {
            if !caps.has("pdftohtml") {
                anyhow::bail!("pdftohtml not found");
            }
            let dir = input.parent().unwrap_or(Path::new(".")).to_path_buf();
            let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("page").to_string();
            let prefix = dir.join(format!("{stem}.mandarine"));
            engines::run_steps(&pdf::to_html_cmd(input, &prefix), None)?;
            let produced = dir.join(format!("{stem}.mandarine.html"));
            if !produced.exists() {
                anyhow::bail!("pdftohtml produced no output for {}", input.display());
            }
            let friendly = naming::unique_named(&dir, &stem, "html");
            std::fs::rename(&produced, &friendly)?;
            Ok(vec![friendly])
        }
        other => convert_with_pandoc(input, other, caps),
    }
}

fn convert_office(input: &Path, target: &str, caps: &Capabilities) -> Result<Vec<PathBuf>> {
    match target {
        "md" => convert_with_pandoc(input, target, caps),
        "epub" if caps.has("ebook-convert") => convert_with_ebook(input, target),
        _ => {
            if !caps.has("soffice") {
                anyhow::bail!("LibreOffice (soffice) not found");
            }
            let out = naming::unique_sibling(input, target);
            soffice_via_temp(input, target, &out, |dir| office::convert_cmd(input, dir, target))
        }
    }
}

fn convert_spreadsheet(input: &Path, target: &str, caps: &Capabilities) -> Result<Vec<PathBuf>> {
    if !caps.has("soffice") {
        anyhow::bail!("LibreOffice (soffice) not found");
    }
    let out = naming::unique_sibling(input, target);
    soffice_via_temp(input, target, &out, |dir| spreadsheet::convert_cmd(input, dir, target))
}

/// soffice names its output after the input stem and refuses to overwrite, so
/// convert into a private temp dir and copy the result to a collision-free name
/// beside the source. Nothing the user already has is renamed or overwritten.
fn soffice_via_temp<F>(input: &Path, target: &str, dest: &Path, build: F) -> Result<Vec<PathBuf>>
where
    F: Fn(&Path) -> Vec<Vec<String>>,
{
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output").to_string();
    let tmp = std::env::temp_dir().join(format!("mandarine-soffice-{}-{}", std::process::id(), stem));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).context("creating soffice temp dir")?;
    let steps = build(&tmp);
    let result = (|| -> Result<Vec<PathBuf>> {
        engines::run_steps(&steps, None)?;
        let produced = tmp.join(format!("{stem}.{target}"));
        if !produced.exists() {
            anyhow::bail!("soffice produced no {} for {}", target, input.display());
        }
        // /tmp can live on another filesystem, so copy rather than rename.
        std::fs::copy(&produced, dest).with_context(|| format!("writing {}", dest.display()))?;
        Ok(vec![dest.to_path_buf()])
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn convert_text(input: &Path, target: &str, caps: &Capabilities) -> Result<Vec<PathBuf>> {
    if caps.has("ebook-convert") && !caps.has("pandoc") {
        return convert_with_ebook(input, target);
    }
    convert_with_pandoc(input, target, caps)
}

/// Pandoc bridge for markdown/HTML/EPUB/document conversions.
fn convert_with_pandoc(input: &Path, target: &str, caps: &Capabilities) -> Result<Vec<PathBuf>> {
    if !caps.has("pandoc") {
        anyhow::bail!("pandoc not found (install pandoc for {target} output)");
    }
    let out = naming::unique_sibling(input, target);
    let argv = vec![
        "pandoc".to_string(),
        "-o".to_string(),
        out.to_string_lossy().into_owned(),
        input.to_string_lossy().into_owned(),
    ];
    engines::run_steps(&[argv], None)?;
    Ok(vec![out])
}

/// Calibre bridge, used when pandoc is absent.
fn convert_with_ebook(input: &Path, target: &str) -> Result<Vec<PathBuf>> {
    if !engines::capabilities().has("ebook-convert") {
        anyhow::bail!("ebook-convert not found (install calibre for {target} output)");
    }
    let out = naming::unique_sibling(input, target);
    let argv = vec![
        "ebook-convert".to_string(),
        input.to_string_lossy().into_owned(),
        out.to_string_lossy().into_owned(),
    ];
    engines::run_steps(&[argv], None)?;
    Ok(vec![out])
}

fn convert_subtitle(input: &Path, target: &str) -> Result<Vec<PathBuf>> {
    let out = naming::unique_sibling(input, target);
    if target == "txt" {
        let text = std::fs::read_to_string(input)
            .with_context(|| format!("reading {}", input.display()))?;
        std::fs::write(&out, subtitle::to_text(&text))?;
        return Ok(vec![out]);
    }
    if !engines::capabilities().has("ffmpeg") {
        anyhow::bail!("ffmpeg not found");
    }
    engines::run_steps(&subtitle::convert_cmd(input, &out), None)?;
    Ok(vec![out])
}

/// Merge several PDFs into one sibling archive name; `files` are already ordered.
pub fn merge_pdfs(files: &[FileInfo], caps: &Capabilities) -> Result<Vec<PathBuf>> {
    if !caps.has("pdfunite") {
        anyhow::bail!("pdfunite not found");
    }
    if files.len() < 2 {
        anyhow::bail!("need at least two PDFs to merge");
    }
    let inputs: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
    let out = naming::unique_archive(&inputs, "pdf");
    engines::run_steps(&pdf::merge_cmd(&inputs, &out), None)?;
    Ok(vec![out])
}

/// Split a PDF into per-page files in a sibling folder `name_pages/`.
pub fn split_pdf(file: &FileInfo, caps: &Capabilities) -> Result<Vec<PathBuf>> {
    if !caps.has("pdfseparate") {
        anyhow::bail!("pdfseparate not found");
    }
    let dir = file.path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let stem = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("pages").to_string();
    let mut out_dir = dir.join(format!("{stem}_pages"));
    let mut n = 2;
    while out_dir.exists() {
        out_dir = dir.join(format!("{stem}_pages ({n})"));
        n += 1;
    }
    std::fs::create_dir_all(&out_dir)?;
    let pattern = out_dir.join(format!("{stem}-%d.pdf"));
    engines::run_steps(&pdf::split_cmd(&file.path, &pattern), None)?;
    let mut pages: Vec<PathBuf> = std::fs::read_dir(&out_dir)?.flatten().map(|e| e.path()).collect();
    pages.sort();
    Ok(pages)
}

/// Unpack one archive into a uniquely-named sibling folder.
pub fn extract_archive(input: &Path, caps: &Capabilities) -> Result<Vec<PathBuf>> {
    let out_dir = naming::unique_extract_dir(input);
    std::fs::create_dir_all(&out_dir)?;
    engines::run_steps(&archive::extract_cmd(input, &out_dir, caps), None)?;
    Ok(vec![out_dir])
}

/// Convert one archive container into another: extract to a temp dir, repack.
fn repack_archive(input: &Path, container: &str, caps: &Capabilities) -> Result<Vec<PathBuf>> {
    if !archive::extractable(input, caps) {
        anyhow::bail!("cannot read {}", input.display());
    }
    let tmp = std::env::temp_dir().join(format!(
        "mandarine-repack-{}-{}",
        std::process::id(),
        input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().replace('/', "_")
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).context("creating repack temp dir")?;
    let result = (|| -> Result<Vec<PathBuf>> {
        engines::run_steps(&archive::extract_cmd(input, &tmp, caps), None)?;
        let ext = match container {
            "zip" => "zip",
            "tar" => "tar",
            _ => "tar.gz",
        };
        let out = naming::unique_sibling(input, ext);
        engines::run_steps(&archive::repack_dir_cmd(&tmp, &out, container), Some(&tmp))?;
        Ok(vec![out])
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// Create an archive (zip | targz) from many files.
pub fn pack(files: &[FileInfo], target: &str, caps: &Capabilities) -> Result<Vec<PathBuf>> {
    let paths: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
    let out = naming::unique_archive(&paths, if target == "targz" { "tar.gz" } else { "zip" });
    let cwd = files
        .first()
        .and_then(|f| f.path.parent())
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let steps = match target {
        "targz" => archive::create_targz_cmd(&paths, &out, &cwd),
        _ => {
            if !caps.has("zip") {
                anyhow::bail!("zip not found");
            }
            archive::create_zip_cmd(&paths, &out, &cwd)
        }
    };
    engines::run_steps(&steps, Some(&cwd))?;
    Ok(vec![out])
}

/// Dispatch used by both the CLI and the wheel: handles special target kinds.
pub fn dispatch(files: &[FileInfo], target_id: &str, kind: Option<TargetKind>, cfg: &Config, caps: &Capabilities, progress: ProgressFn) -> Result<BatchResult> {
    match kind {
        Some(TargetKind::Merge) => {
            progress(0, 1, &files.first().map(|f| f.path.clone()).unwrap_or_default());
            let outputs = merge_pdfs(files, caps)?;
            Ok(BatchResult {
                outcomes: vec![JobOutcome { input: outputs[0].clone(), outputs, error: None }],
            })
        }
        Some(TargetKind::Split) => {
            progress(0, 1, &files.first().map(|f| f.path.clone()).unwrap_or_default());
            let outputs = split_pdf(&files[0], caps)?;
            Ok(BatchResult {
                outcomes: vec![JobOutcome { input: files[0].path.clone(), outputs, error: None }],
            })
        }
        Some(TargetKind::Zip) | Some(TargetKind::TarGz) => {
            progress(0, 1, &files.first().map(|f| f.path.clone()).unwrap_or_default());
            let outputs = pack(files, target_id, caps)?;
            Ok(BatchResult {
                outcomes: vec![JobOutcome { input: outputs[0].clone(), outputs, error: None }],
            })
        }
        Some(TargetKind::Extract) => Ok(run_each(files, progress, |f| extract_archive(&f.path, caps))),
        Some(TargetKind::Repack) => {
            Ok(run_each(files, progress, |f| repack_archive(&f.path, target_id, caps)))
        }
        Some(TargetKind::Custom) => run_custom(files, target_id, cfg, progress),
        Some(TargetKind::Tool) => run_tool(files, target_id, caps, progress),
        _ => Ok(run(files, target_id, cfg, caps, progress)),
    }
}

/// Per-file job runner for targets that bypass the category dispatch.
fn run_each<F>(files: &[FileInfo], progress: ProgressFn, job: F) -> BatchResult
where
    F: Fn(&FileInfo) -> Result<Vec<PathBuf>>,
{
    let mut outcomes = vec![];
    for (i, file) in files.iter().enumerate() {
        progress(i, files.len(), &file.path);
        let outcome = match job(file) {
            Ok(outputs) => JobOutcome { input: file.path.clone(), outputs, error: None },
            Err(e) => JobOutcome {
                input: file.path.clone(),
                outputs: vec![],
                error: Some(format!("{e:#}")),
            },
        };
        outcomes.push(outcome);
    }
    BatchResult { outcomes }
}

/// Run one parameter-light editing tool on each file, writing a suffixed sibling.
fn run_tool(files: &[FileInfo], tool_id: &str, caps: &Capabilities, progress: ProgressFn) -> Result<BatchResult> {
    let Some(tool) = tools::from_id(tool_id) else {
        anyhow::bail!("unknown editing tool {tool_id}");
    };
    let mut outcomes = vec![];
    for (i, file) in files.iter().enumerate() {
        progress(i, files.len(), &file.path);
        let outcome = (|| -> Result<Vec<PathBuf>> {
            if !tool.available(caps) || !tool.categories().contains(&file.category) {
                anyhow::bail!("{} is unavailable for {}", tool.label(), file.path.display());
            }
            let ext = file.path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext.is_empty() {
                anyhow::bail!("{} has no extension", file.path.display());
            }
            let dir = file.path.parent().unwrap_or(Path::new("."));
            let stem = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
            let out = naming::unique_named(dir, &format!("{stem}{}", tool.suffix()), ext);
            let steps = tools::steps(tool, &file.path, &out, file.category);
            engines::run_steps(&steps, None)?;
            Ok(vec![out])
        })();
        outcomes.push(match outcome {
            Ok(outputs) => JobOutcome { input: file.path.clone(), outputs, error: None },
            Err(e) => JobOutcome { input: file.path.clone(), outputs: vec![], error: Some(format!("{e:#}")) },
        });
    }
    Ok(BatchResult { outcomes })
}

fn run_custom(files: &[FileInfo], target_id: &str, cfg: &Config, progress: ProgressFn) -> Result<BatchResult> {
    let Some(def) = cfg.custom_targets.iter().find(|t| t.id == target_id) else {
        anyhow::bail!("unknown custom target {target_id}");
    };
    let mut outcomes = vec![];
    for (i, file) in files.iter().enumerate() {
        progress(i, files.len(), &file.path);
        let out = naming::unique_sibling(&file.path, &def.ext);
        let steps: Vec<Vec<String>> = def
            .steps
            .iter()
            .map(|step| {
                step.split_whitespace()
                    .map(|tok| tok.replace("{in}", &file.path.to_string_lossy()).replace("{out}", &out.to_string_lossy()))
                    .collect()
            })
            .collect();
        let outcome = match engines::run_steps(&steps, None) {
            Ok(()) => JobOutcome { input: file.path.clone(), outputs: vec![out], error: None },
            Err(e) => JobOutcome { input: file.path.clone(), outputs: vec![], error: Some(format!("{e:#}")) },
        };
        outcomes.push(outcome);
    }
    Ok(BatchResult { outcomes })
}
