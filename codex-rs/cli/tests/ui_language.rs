use std::path::Path;

use anyhow::Result;
use codex_config::ui_language::UiLanguage;
use codex_core::config::edit::ConfigEditsBuilder;
use predicates::str::contains;
use tempfile::TempDir;

fn command(home: &Path, project: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = std::process::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd.env("CODEX_HOME", home)
        .env_remove("CODEX_UI_LANGUAGE")
        .current_dir(project)
        .arg("--help");
    Ok(assert_cmd::Command::from_std(cmd))
}

#[test]
fn saved_global_language_controls_help_across_new_processes_and_ignores_project_overrides()
-> Result<()> {
    let home = TempDir::new()?;
    let project = TempDir::new()?;
    std::fs::create_dir(project.path().join(".codex"))?;
    std::fs::write(
        project.path().join(".codex/config.toml"),
        "ui_language = 'ko'\n",
    )?;
    command(home.path(), project.path())?
        .assert()
        .success()
        .stdout(contains("Usage:"));
    for (language, heading) in [
        (UiLanguage::Korean, "사용법:"),
        (UiLanguage::English, "Usage:"),
    ] {
        ConfigEditsBuilder::new(home.path())
            .set_ui_language(language)
            .apply_blocking()?;
        let saved = std::fs::read(home.path().join("config.toml"))?;
        for _ in 0..2 {
            command(home.path(), project.path())?
                .assert()
                .success()
                .stdout(contains(heading));
        }
        // A CLI/session override must not become a display preference either.
        command(home.path(), project.path())?
            .args(["-c", "ui_language='ko'"])
            .assert()
            .success()
            .stdout(contains(heading));
        assert_eq!(std::fs::read(home.path().join("config.toml"))?, saved);
    }
    Ok(())
}

#[test]
fn language_environment_override_is_explicit_temporary_and_validated() -> Result<()> {
    let home = TempDir::new()?;
    let project = TempDir::new()?;
    ConfigEditsBuilder::new(home.path())
        .set_ui_language(UiLanguage::English)
        .apply_blocking()?;
    let saved = std::fs::read(home.path().join("config.toml"))?;
    command(home.path(), project.path())?
        .env("CODEX_UI_LANGUAGE", "ko")
        .assert()
        .success()
        .stdout(contains("사용법:"));
    command(home.path(), project.path())?
        .env("CODEX_UI_LANGUAGE", "kr")
        .assert()
        .failure()
        .stderr(contains("CODEX_UI_LANGUAGE must be ko or en"));
    command(home.path(), project.path())?
        .assert()
        .success()
        .stdout(contains("Usage:"));
    assert_eq!(std::fs::read(home.path().join("config.toml"))?, saved);
    std::fs::write(home.path().join("config.toml"), "ui_language = 'kr'\n")?;
    command(home.path(), project.path())?
        .assert()
        .failure()
        .stderr(contains("ui_language"));
    Ok(())
}
