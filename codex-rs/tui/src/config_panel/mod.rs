// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! Catalog of TUI configuration editors and directly editable preferences shown by `/config`.
//!
//! This module defines the stable grouping, copy, search terms, and displayed values. Rendering
//! and interaction live in later modules so they can consume one ordered catalog.

mod input;
mod render;
mod toggle;
pub(crate) use render::EFFECTS_VIEW_ID;
pub(crate) use render::copy_on_select;
pub(crate) use render::effects;
pub(crate) use render::effects_with_toggle_states;
pub(crate) use render::notification_condition;
pub(crate) use render::resume_directory;
pub(crate) use render::right_click_paste;
pub(crate) use render::session_picker_layout;
pub(crate) use toggle::ConfigPreference;
pub(crate) use toggle::ConfigToggle;

#[cfg(test)]
#[path = "config_panel_tests.rs"]
mod tests;

use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::ListSelectionView;
use crate::key_hint::KeyBinding;
use crate::keymap::ListKeymap;
use codex_config::types::CopyOnSelect;
use codex_config::types::NotificationCondition;
use codex_config::types::Notifications;
use codex_config::types::RightClickPaste;
use codex_config::types::Tui;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

const RIGHT_CLICK_PASTE_DESCRIPTION: &str = "Paste when nothing is selected, with Auto following platform defaults and On also enabling macOS";

pub(crate) struct ConfigPanelUpdate {
    pub(crate) tui: Tui,
    pub(crate) keymap: ListKeymap,
    pub(crate) status: ConfigPanelStatus,
    pub(crate) notice: Option<String>,
}

#[derive(Default)]
struct PickerState {
    tab: Option<String>,
    selected: Option<usize>,
    query: Option<String>,
    notice: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfigEditor {
    Theme,
    Effects,
    StatusLine,
    TerminalTitle,
    Keyboard,
    SessionPickerLayout,
    ResumeDirectory,
    CopyOnSelect,
    RightClickPaste,
    NotificationCondition,
}

#[derive(Clone, Copy)]
pub(crate) enum ResumeCurrentAvailability {
    Available,
    RequiresCwdOverride,
}

/// Effective state the panel cannot derive from the `Tui` preferences alone.
#[derive(Clone, Copy)]
pub(crate) struct ConfigPanelStatus {
    pub(crate) copy_on_select: bool,
    pub(crate) right_click_paste: Result<bool, &'static str>,
    pub(crate) custom_notification_filters: bool,
}

/// State shared by the panel that queued an editor and the event that will open it.
#[derive(Clone, Debug)]
pub(crate) struct ConfigEditorHandoff {
    pub(crate) editor: ConfigEditor,
    pub(crate) activation_key: KeyBinding,
    activation: Arc<Mutex<ConfigEditorActivation>>,
}

#[derive(Debug)]
struct ConfigEditorActivation {
    active: bool,
    last_seen_at: Instant,
}

impl ConfigEditorHandoff {
    pub(crate) fn new(editor: ConfigEditor, activation_key: KeyBinding) -> Self {
        Self {
            editor,
            activation_key,
            activation: Arc::new(Mutex::new(ConfigEditorActivation {
                active: true,
                last_seen_at: Instant::now(),
            })),
        }
    }

    pub(crate) fn mark_activation_seen(&self) {
        self.mark_activation_seen_at(Instant::now());
    }

    fn mark_activation_seen_at(&self, seen_at: Instant) {
        let mut activation = self
            .activation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if activation.active {
            activation.last_seen_at = activation.last_seen_at.max(seen_at);
        }
    }

    pub(crate) fn mark_activation_inactive(&self) {
        self.activation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active = false;
    }

