// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! Client display preference read only from the base user config, never merged layers.

use std::io;
use std::path::Path;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug, Default, Copy, Clone, PartialEq, Eq, JsonSchema)]
pub enum UiLanguage {
    #[serde(rename = "ko")]
    Korean,
    #[default]
    #[serde(rename = "en")]
    English,
}

impl UiLanguage {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        match value {
            "ko" => Ok(Self::Korean),
            "en" => Ok(Self::English),
            _ => Err("UI language must be ko or en"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Korean => "ko",
            Self::English => "en",
        }
    }
}

/// Read the local display preference before CLI help or any merged/server config is loaded.
/// Profiles, projects, named user layers and remote thread settings are intentionally absent.
pub fn read_user_ui_language(codex_home: &Path) -> io::Result<UiLanguage> {
    #[derive(Deserialize, Default)]
    struct Preference {
        ui_language: Option<UiLanguage>,
    }
    let text = match std::fs::read_to_string(codex_home.join(crate::CONFIG_TOML_FILE)) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(UiLanguage::default()),
        Err(error) => return Err(error),
    };
    let preference: Preference =
        toml::from_str(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(preference.ui_language.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_user_preference_is_english_and_invalid_values_are_rejected() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            read_user_ui_language(home.path()).unwrap(),
            UiLanguage::English
        );
        let path = home.path().join(crate::CONFIG_TOML_FILE);
        std::fs::write(&path, "model = 'untouched'\n").unwrap();
        assert_eq!(
            read_user_ui_language(home.path()).unwrap(),
            UiLanguage::English
        );
        for invalid in ["'kr'", "'KO'", "'ko-KR'", "''", "7", "true"] {
            std::fs::write(&path, format!("ui_language = {invalid}\n")).unwrap();
            assert_eq!(
                read_user_ui_language(home.path()).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        for invalid in ["kr", "KO", "ko-KR", "", "ko en"] {
            assert!(UiLanguage::parse(invalid).is_err());
        }
    }

    #[test]
    fn only_base_user_file_controls_language_even_with_project_and_profile_values() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join(".codex")).unwrap();
        std::fs::write(
            project.path().join(".codex/config.toml"),
            "ui_language = 'ko'\n",
        )
        .unwrap();
        std::fs::write(
            home.path().join(crate::CONFIG_TOML_FILE),
            "ui_language = 'en'\nprofile = 'korean'\n[profiles.korean]\nui_language = 'ko'\n",
        )
        .unwrap();
        assert_eq!(
            read_user_ui_language(home.path()).unwrap(),
            UiLanguage::English
        );
        // Even an invalid project value must not enter this global-only reader.
        std::fs::write(
            project.path().join(".codex/config.toml"),
            "ui_language = 7\n",
        )
        .unwrap();
        assert_eq!(
            read_user_ui_language(home.path()).unwrap(),
            UiLanguage::English
        );
    }
}
