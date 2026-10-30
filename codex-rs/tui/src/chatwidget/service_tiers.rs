// Modified for OpenDIY Codex R30 (2026-10-30); see MODIFICATIONS.md.
//! Service-tier selection and model-catalog helpers for `ChatWidget`.

use super::ChatWidget;
use crate::app_command::AppCommand;
use crate::app_event::AppEvent;
use crate::bottom_pane::slash_commands::ServiceTierCommand;
use crate::service_tier_resolution;
use codex_features::Feature;
use codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE;
use codex_protocol::openai_models::SPEED_TIER_FAST;

impl ChatWidget {
    pub(crate) fn set_service_tier(&mut self, service_tier: Option<String>) {
        self.config.service_tier = service_tier;
        self.refresh_effective_service_tier();
        self.refresh_model_dependent_surfaces();
    }

    pub(crate) fn current_service_tier(&self) -> Option<&str> {
        self.effective_service_tier.as_deref()
    }

    pub(crate) fn configured_service_tier(&self) -> Option<String> {
        self.config.service_tier.clone()
    }

    pub(crate) fn service_tier_update_for_core(&self) -> Option<Option<String>> {
        service_tier_resolution::service_tier_update_for_core(
            &self.config,
            &self.local_settings.notices,
            self.current_model(),
            &self.model_catalog.try_list_models().unwrap_or_default(),
        )
    }

    pub(super) fn fast_mode_enabled(&self) -> bool {
        self.config.features.enabled(Feature::FastMode)
    }

    pub(crate) fn can_toggle_fast_mode_from_keybinding(&self) -> bool {
        self.fast_mode_enabled()
            && self.current_model_fast_service_tier().is_some()
            && !self.input_queue.service_tier_selection_pending
            && !self.is_user_turn_pending_or_running()
            && self.bottom_pane.no_modal_or_popup_active()
    }

    pub(crate) fn toggle_fast_mode_from_ui(&mut self) {
        let Some(fast_tier) = self.current_model_fast_service_tier() else {
            return;
        };
        let next_tier = if self.current_service_tier() == Some(fast_tier.id.as_str()) {
            Some(SERVICE_TIER_DEFAULT_REQUEST_VALUE.to_string())
        } else {
            Some(fast_tier.id)
        };
        self.set_service_tier_selection(next_tier);
    }

    pub(crate) fn toggle_service_tier_from_ui(&mut self, command: ServiceTierCommand) {
        let next_tier = if self.current_service_tier() == Some(command.id.as_str()) {
            Some(SERVICE_TIER_DEFAULT_REQUEST_VALUE.to_string())
        } else {
            Some(command.id)
        };
        self.set_service_tier_selection(next_tier);
    }

    pub(super) fn sync_service_tier_commands(&mut self) {
        let commands = self.current_model_service_tier_commands();
        self.bottom_pane
            .set_service_tier_commands_enabled(!commands.is_empty());
        self.bottom_pane.set_service_tier_commands(commands);
    }

    pub(super) fn current_model_service_tier_commands(&self) -> Vec<ServiceTierCommand> {
        let model = self.current_model();
        self.model_catalog
            .try_list_models()
            .ok()
            .and_then(|models| {
                models
                    .into_iter()
                    .find(|preset| preset.model == model)
                    .map(|preset| {
                        preset
                            .service_tiers
                            .into_iter()
                            .filter(|tier| self.config.features.service_tier_enabled(&tier.id))
                            .map(|tier| ServiceTierCommand {
                                id: tier.id,
                                name: tier.name.to_lowercase(),
                                description: tier.description,
                            })
                            .collect()
                    })
            })
            .unwrap_or_default()
    }

    /// Validate against the current catalog and existing feature/account filtering.
    pub(crate) fn can_select_service_tier(&self, service_tier: Option<&str>) -> bool {
        service_tier.is_none_or(|tier| {
            tier == SERVICE_TIER_DEFAULT_REQUEST_VALUE
                || self
                    .current_model_service_tier_commands()
                    .iter()
                    .any(|command| command.id == tier)
        })
    }

    pub(crate) fn set_service_tier_selection_pending(&mut self, pending: bool) {
        self.input_queue.service_tier_selection_pending = pending;
    }

    fn set_service_tier_selection(&mut self, service_tier: Option<String>) {
        if self.input_queue.service_tier_selection_pending {
            self.add_error_message("A service tier selection is awaiting confirmation.".into());
            return;
        }
        let Some(thread_id) = self.thread_id else {
            self.add_error_message("Service tier selection requires a loaded conversation.".into());
            return;
        };
        if !self.can_select_service_tier(service_tier.as_deref()) {
            self.add_error_message(
                "The selected service tier is unavailable for this model.".into(),
            );
            return;
        }
        // Keep following queued input behind the thread-settings ACK. The indicator stays at
        // its confirmed value, and no default is persisted by this session selection.
        self.set_service_tier_selection_pending(true);
        self.app_event_tx.send(AppEvent::SubmitThreadOp {
            thread_id,
            op: AppCommand::SelectServiceTier {
                owner_generation: self.thread_runtime_generation,
                service_tier,
            },
        });
    }

    fn current_model_fast_service_tier(&self) -> Option<ServiceTierCommand> {
        self.current_model_service_tier_commands()
            .into_iter()
            .find(|tier| tier.name.eq_ignore_ascii_case(SPEED_TIER_FAST))
    }

    pub(super) fn refresh_effective_service_tier(&mut self) {
        self.effective_service_tier = service_tier_resolution::effective_service_tier(
            &self.config,
            &self.local_settings.notices,
            self.current_model(),
            &self.model_catalog.try_list_models().unwrap_or_default(),
        );
    }
}