    pub(crate) fn active_at(&self) -> Option<Instant> {
        let activation = self
            .activation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        activation.active.then_some(activation.last_seen_at)
    }
}

impl ConfigEditor {
    fn value(self, tui: &Tui, status: ConfigPanelStatus) -> ConfigSettingValue {
        let label = match self {
            Self::Theme => tui
                .theme
                .clone()
                .unwrap_or_else(|| crate::i18n::text("Automatic").to_string()),
            Self::SessionPickerLayout => match tui.session_picker_view.unwrap_or_default() {
                codex_config::types::SessionPickerViewMode::Comfortable => {
                    crate::i18n::text("Comfortable")
                }
                codex_config::types::SessionPickerViewMode::Dense => crate::i18n::text("Dense"),
            }
            .to_string(),
            Self::ResumeDirectory => match tui.resume_cwd {
                None => crate::i18n::text("Ask"),
                Some(codex_config::types::ResumeCwdMode::Current) => {
                    crate::i18n::text("Current directory")
                }
                Some(codex_config::types::ResumeCwdMode::Session) => {
                    crate::i18n::text("Session directory")
                }
            }
            .to_string(),
            Self::CopyOnSelect => match tui.copy_on_select {
                CopyOnSelect::Auto => format!(
                    "Auto · {}",
                    if status.copy_on_select {
                        crate::i18n::text("On")
                    } else {
                        crate::i18n::text("Off")
                    }
                ),
                CopyOnSelect::Always => crate::i18n::text("On").to_string(),
                CopyOnSelect::Never => crate::i18n::text("Off").to_string(),
            },
            Self::RightClickPaste => {
                let preference = match tui.right_click_paste {
                    RightClickPaste::Auto => crate::i18n::text("Auto"),
                    RightClickPaste::On => crate::i18n::text("On"),
                    RightClickPaste::Off => crate::i18n::text("Off"),
                };
                match status.right_click_paste {
                    Ok(enabled) if tui.right_click_paste == RightClickPaste::Auto => {
                        format!(
                            "Auto · {}",
                            if enabled {
                                crate::i18n::text("On")
                            } else {
                                crate::i18n::text("Off")
                            }
                        )
                    }
                    Ok(_) => preference.to_string(),
                    Err(_) => format!("{preference} · Unavailable"),
                }
            }
            Self::NotificationCondition => match tui.notification_settings.condition {
                NotificationCondition::Unfocused => crate::i18n::text("Unfocused"),
                NotificationCondition::Always => crate::i18n::text("Always"),
            }
            .to_string(),
            Self::Effects | Self::StatusLine | Self::TerminalTitle | Self::Keyboard => {
                crate::i18n::text("Configure").to_string()
            }
        };
        ConfigSettingValue::Editor {
            label,
            editor: self,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfigSection {
    Appearance,
    Notifications,
    Input,
    Sessions,
    TextSelection,
}

impl ConfigSection {
    const ALL: [Self; 5] = [
        Self::Appearance,
        Self::Notifications,
        Self::Input,
        Self::Sessions,
        Self::TextSelection,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Appearance => crate::i18n::text("Appearance"),
            Self::Notifications => crate::i18n::text("Notifications"),
            Self::Input => crate::i18n::text("Input"),
            Self::Sessions => crate::i18n::text("Sessions"),
            Self::TextSelection => crate::i18n::text("Text selection"),
        }
    }
}

struct ConfigSetting {
    section: ConfigSection,
    label: &'static str,
    description: &'static str,
    keywords: &'static str,
    value: ConfigSettingValue,
}

enum ConfigSettingValue {
    Toggle {
        setting: ConfigToggle,
        enabled: bool,
    },
    Editor {
        label: String,
        editor: ConfigEditor,
    },
    DependentToggle {
        setting: ConfigToggle,
        enabled: bool,
        dependency_met: bool,
        unavailable_reason: &'static str,
    },
    NotificationTest,
    Unavailable {
        reason: &'static str,
    },
}

impl ConfigSettingValue {
    fn label(&self) -> String {
        match self {
            Self::Toggle { enabled, .. }
            | Self::DependentToggle {
                enabled,
                dependency_met: true,
                ..
            } => if *enabled {
                crate::i18n::text("[x] On")
            } else {
                crate::i18n::text("[ ] Off")
            }
            .to_string(),
            Self::Editor { label, .. } => format!("{label} ›"),
            Self::NotificationTest => crate::i18n::text("Send in 5s").to_string(),
            Self::DependentToggle {
                dependency_met: false,
                ..
            }
            | Self::Unavailable { .. } => crate::i18n::text("Unavailable").to_string(),
        }
    }
}

fn settings(tui: &Tui, status: ConfigPanelStatus) -> Vec<ConfigSetting> {
    let notifications_enabled = !matches!(
        tui.notification_settings.notifications,
        Notifications::Enabled(false)
    );
    vec![
        ConfigSetting {
            section: ConfigSection::Appearance,
            label: crate::i18n::text("Theme"),
            description: crate::i18n::text("Choose a syntax color palette with a live preview"),
            keywords: "colors dark light palette",
            value: ConfigEditor::Theme.value(tui, status),
        },
        ConfigSetting {
            section: ConfigSection::Appearance,
            label: crate::i18n::text("Effects"),
            description: crate::i18n::text("Configure animations and composer effects"),
            keywords: "animations motion accessibility spinner starfield stars composer stars",
            value: ConfigEditor::Effects.value(tui, status),
        },
        ConfigSetting {
            section: ConfigSection::Appearance,
            label: crate::i18n::text("Tips"),
            description: crate::i18n::text("Show helpful shortcuts and suggestions"),
            keywords: "welcome hints tooltips",
            value: ConfigSettingValue::Toggle {
                setting: ConfigToggle::Tips,
                enabled: ConfigToggle::Tips.enabled(tui),
            },
        },
        ConfigSetting {
            section: ConfigSection::Appearance,
            label: crate::i18n::text("Status line colors"),
            description: crate::i18n::text(
                "Use theme colors for the model, branch, and other status line details",
            ),
            keywords: "statusline footer colour",
            value: ConfigSettingValue::Toggle {
                setting: ConfigToggle::StatusLineColors,
                enabled: ConfigToggle::StatusLineColors.enabled(tui),
            },
        },
        ConfigSetting {
            section: ConfigSection::Appearance,
            label: crate::i18n::text("Status line display"),
            description: crate::i18n::text("Choose and reorder the information below the composer"),
            keywords: "statusline footer model branch context",
            value: ConfigEditor::StatusLine.value(tui, status),
        },
        ConfigSetting {
            section: ConfigSection::Appearance,
            label: crate::i18n::text("Terminal title display"),
            description: crate::i18n::text(
                "Choose and reorder the information in your terminal tab or window title",
            ),
            keywords: "title tab window name",
            value: ConfigEditor::TerminalTitle.value(tui, status),
        },
        ConfigSetting {
            section: ConfigSection::Notifications,
            label: crate::i18n::text("Notifications"),
            description: crate::i18n::text(
                "Let your terminal notify you when a turn finishes or approval is needed",
            ),
            keywords: "alerts bell completion approval",
            value: if status.custom_notification_filters {
                ConfigSettingValue::Unavailable {
                    reason: crate::i18n::text(
                        "Event-specific notifications are configured in config.toml",
                    ),
                }
            } else {
                ConfigSettingValue::Toggle {
                    setting: ConfigToggle::Notifications,
                    enabled: notifications_enabled,
                }
            },
        },
        ConfigSetting {
            section: ConfigSection::Notifications,
            label: crate::i18n::text("Notify when"),
            description: crate::i18n::text(
                "Notify only while you are away or while this terminal is focused too",
            ),
            keywords: "focus alerts background always unfocused",
            value: if notifications_enabled {
                ConfigEditor::NotificationCondition.value(tui, status)
            } else {
                ConfigSettingValue::Unavailable {
                    reason: crate::i18n::text("Turn on notifications to choose when they appear"),
                }
            },
        },
        ConfigSetting {
            section: ConfigSection::Notifications,
            label: crate::i18n::text("Test notification"),
            description: crate::i18n::text(
                "Send in 5 seconds so you can switch apps before the notification appears",
            ),
            keywords: "alerts bell sound test",
            value: ConfigSettingValue::NotificationTest,
        },
        ConfigSetting {
            section: ConfigSection::Input,
            label: crate::i18n::text("Keyboard shortcuts"),
            description: crate::i18n::text("Browse actions and remap their keys"),
            keywords: "keymap bindings hotkeys",
            value: ConfigEditor::Keyboard.value(tui, status),
        },
        ConfigSetting {
            section: ConfigSection::Input,
            label: crate::i18n::text("Start in Vim mode"),
            description: crate::i18n::text(
                "Start new composers in Vim mode and use /vim for the current composer",
            ),
            keywords: "editor modal normal",
            value: ConfigSettingValue::Toggle {
                setting: ConfigToggle::StartInVimMode,
                enabled: ConfigToggle::StartInVimMode.enabled(tui),
            },
        },
        ConfigSetting {
            section: ConfigSection::Input,
            label: crate::i18n::text("Paste detection"),
            description: crate::i18n::text(
                "Recognize pasted-text bursts and keep multiline input together",
            ),
            keywords: "burst clipboard multiline",
            value: ConfigSettingValue::Toggle {
                setting: ConfigToggle::PasteDetection,
                enabled: ConfigToggle::PasteDetection.enabled(tui),
            },
        },
        ConfigSetting {
            section: ConfigSection::Sessions,
            label: crate::i18n::text("Automatic recaps"),
            description: crate::i18n::text("Prepare recaps while the terminal is unfocused"),
            keywords: "summary background",
            value: ConfigSettingValue::Toggle {
                setting: ConfigToggle::AutomaticRecaps,
                enabled: ConfigToggle::AutomaticRecaps.enabled(tui),
            },
        },
        ConfigSetting {
            section: ConfigSection::Sessions,
            label: crate::i18n::text("Session picker layout"),
            description: crate::i18n::text(
                "Choose tighter rows or a roomier list when resuming and forking conversations",
            ),
            keywords: "dense comfortable spacing resume fork",
            value: ConfigEditor::SessionPickerLayout.value(tui, status),
        },
        ConfigSetting {
            section: ConfigSection::Sessions,
            label: crate::i18n::text("Resume directory"),
            description: crate::i18n::text(
                "Choose which working directory to use when the current directory and saved session differ",
            ),
            keywords: "cwd folder path fork ask",
            value: ConfigEditor::ResumeDirectory.value(tui, status),
        },
        ConfigSetting {
            section: ConfigSection::TextSelection,
            label: crate::i18n::text("Copy on select"),
            description: crate::i18n::text(
                "Copy transcript, footer, and composer selections on release; Auto follows terminal defaults",
            ),
            keywords: "clipboard mouse selection automatic drag",
            value: ConfigEditor::CopyOnSelect.value(tui, status),
        },
        ConfigSetting {
            section: ConfigSection::TextSelection,
            label: crate::i18n::text("Deselect after copy"),
            description: crate::i18n::text(
                "Remove read-only transcript and footer highlights after automatic copy succeeds",
            ),
            keywords: "clipboard deselect clear highlight automatic mouse selection",
            value: ConfigSettingValue::DependentToggle {
                setting: ConfigToggle::CopyOnSelectClearSelection,
                enabled: ConfigToggle::CopyOnSelectClearSelection.enabled(tui),
                dependency_met: status.copy_on_select,
                unavailable_reason: crate::i18n::text(
                    "Turn on Copy on select to deselect after copying",
                ),
            },
        },
        ConfigSetting {
            section: ConfigSection::TextSelection,
            label: crate::i18n::text("Right-click paste"),
            description: status
                .right_click_paste
                .err()
                .unwrap_or(RIGHT_CLICK_PASTE_DESCRIPTION),
            keywords: "clipboard mouse paste automatic",
            value: ConfigEditor::RightClickPaste.value(tui, status),
        },
    ]
}

pub(crate) struct ConfigPanel {
    settings: Vec<ConfigSetting>,
    picker: ListSelectionView,
    saving: bool,
    editor_opening: Option<ConfigEditorHandoff>,
    app_event_tx: AppEventSender,
}

impl ConfigPanel {
    pub(crate) fn new(
        tui: &Tui,
        app_event_tx: AppEventSender,
        keymap: ListKeymap,
        status: ConfigPanelStatus,
    ) -> Self {
        let settings = settings(tui, status);
        let picker = render::picker(
            &settings,
            app_event_tx.clone(),
            keymap,
            PickerState::default(),
        );
        Self {
            settings,
            picker,
            saving: false,
            editor_opening: None,
            app_event_tx,
        }
    }
}
