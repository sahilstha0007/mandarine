use std::path::{Path, PathBuf};

/// A sibling of `src` with a new extension, made unique with " (n)" suffixes.
pub fn unique_sibling(src: &Path, new_ext: &str) -> PathBuf {
    let dir = src.parent().unwrap_or(Path::new("."));
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    unique_in(dir, stem, new_ext)
}

/// A uniquely-named sibling directory for extraction (e.g. `photos_extracted`).
pub fn unique_extract_dir(src: &Path) -> PathBuf {
    let dir = src.parent().unwrap_or(Path::new("."));
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("archive");
    let mut candidate = dir.join(format!("{stem}_extracted"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}_extracted ({n})"));
        n += 1;
    }
    candidate
}

/// A new file `name.ext` in `dir`, made unique with " (n)" suffixes.
pub fn unique_named(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    unique_in(dir, stem, ext)
}

/// A new archive `stem.zip`/`stem.tar.gz` beside the first source file.
pub fn unique_archive(files: &[PathBuf], ext: &str) -> PathBuf {
    let dir = files.first()
        .and_then(|f| f.parent())
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let stem = match files.len() {
        1 => files[0].file_stem().and_then(|s| s.to_str()).unwrap_or("archive").to_string(),
        _ => "archive".to_string(),
    };
    unique_in(&dir, &stem, ext)
}

fn unique_in(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avoids_collisions_in_missing_dir() {
        // /nonexistent-dir-x has no files, so first candidate is used directly.
        let p = unique_sibling(Path::new("/tmp/definitely-missing-dir-mandarine/photo.jpg"), "png");
        assert_eq!(p, PathBuf::from("/tmp/definitely-missing-dir-mandarine/photo.png"));
    }

    #[test]
    fn numbered_suffixes_on_collision() {
        let tmp = std::env::temp_dir().join(format!("mandarine-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("doc.pdf");
        std::fs::write(&src, b"x").unwrap();
        let first = unique_sibling(&src, "png");
        assert_eq!(first, tmp.join("doc.png"));
        std::fs::write(&first, b"x").unwrap();
        let second = unique_sibling(&src, "png");
        assert_eq!(second, tmp.join("doc (2).png"));
        std::fs::write(&second, b"x").unwrap();
        assert_eq!(unique_sibling(&src, "png"), tmp.join("doc (3).png"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn extract_dir_suffixes() {
        let tmp = std::env::temp_dir().join(format!("mandarine-test-ex-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("data_extracted")).unwrap();
        let got = unique_extract_dir(&tmp.join("data.zip"));
        assert_eq!(got, tmp.join("data_extracted (2)"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
