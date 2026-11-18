use crate::config::Config;
use std::path::Path;

/// Audio codec arguments per container extension.
fn audio_args(ext: &str, cfg: &Config) -> Vec<String> {
    let bitrate = cfg.audio_bitrate.clone();
    match ext {
        "mp3" => flagset(&["-vn", "-c:a", "libmp3lame", "-b:a", &bitrate]),
        "m4a" => flagset(&["-vn", "-c:a", "aac", "-b:a", &bitrate]),
        "wav" => flagset(&["-vn", "-c:a", "pcm_s16le"]),
        "flac" => flagset(&["-vn", "-c:a", "flac"]),
        "ogg" => flagset(&["-vn", "-c:a", "libvorbis", "-q:a", "6"]),
        "opus" => flagset(&["-vn", "-c:a", "libopus", "-b:a", "160k"]),
        "aiff" | "aif" => flagset(&["-vn", "-c:a", "pcm_s16be"]),
        "wma" => flagset(&["-vn", "-c:a", "wmav2", "-b:a", &bitrate]),
        _ => flagset(&["-vn"]),
    }
}

/// Video codec arguments per container extension.
fn video_args(ext: &str, cfg: &Config) -> Vec<String> {
    let crf = cfg.video_crf.to_string();
    match ext {
        "mp4" | "mov" => {
            let mut v = flagset(&["-c:v", "libx264", "-crf", &crf, "-preset", "medium", "-c:a", "aac", "-b:a", &cfg.audio_bitrate]);
            if ext == "mp4" {
                v.extend(["-movflags".into(), "+faststart".into()]);
            }
            v
        }
        "mkv" => flagset(&["-c:v", "libx264", "-crf", &crf, "-preset", "medium", "-c:a", "aac", "-b:a", &cfg.audio_bitrate]),
        "webm" => flagset(&["-c:v", "libvpx-vp9", "-crf", "32", "-b:v", "0", "-c:a", "libopus"]),
        "wmv" => flagset(&["-c:v", "wmv2", "-b:v", "4M", "-c:a", "wmav2", "-b:a", &cfg.audio_bitrate]),
        _ => flagset(&["-c:v", "libx264", "-crf", &crf]),
    }
}

fn flagset(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// Build ffmpeg argv for `input` -> `output`. If the output extension is an
/// audio format the video stream is dropped, which also covers video->audio.
pub fn convert_cmd(input: &Path, output: &Path, cfg: &Config) -> Vec<Vec<String>> {
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let args = if matches!(ext.as_str(), "mp3" | "m4a" | "wav" | "flac" | "ogg" | "opus" | "aac" | "aiff" | "wma") {
        audio_args(&ext, cfg)
    } else if ext == "gif" {
        // Two-in-one palette pipeline, single pass.
        return vec![vec![
            "ffmpeg".into(),
            "-y".into(),
            "-i".into(),
            input.to_string_lossy().into_owned(),
            "-vf".into(),
            "fps=12,scale=640:-1:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse".into(),
            "-loop".into(),
            "0".into(),
            output.to_string_lossy().into_owned(),
        ]];
    } else {
        video_args(&ext, cfg)
    };
    let mut argv = vec!["ffmpeg".to_string(), "-y".to_string(), "-i".to_string()];
    argv.push(input.to_string_lossy().into_owned());
    argv.extend(args);
    argv.push(output.to_string_lossy().into_owned());
    vec![argv]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_mp3_args() {
        let cfg = Config::default();
        let steps = convert_cmd(Path::new("/a/b.wav"), Path::new("/a/b.mp3"), &cfg);
        assert_eq!(steps[0], vec![
            "ffmpeg", "-y", "-i", "/a/b.wav", "-vn", "-c:a", "libmp3lame", "-b:a", "192k", "/a/b.mp3"
        ]);
    }

    #[test]
    fn video_to_audio_drops_stream() {
        let cfg = Config::default();
        let steps = convert_cmd(Path::new("/a/b.mp4"), Path::new("/a/b.mp3"), &cfg);
        assert!(steps[0].contains(&"-vn".to_string()));
        assert!(steps[0].windows(2).any(|w| w == ["-i".to_string(), "/a/b.mp4".to_string()]));
    }

    #[test]
    fn mp4_gets_faststart() {
        let cfg = Config::default();
        let steps = convert_cmd(Path::new("/a/b.mkv"), Path::new("/a/b.mp4"), &cfg);
        let joined = steps[0].join(" ");
        assert!(joined.contains("-movflags +faststart"));
        assert!(joined.contains("-crf 23"));
    }

    #[test]
    fn webm_uses_vp9() {
        let cfg = Config::default();
        let steps = convert_cmd(Path::new("/a/b.mp4"), Path::new("/a/b.webm"), &cfg);
        assert!(steps[0].contains(&"libvpx-vp9".to_string()));
    }

    #[test]
    fn parity_audio_and_video_args() {
        let cfg = Config::default();
        let aiff = convert_cmd(Path::new("/a/b.wav"), Path::new("/a/b.aiff"), &cfg);
        assert!(aiff[0].contains(&"pcm_s16be".to_string()));
        let wma = convert_cmd(Path::new("/a/b.mp3"), Path::new("/a/b.wma"), &cfg);
        assert!(wma[0].contains(&"wmav2".to_string()));
        let wmv = convert_cmd(Path::new("/a/b.mp4"), Path::new("/a/b.wmv"), &cfg);
        assert!(wmv[0].contains(&"wmv2".to_string()));
        let avi = convert_cmd(Path::new("/a/b.mp4"), Path::new("/a/b.avi"), &cfg);
        assert!(avi[0].contains(&"libx264".to_string()));
    }

    #[test]
    fn gif_uses_palette_pipeline() {
        let cfg = Config::default();
        let steps = convert_cmd(Path::new("/a/b.mp4"), Path::new("/a/b.gif"), &cfg);
        assert!(steps[0].iter().any(|a| a.contains("palettegen")));
        assert!(steps[0].contains(&"-loop".to_string()));
    }
}
