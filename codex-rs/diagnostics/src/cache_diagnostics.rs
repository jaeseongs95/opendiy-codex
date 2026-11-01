// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! Immutable, process-local cache observation choices. This extension never
//! changes the shared logging settings understood by older installations.
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

static PROCESS_MODE: OnceLock<CacheMode> = OnceLock::new();

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub response: bool,
    pub wire: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CacheMode {
    pub response_requested: bool,
    pub wire_requested: bool,
    pub wire_producer_enabled: bool,
    pub response_enabled: bool,
    pub wire_enabled: bool,
    pub wire_disabled_reason: Option<&'static str>,
}

impl CacheMode {
    pub fn resolve(logging_enabled: bool, settings: Settings, explicit_wire: bool) -> Self {
        let response_enabled = logging_enabled && settings.response;
        let wire_enabled = response_enabled && settings.wire && explicit_wire;
        Self {
            response_requested: settings.response,
            wire_requested: settings.wire,
            // Preserve the existing explicit producer opt-in even when batch
            // output is off: another existing tracing layer may consume it.
            wire_producer_enabled: explicit_wire,
            response_enabled,
            wire_enabled,
            wire_disabled_reason: if wire_enabled {
                None
            } else if !logging_enabled {
                Some("logging_disabled")
            } else if !settings.response {
                Some("response_disabled")
            } else if !settings.wire {
                Some("wire_not_selected")
            } else {
                Some("producer_not_selected")
            },
        }
    }
}

fn explicit_wire() -> bool {
    std::env::var("APP_CACHE_WIRE_DIAGNOSTICS").as_deref() == Ok("1")
}

/// API-only consumers retain the legacy explicit opt-in. Normal CLI/server
/// startup initializes the same value before installing its tracing layers.
pub fn process_mode() -> CacheMode {
    *PROCESS_MODE.get_or_init(|| CacheMode::resolve(false, Settings::default(), explicit_wire()))
}

pub(crate) fn initialize(home: &Path, logging_enabled: bool) -> io::Result<CacheMode> {
    let settings = match fs::read_to_string(home.join("cli-cache-diagnostics.toml")) {
        Ok(text) => toml::from_str::<Settings>(&text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Settings::default(),
        Err(error) => return Err(error),
    };
    let mode = CacheMode::resolve(logging_enabled, settings, explicit_wire());
    match PROCESS_MODE.set(mode) {
        Ok(()) => Ok(mode),
        Err(_) if PROCESS_MODE.get() == Some(&mode) => Ok(mode),
        Err(_) => Err(io::Error::other(
            "cache diagnostics were initialized earlier with a different mode",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_wire_requires_every_gate_but_explicit_producer_is_preserved() {
        for logging in [false, true] {
            for response in [false, true] {
                for wire in [false, true] {
                    for explicit in [false, true] {
                        let mode =
                            CacheMode::resolve(logging, Settings { response, wire }, explicit);
                        assert_eq!(mode.response_enabled, logging && response);
                        assert_eq!(mode.wire_enabled, logging && response && wire && explicit);
                        assert_eq!(mode.wire_producer_enabled, explicit);
                        assert_eq!(mode.wire_disabled_reason.is_none(), mode.wire_enabled);
                    }
                }
            }
        }
    }

    #[test]
    fn extension_is_strict_and_shared_settings_remain_unchanged() {
        assert_eq!(toml::from_str::<Settings>("").unwrap(), Settings::default());
        assert!(toml::from_str::<Settings>("response = 1").is_err());
        assert!(toml::from_str::<Settings>("prompt_cache = true").is_err());
        assert!(toml::from_str::<crate::batch_log::Settings>("prompt_cache = true").is_err());
        assert!(toml::from_str::<crate::batch_log::Settings>("enabled = true").is_ok());
    }

    #[test]
    fn startup_mode_is_frozen_in_an_isolated_process() {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "cache_diagnostics::tests::frozen_mode_child",
                "--ignored",
                "--test-threads=1",
            ])
            .env("APP_CACHE_WIRE_DIAGNOSTICS", "1");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        assert!(command.status().unwrap().success());
    }

    #[test]
    #[ignore = "isolated process for immutable startup state"]
    fn frozen_mode_child() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("cli-cache-diagnostics.toml");
        fs::write(&settings, "response = true\nwire = true\n").unwrap();
        let before = initialize(root.path(), true).unwrap();
        assert!(before.response_enabled && before.wire_enabled && before.wire_producer_enabled);
        fs::write(&settings, "response = false\nwire = false\n").unwrap();
        assert_eq!(process_mode(), before);
        assert!(initialize(root.path(), true).is_err());
        assert_eq!(process_mode(), before);
    }
}
