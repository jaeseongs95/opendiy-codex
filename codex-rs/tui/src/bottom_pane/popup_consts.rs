// Modified for OpenDIY Codex R30 (2026-10-30); see MODIFICATIONS.md.
//! Shared popup-related constants for bottom pane widgets.

use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::key_hint;
use crate::key_hint::ShortcutHint;
use crate::keymap::ListAction;
use crate::keymap::ListKeymap;
use crossterm::event::KeyCode;

/// Maximum number of rows any popup should attempt to display.
/// Keep this consistent across all popups for a uniform feel.
pub(crate) const MAX_POPUP_ROWS: usize = 8;

/// Standard footer hint text used by popups.
pub(crate) fn standard_popup_hint_line() -> Line<'static> {
    Line::from(vec![
        crate::i18n::text("Press ").into(),
        key_hint::plain(KeyCode::Enter).into(),
        crate::i18n::text(" to confirm or ").into(),
        key_hint::plain(KeyCode::Esc).into(),
        crate::i18n::text(" to go back").into(),
    ])
}

/// Compact footer for shared pickers, using only the configured list actions.
pub(crate) fn picker_hint_line_for_keymap(list_keymap: &ListKeymap) -> Line<'static> {
    let mut spans = Vec::new();
    for (action, label) in [
        (ListAction::Accept, crate::i18n::text("select")),
        (ListAction::Cancel, crate::i18n::text("back")),
    ] {
        if let Some(hint) = list_keymap.primary_hint(action) {
            if !spans.is_empty() {
                spans.push(" · ".dim());
            }
            spans.extend(hint.spans());
            spans.push(format!(" {label}").dim());
        }
    }
    spans.into()
}

pub(crate) fn accept_cancel_hint_line(
    accept: Option<ShortcutHint>,
    accept_label: &'static str,
    cancel: Option<ShortcutHint>,
    cancel_label: &'static str,
) -> Line<'static> {
    let accept_label = crate::i18n::text(accept_label);
    let cancel_label = crate::i18n::text(cancel_label);
    let mut spans = Vec::new();
    if let Some(accept) = accept {
        spans.push(crate::i18n::text("Press ").dim());
        spans.extend(accept.spans());
        spans.push(format!(" {accept_label}").dim());
    }
    if let Some(cancel) = cancel {
        spans.push(
            if spans.is_empty() {
                crate::i18n::text("Press ")
            } else {
                crate::i18n::text(" or ")
            }
            .dim(),
        );
        spans.extend(cancel.spans());
        spans.push(format!(" {cancel_label}").dim());
    }
    spans.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;

    #[test]
    fn korean_popup_hint_renders_korean_and_preserves_keys() {
        crate::i18n::with_locale(Locale::Korean, || {
            let text = standard_popup_hint_line().to_string();
            assert_eq!(text, "키: enter 확인 / esc 돌아가기");
        });
        crate::i18n::with_locale(Locale::English, || {
            assert_eq!(
                standard_popup_hint_line().to_string(),
                "Press enter to confirm or esc to go back"
            );
        });
    }
}
