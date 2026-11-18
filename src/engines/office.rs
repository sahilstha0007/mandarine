use std::path::{Path, PathBuf};

/// soffice --headless --convert-to <ext> writes the result into `out_dir` with
/// the source stem, so we convert in place and report where it landed.
pub fn convert_cmd(input: &Path, out_dir: &Path, ext: &str) -> Vec<Vec<String>> {
    vec![vec![
        "soffice".into(),
        "--headless".into(),
        "--convert-to".into(),
        ext.into(),
        "--outdir".into(),
        out_dir.to_string_lossy().into_owned(),
        input.to_string_lossy().into_owned(),
    ]]
}

pub fn to_pdf_cmd(input: &Path, out_dir: &Path) -> Vec<Vec<String>> {
    convert_cmd(input, out_dir, "pdf")
}

/// Where soffice's output will be for `input` (`stem.ext` in `out_dir`).
pub fn expected_output(input: &Path, out_dir: &Path, ext: &str) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    out_dir.join(format!("{stem}.{ext}"))
}

/// A warm profile lets the first real conversion skip LibreOffice's slow
/// first-run bootstrap. Run in a background thread at GUI startup.
pub fn prewarm_profile() {
    let _ = std::process::Command::new("soffice")
        .arg("--headless")
        .arg("--terminate_after_init")
        .output();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_command_shape() {
        let steps = convert_cmd(Path::new("/a/report.docx"), Path::new("/a"), "pdf");
        assert_eq!(
            steps[0],
            vec!["soffice", "--headless", "--convert-to", "pdf", "--outdir", "/a", "/a/report.docx"]
        );
    }

    #[test]
    fn html_and_odt_targets() {
        let html = convert_cmd(Path::new("/a/report.docx"), Path::new("/a"), "html");
        assert!(html[0].windows(2).any(|w| w == ["--convert-to", "html"]));
        let odt = convert_cmd(Path::new("/a/report.docx"), Path::new("/a"), "odt");
        assert!(odt[0].windows(2).any(|w| w == ["--convert-to", "odt"]));
    }

    #[test]
    fn expected_output_uses_stem_and_extension() {
        assert_eq!(
            expected_output(&PathBuf::from("/a/report.docx"), Path::new("/a"), "pdf"),
            PathBuf::from("/a/report.pdf")
        );
        assert_eq!(
            expected_output(&PathBuf::from("/a/report.docx"), Path::new("/a"), "txt"),
            PathBuf::from("/a/report.txt")
        );
    }
}
