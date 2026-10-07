use serde::Deserialize;
use std::path::Path;

/// User settings from `settings.json` next to the cache. Everything is off unless opted in.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    /// Also search the public Rekor v1 log by hash, which sends the hash to a second service.
    pub rekor: bool,
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let text = text.trim_start_matches('\u{feff}');
        serde_json::from_str(text).unwrap_or_default()
    }

    pub fn load(app_dir: &Path) -> Self {
        std::fs::read_to_string(app_dir.join("settings.json"))
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }
}

/// `%LOCALAPPDATA%\sigstore-shell`, or `None` when the variable is unset.
pub fn app_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .map(|v| Path::new(&v).join("sigstore-shell"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_off() {
        assert!(!Settings::parse("").rekor);
        assert!(!Settings::parse("{}").rekor);
        assert!(!Settings::parse("not json").rekor);
        assert!(!Settings::parse(r#"{"rekor": "yes"}"#).rekor);
    }

    #[test]
    fn rekor_opt_in_with_powershell_bom() {
        assert!(Settings::parse("\u{feff}{\"rekor\": true}").rekor);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert!(Settings::parse(r#"{"rekor": true, "future": 1}"#).rekor);
    }

    #[test]
    fn missing_file_is_default() {
        assert_eq!(
            Settings::load(Path::new("/nonexistent-dir")),
            Settings::default()
        );
    }
}
