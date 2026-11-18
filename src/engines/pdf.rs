use crate::config::Config;
use std::path::Path;

/// PDF -> raster images via pdftoppm. The stem is a prefix: poppler writes
/// `stem-1.png`, `stem-2.png`, ... (single-page documents still get `-1`).
pub fn to_image_cmd(input: &Path, stem_prefix: &Path, ext: &str, cfg: &Config) -> Vec<Vec<String>> {
    let format_flag = if ext == "jpg" || ext == "jpeg" { "-jpeg" } else { "-png" };
    vec![vec![
        "pdftoppm".into(),
        format_flag.into(),
        "-r".into(),
        cfg.pdf_dpi.to_string(),
        input.to_string_lossy().into_owned(),
        stem_prefix.to_string_lossy().into_owned(),
    ]]
}

/// Rename `prefix-1.png`-style output to `prefix.png` when there is exactly one page.
/// Returns the produced file paths either way. The prefix is matched by full
/// file name (`sample.mandarine`), not file_stem, since it contains dots.
pub fn collect_page_outputs(stem_prefix: &Path, ext: &str) -> Vec<std::path::PathBuf> {
    let dir = stem_prefix.parent().unwrap_or(Path::new("."));
    let stem = stem_prefix.file_name().and_then(|s| s.to_str()).unwrap_or("page");
    let want_ext = if ext == "jpg" || ext == "jpeg" { "jpg" } else { "png" };
    let mut pages: Vec<std::path::PathBuf> = vec![];
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(idx) = name.rfind('-') {
                let (base, num_ext) = name.split_at(idx);
                if base == stem {
                    let num_ext = &num_ext[1..]; // strip '-'
                    let (num, file_ext) = match num_ext.rfind('.') {
                        Some(d) => (&num_ext[..d], &num_ext[d + 1..]),
                        None => continue,
                    };
                    if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) && file_ext.eq_ignore_ascii_case(want_ext) {
                        pages.push(entry.path());
                    }
                }
            }
        }
    }
    pages.sort();
    // Single page: drop the "-1" suffix so it feels like a plain conversion.
    if pages.len() == 1 {
        let cleaned = dir.join(format!("{stem}.{want_ext}"));
        if !cleaned.exists() {
            if std::fs::rename(&pages[0], &cleaned).is_ok() {
                pages[0] = cleaned;
            }
        }
    }
    pages
}

pub fn to_txt_cmd(input: &Path, output: &Path) -> Vec<Vec<String>> {
    vec![vec![
        "pdftotext".into(),
        input.to_string_lossy().into_owned(),
        output.to_string_lossy().into_owned(),
    ]]
}

/// PDF -> HTML through poppler's pdftohtml. Writes `<prefix>.html`.
pub fn to_html_cmd(input: &Path, prefix: &Path) -> Vec<Vec<String>> {
    vec![vec![
        "pdftohtml".into(),
        "-q".into(),
        "-noframes".into(),
        "-s".into(),
        input.to_string_lossy().into_owned(),
        prefix.to_string_lossy().into_owned(),
    ]]
}

pub fn merge_cmd(inputs: &[std::path::PathBuf], output: &Path) -> Vec<Vec<String>> {
    let mut argv = vec!["pdfunite".to_string()];
    for p in inputs {
        argv.push(p.to_string_lossy().into_owned());
    }
    argv.push(output.to_string_lossy().into_owned());
    vec![argv]
}

pub fn split_cmd(input: &Path, pattern: &Path) -> Vec<Vec<String>> {
    vec![vec![
        "pdfseparate".into(),
        input.to_string_lossy().into_owned(),
        pattern.to_string_lossy().into_owned(),
    ]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn to_image_uses_dpi() {
        let cfg = Config { pdf_dpi: 150, ..Config::default() };
        let steps = to_image_cmd(Path::new("/a/b.pdf"), Path::new("/a/b"), "png", &cfg);
        assert_eq!(steps[0], vec!["pdftoppm", "-png", "-r", "150", "/a/b.pdf", "/a/b"]);
    }

    #[test]
    fn merge_lists_all_inputs() {
        let steps = merge_cmd(
            &[PathBuf::from("/a/1.pdf"), PathBuf::from("/a/2.pdf")],
            Path::new("/a/merged.pdf"),
        );
        assert_eq!(steps[0], vec!["pdfunite", "/a/1.pdf", "/a/2.pdf", "/a/merged.pdf"]);
    }

    #[test]
    fn html_uses_noframes_single_file() {
        let steps = to_html_cmd(Path::new("/a/b.pdf"), Path::new("/a/b.mandarine"));
        assert_eq!(steps[0], vec!["pdftohtml", "-q", "-noframes", "-s", "/a/b.pdf", "/a/b.mandarine"]);
    }

    #[test]
    fn split_pattern() {
        let steps = split_cmd(Path::new("/a/b.pdf"), Path::new("/a/b-%d.pdf"));
        assert_eq!(steps[0], vec!["pdfseparate", "/a/b.pdf", "/a/b-%d.pdf"]);
    }

    #[test]
    fn collect_matches_dotted_prefixes_and_renames_single_page() {
        let tmp = std::env::temp_dir().join(format!("mandarine-collect-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let page = tmp.join("doc.mandarine-1.png");
        std::fs::write(&page, b"x").unwrap();
        let pages = collect_page_outputs(&tmp.join("doc.mandarine"), "png");
        assert_eq!(pages, vec![tmp.join("doc.mandarine.png")]);
        assert!(!page.exists());
        // Two pages: both found, none renamed.
        let tmp2 = std::env::temp_dir().join(format!("mandarine-collect2-{}", std::process::id()));
        std::fs::create_dir_all(&tmp2).unwrap();
        for n in ["doc.mandarine-1.png", "doc.mandarine-2.png"] {
            std::fs::write(tmp2.join(n), b"x").unwrap();
        }
        let two = collect_page_outputs(&tmp2.join("doc.mandarine"), "png");
        assert_eq!(two.len(), 2);
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::remove_dir_all(&tmp2);
    }
}
