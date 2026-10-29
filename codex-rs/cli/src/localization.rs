//! Localize Clap presentation while preserving argument IDs, names, values and parsers.

use clap::Command;
use codex_tui::i18n::Locale;
use codex_tui::i18n::text_for;

pub(super) fn localize_help(mut command: Command, locale: Locale) -> Command {
    if locale == Locale::English {
        return command;
    }
    if let Some(about) = command.get_about() {
        let localized = text_for(locale, &about.to_string()).to_string();
        command = command.about(localized);
    }
    if let Some(about) = command.get_long_about() {
        let localized = text_for(locale, &about.to_string()).to_string();
        command = command.long_about(localized);
    }
    command
        .subcommand_help_heading("명령어")
        .help_template("{before-help}{name} {version}\n{about-with-newline}\n사용법: {usage}\n\n{all-args}{after-help}")
        .mut_args(|arg| {
            let help = arg.get_help().map(|help| text_for(locale, &help.to_string()).to_string());
            let long_help = arg.get_long_help().map(|help| text_for(locale, &help.to_string()).to_string());
            let heading = if arg.is_positional() { "인수" } else { "옵션" };
            arg.help(help).long_help(long_help).help_heading(heading)
        })
        .mut_subcommands(|child| localize_help(child, locale))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Arg;
    use clap::CommandFactory;

    #[test]
    fn korean_help_keeps_cli_argument_and_subcommand_tokens() {
        let mut command = localize_help(crate::MultitoolCli::command(), Locale::Korean);
        let help = command.render_help().to_string();
        assert!(help.contains("사용법:"));
        assert!(help.contains("명령어:"));
        assert!(help.contains("비대화형으로 Codex 실행"));
        assert!(help.contains("--no-alt-screen"));
        assert!(help.contains("exec"));
        let parsed = command
            .try_get_matches_from(["codex", "exec", "hello 원문"])
            .expect("localized command parses the original syntax");
        let exec = parsed
            .subcommand_matches("exec")
            .expect("exec token unchanged");
        assert_eq!(
            exec.get_one::<String>("prompt").map(String::as_str),
            Some("hello 원문")
        );
    }

    fn command_at<'a>(mut command: &'a mut Command, path: &[&str]) -> &'a mut Command {
        for name in path {
            command = command
                .find_subcommand_mut(name)
                .expect("existing command token");
        }
        command
    }

    #[test]
    fn r28_korean_help_covers_remote_startup_status_and_cloud_environments() {
        let mut command = localize_help(crate::MultitoolCli::command(), Locale::Korean);
        let server_help = command_at(&mut command, &["app-server"])
            .render_help()
            .to_string();
        assert!(server_help.contains("--no-remote-control"));
        assert!(
            server_help.contains("저장된 등록과 설정을 유지하며"),
            "{server_help}"
        );
        let remote_help = command_at(&mut command, &["remote-control"])
            .render_help()
            .to_string();
        assert!(remote_help.contains("status"));
        assert!(
            remote_help
                .split_whitespace()
                .collect::<String>()
                .contains("relay상태를조회합니다"),
            "{remote_help}"
        );
        let cloud_help = command_at(&mut command, &["cloud"])
            .render_help()
            .to_string();
        assert!(cloud_help.contains("environments"));
        assert!(
            cloud_help.contains("접근 가능한 Codex Cloud 환경 목록 표시"),
            "{cloud_help}"
        );
        let environments_help = command_at(&mut command, &["cloud", "environments"])
            .render_help()
            .to_string();
        assert!(environments_help.contains("--json"));
        assert!(
            environments_help.contains("일반 텍스트 대신 JSON 출력"),
            "{environments_help}"
        );

        let parsed = command
            .clone()
            .try_get_matches_from(["codex", "app-server", "--no-remote-control"])
            .expect("localized startup flag retains its parser");
        assert!(
            parsed
                .subcommand_matches("app-server")
                .expect("app-server")
                .get_flag("no_remote_control")
        );
        let parsed = command
            .clone()
            .try_get_matches_from(["codex", "remote-control", "status", "--json"])
            .expect("localized status command retains its parser");
        let remote = parsed
            .subcommand_matches("remote-control")
            .expect("remote-control");
        assert!(remote.get_flag("json"));
        assert!(remote.subcommand_matches("status").is_some());
        let parsed = command
            .try_get_matches_from(["codex", "cloud", "environments", "--json"])
            .expect("localized environments command retains its parser");
        assert!(
            parsed
                .subcommand_matches("cloud")
                .expect("cloud")
                .subcommand_matches("environments")
                .expect("environments")
                .get_flag("json")
        );
    }

    #[test]
    fn r28_english_help_preserves_new_command_and_flag_descriptions() {
        let mut original = crate::MultitoolCli::command();
        let mut english = localize_help(crate::MultitoolCli::command(), Locale::English);
        for path in [
            vec!["app-server"],
            vec!["remote-control"],
            vec!["remote-control", "status"],
            vec!["cloud"],
            vec!["cloud", "environments"],
        ] {
            assert_eq!(
                command_at(&mut original, &path).render_help().to_string(),
                command_at(&mut english, &path).render_help().to_string()
            );
        }
        let help = command_at(&mut english, &["app-server"])
            .render_help()
            .to_string();
        assert!(help.contains("Start with remote control disabled"));
        assert!(help.contains("--no-remote-control"));
    }

    #[test]
    fn english_help_and_unknown_descriptions_are_preserved() {
        let mut original = crate::MultitoolCli::command();
        let mut english = localize_help(crate::MultitoolCli::command(), Locale::English);
        assert_eq!(
            original.render_help().to_string(),
            english.render_help().to_string()
        );
        let mut unknown = localize_help(
            Command::new("example")
                .about("custom original")
                .arg(Arg::new("custom").long("custom").help("unknown original")),
            Locale::Korean,
        );
        let help = unknown.render_help().to_string();
        assert!(help.contains("custom original"));
        assert!(help.contains("unknown original"));
        assert!(help.contains("--custom"));
    }
}
