use crate::engines::Capabilities;
use std::path::Path;

/// Can we extract an archive at this path? Handles compound names like
/// `photo.tar.gz`, whose `Path::extension()` is only "gz".
pub fn extractable(path: &Path, caps: &Capabilities) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) else {
        return false;
    };
    let is_tar_compressed = matches!(ext.as_str(), "gz" | "bz2" | "xz" | "zst")
        && path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase().ends_with(".tar"))
            .unwrap_or(false);
    match ext.as_str() {
        "zip" => caps.has("7z") || caps.has("unzip"),
        "7z" => caps.has("7z"),
        "rar" => caps.has("unrar") || caps.has("7z"),
        "tar" => caps.has("tar"),
        _ if is_tar_compressed => caps.has("tar"),
        _ => false,
    }
}

/// One argv that handles every extractable format we support.
pub fn extract_cmd(input: &Path, out_dir: &Path, caps: &Capabilities) -> Vec<Vec<String>> {
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let out = out_dir.to_string_lossy().into_owned();
    let src = input.to_string_lossy().into_owned();
    let seven = caps.has("7z");
    match ext.as_str() {
        "rar" if caps.has("unrar") => vec![vec!["unrar".into(), "x".into(), "-y".into(), src, out.clone() + "/"]],
        "rar" if seven => vec![vec!["7z".into(), "x".into(), "-y".into(), format!("-o{out}"), src]],
        "zip" if seven => vec![vec!["7z".into(), "x".into(), "-y".into(), format!("-o{out}"), src]],
        "zip" if caps.has("unzip") => vec![vec!["unzip".into(), "-o".into(), src, "-d".into(), out]],
        "7z" if seven => vec![vec!["7z".into(), "x".into(), "-y".into(), format!("-o{out}"), src]],
        // tar, tar.gz/bz2/xz/zst — GNU tar autodetects compression.
        _ => vec![vec!["tar".into(), "-xf".into(), src, "-C".into(), out]],
    }
}

/// Members for `files` when the caller runs the command with `cwd` set to the
/// shared parent: bare names, so archives stay relative. Files from elsewhere
/// fall back to absolute paths.
fn member_names(files: &[std::path::PathBuf], cwd: &Path) -> Vec<String> {
    let same_dir = files.iter().all(|f| f.parent() == Some(cwd));
    files
        .iter()
        .map(|f| {
            if same_dir {
                f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            } else {
                f.to_string_lossy().into_owned()
            }
        })
        .collect()
}

/// Create a zip or tar.gz from the given files (directories included).
pub fn create_zip_cmd(files: &[std::path::PathBuf], output: &Path, cwd: &Path) -> Vec<Vec<String>> {
    let mut argv = vec!["zip".to_string(), "-r".to_string(), "-9".to_string(), "-q".to_string(), output.to_string_lossy().into_owned()];
    argv.extend(member_names(files, cwd));
    vec![argv]
}

pub fn create_targz_cmd(files: &[std::path::PathBuf], output: &Path, cwd: &Path) -> Vec<Vec<String>> {
    let mut argv = vec!["tar".to_string(), "-czf".to_string(), output.to_string_lossy().into_owned()];
    argv.extend(member_names(files, cwd));
    vec![argv]
}

/// Repack a directory's contents (used after extracting an archive).
pub fn repack_dir_cmd(dir: &Path, output: &Path, container: &str) -> Vec<Vec<String>> {
    let out = output.to_string_lossy().into_owned();
    match container {
        "zip" => vec![vec!["zip".into(), "-r".into(), "-9".into(), "-q".into(), out, ".".into()]],
        "tar" => vec![vec!["tar".into(), "-cf".into(), out, "-C".into(), dir.to_string_lossy().into_owned(), ".".into()]],
        _ => vec![vec!["tar".into(), "-czf".into(), out, "-C".into(), dir.to_string_lossy().into_owned(), ".".into()]],
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn caps(present: &[&'static str]) -> Capabilities {
        let mut c = Capabilities::default();
        for p in present {
            c.binaries.insert(*p, true);
        }
        c
    }

    #[test]
    fn extractable_matrix() {
        let full = caps(&["tar", "7z", "unrar", "unzip"]);
        for name in ["a.zip", "a.7z", "a.rar", "a.tar", "a.tar.gz", "a.tar.xz", "a.tar.zst", "a.tar.bz2"] {
            assert!(extractable(Path::new(&format!("/d/{name}")), &full), "{name}");
        }
        // A bare .gz/.xz is not an archive we claim.
        assert!(!extractable(Path::new("/d/data.gz"), &full));
        assert!(!extractable(Path::new("/d/noext"), &full));
        // Engines gate: no tar, no tar.gz; no 7z/unzip, no zip.
        assert!(!extractable(Path::new("/d/a.tar.gz"), &caps(&[])));
        assert!(!extractable(Path::new("/d/a.zip"), &caps(&["tar"])));
        assert!(!extractable(Path::new("/d/a.7z"), &caps(&["tar"])));
    }

    #[test]
    fn prefers_unrar_for_rar() {
        let c = caps(&["unrar"]);
        let steps = extract_cmd(Path::new("/a/x.rar"), Path::new("/a/x_extracted"), &c);
        assert_eq!(steps[0][0], "unrar");
        // 7z handles 7z archives
        let steps7 = extract_cmd(Path::new("/a/x.7z"), Path::new("/a/x_extracted"), &caps(&["7z"]));
        assert_eq!(steps7[0][0], "7z");
    }

    #[test]
    fn repack_containers() {
        let dir = Path::new("/tmp/x_extracted");
        let zip = repack_dir_cmd(dir, Path::new("/x.zip"), "zip");
        assert_eq!(zip[0][0..2], ["zip", "-r"]);
        let tar = repack_dir_cmd(dir, Path::new("/x.tar"), "tar");
        assert!(tar[0].windows(2).any(|w| w == ["-C", "/tmp/x_extracted"]));
        let gz = repack_dir_cmd(dir, Path::new("/x.tar.gz"), "gz");
        assert_eq!(gz[0][1], "-czf");
    }

    #[test]
    fn zip_and_tar_create_shapes() {
        let files = vec![std::path::PathBuf::from("/a/1.png"), std::path::PathBuf::from("/a/2.png")];
        let zip = create_zip_cmd(&files, Path::new("/a/archive.zip"), Path::new("/a"));
        assert_eq!(zip[0], vec!["zip", "-r", "-9", "-q", "/a/archive.zip", "1.png", "2.png"]);
        let tar = create_targz_cmd(&files, Path::new("/a/archive.tar.gz"), Path::new("/a"));
        assert_eq!(tar[0], vec!["tar", "-czf", "/a/archive.tar.gz", "1.png", "2.png"]);
        // A folder selection keeps its name and recurses.
        let dirs = vec![std::path::PathBuf::from("/a/photos")];
        let zip_dir = create_zip_cmd(&dirs, Path::new("/a/photos.zip"), Path::new("/a"));
        assert_eq!(zip_dir[0], vec!["zip", "-r", "-9", "-q", "/a/photos.zip", "photos"]);
    }
}
