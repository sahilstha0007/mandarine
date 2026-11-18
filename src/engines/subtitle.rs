use std::path::Path;

/// Subtitle format conversion through ffmpeg (srt <-> vtt).
pub fn convert_cmd(input: &Path, output: &Path) -> Vec<Vec<String>> {
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("srt").to_ascii_lowercase();
    let codec = match ext.as_str() {
        "vtt" => "webvtt",
        _ => "srt",
    };
    vec![vec![
        "ffmpeg".into(),
        "-y".into(),
        "-i".into(),
        input.to_string_lossy().into_owned(),
        "-c:s".into(),
        codec.into(),
        output.to_string_lossy().into_owned(),
    ]]
}

/// Plain-text transcript: cue indices and timecodes stripped, one line per cue.
/// Done in-process so `srt -> txt` needs no ffmpeg build with subtitle muxers.
pub fn to_text(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n");
    let mut out: Vec<String> = vec![];
    let mut current: Vec<&str> = vec![];
    for line in normalized.lines() {
        let trimmed = line.trim();
        let is_timestamp = trimmed.contains("-->");
        let is_index = !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit());
        if trimmed.is_empty() {
            if !current.is_empty() {
                out.push(current.join(" "));
                current.clear();
            }
            continue;
        }
        if is_timestamp || is_index || trimmed == "WEBVTT" || trimmed.starts_with("NOTE") {
            continue;
        }
        current.push(trimmed);
    }
    if !current.is_empty() {
        out.push(current.join(" "));
    }
    let mut text = out.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_conversion_argv() {
        let steps = convert_cmd(Path::new("/a/subs.srt"), Path::new("/a/subs.vtt"));
        assert_eq!(steps[0][0], "ffmpeg");
        assert!(steps[0].windows(2).any(|w| w == ["-c:s", "webvtt"]));
        let back = convert_cmd(Path::new("/a/subs.vtt"), Path::new("/a/subs.srt"));
        assert!(back[0].windows(2).any(|w| w == ["-c:s", "srt"]));
    }

    #[test]
    fn text_strips_cues_and_timestamps() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\nHello there\n\n2\n00:00:03,500 --> 00:00:05,000\nGeneral Kenobi\n";
        assert_eq!(to_text(srt), "Hello there\nGeneral Kenobi\n");
    }

    #[test]
    fn text_handles_vtt_header_and_wrapped_cues() {
        let vtt = "WEBVTT\n\n00:00.000 --> 00:02.000\nline one\nline two\n";
        assert_eq!(to_text(vtt), "line one line two\n");
    }
}
