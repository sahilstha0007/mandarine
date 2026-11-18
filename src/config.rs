use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub wheel: WheelConfig,
    pub notify: bool,
    pub audio_bitrate: String,
    pub video_crf: u32,
    pub pdf_dpi: u32,
    pub clipboard_fallback_secs: u64,
    /// Terminal used by the Terminal action: a binary name, or "auto" to pick
    /// the first of kitty/wezterm/foot/ghostty/alacritty on PATH.
    pub terminal: String,
    pub custom_targets: Vec<CustomTarget>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct WheelConfig {
    pub size: i32,
    /// Inner radius as a fraction of outer radius (0.0-1.0).
    pub inner_radius: f64,
    /// Gap between sectors, in degrees.
    pub sector_gap_deg: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct CustomTarget {
    pub id: String,
    pub label: String,
    /// Output extension used to name the result file.
    pub ext: String,
    /// Categories this target appears for: image, audio, video, pdf, office, archive, text.
    pub categories: Vec<String>,
    /// Command steps; `{in}` and `{out}` are replaced with quoted-free absolute paths.
    /// Example: ["magick", "{in}", "-quality", "95", "{out}"]
    pub steps: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            wheel: WheelConfig::default(),
            notify: true,
            audio_bitrate: "192k".into(),
            video_crf: 23,
            pdf_dpi: 300,
            clipboard_fallback_secs: 4,
            terminal: "auto".into(),
            custom_targets: vec![],
        }
    }
}

impl Default for WheelConfig {
    fn default() -> Self {
        Self { size: 440, inner_radius: 0.42, sector_gap_deg: 3.0 }
    }
}

pub fn config_path() -> PathBuf {
    dirs_config_dir().join("config.toml")
}

fn dirs_config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"))
        .join("mandarine")
}

/// Load the user config over compiled defaults; missing file or missing keys are fine.
pub fn load() -> Config {
    let path = config_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Config::default();
    };
    match toml::from_str::<Config>(&text) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("mandarine: ignoring malformed {}: {e}", path.display());
            Config::default()
        }
    }
}

/// Serialize the current config (including defaults) to TOML for `init-config`.
pub fn to_toml(cfg: &Config) -> String {
    toml::to_string_pretty(cfg).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_through_toml() {
        let cfg = Config::default();
        let text = to_toml(&cfg);
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn partial_overlay_keeps_defaults() {
        let parsed: Config = toml::from_str("notify = false\n").unwrap();
        assert!(!parsed.notify);
        assert_eq!(parsed.wheel.size, 440);
        assert_eq!(parsed.video_crf, 23);
        assert_eq!(parsed.terminal, "auto");
    }

    #[test]
    fn terminal_override_parses() {
        let parsed: Config = toml::from_str("terminal = \"wezterm\"\n").unwrap();
        assert_eq!(parsed.terminal, "wezterm");
    }

    #[test]
    fn custom_target_parses() {
        let parsed: Config = toml::from_str(r#"
            [[custom_targets]]
            id = "png90"
            label = "PNG q90"
            ext = "png"
            categories = ["image"]
            steps = ["magick", "{in}", "-quality", "90", "{out}"]
        "#)
            .unwrap();
        let t = &parsed.custom_targets[0];
        assert_eq!(t.id, "png90");
        assert!(t.steps.contains(&"{out}".to_string()));
    }
}
