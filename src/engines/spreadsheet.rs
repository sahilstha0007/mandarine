use std::path::{Path, PathBuf};

/// LibreOffice's `--convert-to` filter token for a target extension.
pub fn filter_for(ext: &str) -> String {
    match ext {
        "csv" => "csv:Text - txt - csv (StarCalc)".to_string(),
        "tsv" => "tsv:Text - txt - csv (StarCalc):9".to_string(),
        other => other.to_string(),
    }
}

/// soffice writes the result into `out_dir` with the source stem and the target
/// extension. `--convert-to` takes `filter` or `Extension:Filter`.
pub fn convert_cmd(input: &Path, out_dir: &Path, ext: &str) -> Vec<Vec<String>> {
    vec![vec![
        "soffice".into(),
        "--headless".into(),
        "--convert-to".into(),
        filter_for(ext),
        "--outdir".into(),
        out_dir.to_string_lossy().into_owned(),
        input.to_string_lossy().into_owned(),
    ]]
}

/// Where soffice will write `input` converted to `ext`.
pub fn expected_output(input: &Path, out_dir: &Path, ext: &str) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    out_dir.join(format!("{stem}.{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_uses_calc_filter() {
        let steps = convert_cmd(Path::new("/a/books.xlsx"), Path::new("/a"), "csv");
        assert_eq!(steps[0][0], "soffice");
        assert!(steps[0].contains(&"csv:Text - txt - csv (StarCalc)".to_string()));
    }

    #[test]
    fn expected_output_swaps_extension() {
        assert_eq!(
            expected_output(Path::new("/a/books.xlsx"), Path::new("/a"), "ods"),
            PathBuf::from("/a/books.ods")
        );
    }
}
