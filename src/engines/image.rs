use std::path::Path;

/// Build the ImageMagick argv converting `input` to `output` (extension decides format).
pub fn convert_cmd(input: &Path, output: &Path) -> Vec<Vec<String>> {
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let mut argv = vec!["magick".to_string(), input.to_string_lossy().into_owned()];
    match ext.as_str() {
        "jpg" | "jpeg" => argv.extend(["-quality".into(), "90".into()]),
        "webp" => argv.extend(["-quality".into(), "85".into()]),
        "avif" => argv.extend(["-quality".into(), "60".into()]),
        _ => {}
    }
    argv.push(output.to_string_lossy().into_owned());
    vec![argv]
}

/// Raster image -> PDF at a print-friendly density.
pub fn to_pdf_cmd(input: &Path, output: &Path) -> Vec<Vec<String>> {
    vec![vec![
        "magick".into(),
        input.to_string_lossy().into_owned(),
        "-units".into(),
        "PixelsPerInch".into(),
        "-density".into(),
        "300".into(),
        output.to_string_lossy().into_owned(),
    ]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn plain_convert_has_no_quality_flag() {
        let steps = convert_cmd(Path::new("/a/b.png"), Path::new("/a/b.tiff"));
        assert_eq!(steps[0], vec!["magick", "/a/b.png", "/a/b.tiff"]);
    }

    #[test]
    fn quality_per_target() {
        assert_eq!(
            convert_cmd(Path::new("/a/b.png"), Path::new("/a/b.webp"))[0],
            vec!["magick", "/a/b.png", "-quality", "85", "/a/b.webp"]
        );
        assert_eq!(
            convert_cmd(Path::new("/a/b.png"), Path::new("/a/b.avif"))[0],
            vec!["magick", "/a/b.png", "-quality", "60", "/a/b.avif"]
        );
        assert_eq!(
            convert_cmd(Path::new("/a/b.png"), Path::new("/a/b.jpg"))[0],
            vec!["magick", "/a/b.png", "-quality", "90", "/a/b.jpg"]
        );
    }

    #[test]
    fn pdf_output_carries_density() {
        let steps = to_pdf_cmd(PathBuf::from("/a/b.png").as_path(), PathBuf::from("/a/b.pdf").as_path());
        assert_eq!(steps[0], vec!["magick", "/a/b.png", "-units", "PixelsPerInch", "-density", "300", "/a/b.pdf"]);
    }
}
