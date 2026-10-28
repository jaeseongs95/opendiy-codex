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
