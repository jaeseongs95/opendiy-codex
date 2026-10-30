// Modified for OpenDIY Codex R29 (2026-10-30); see MODIFICATIONS.md.
//! Explicit memory management commands; model inference is not needed.
use super::App;
use crate::app_server_session::AppServerSession;
use crate::i18n::text;
use codex_app_server_protocol::WriteStatus;
use codex_config::types::MemoryScope;
use codex_protocol::ThreadId;

impl App {
    pub(super) async fn handle_memory_command(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: Option<ThreadId>,
        args: String,
    ) {
        let args = args.trim();
        if args == "reset-project-unavailable" {
            self.chat_widget.add_error_message(text("Project memory reset is unavailable. Disable generation and use an explicit correction; no deletion has been performed.").to_string());
            return;
        }
        let Some(thread_id) =
            thread_id.filter(|id| Some(*id) == self.current_displayed_thread_id())
        else {
            self.chat_widget.add_error_message(
                text("Open a connected chat before managing its memories.").to_string(),
            );
            return;
        };
        let (command, value) = args
            .split_once(char::is_whitespace)
            .map_or((args, ""), |(command, value)| (command, value.trim()));
        match command {
            "" => match app_server.memory_read(thread_id).await {
                Ok(response) => self.chat_widget.open_memories_popup_with_scope(response.scope.as_deref().unwrap_or("unknown")),
                Err(error) => self.chat_widget.add_error_message(format!("{} {error:#}", text("Memory request failed:"))),
            },
            "view" | "show" | "status" if value.is_empty() => {
                match app_server.memory_read(thread_id).await {
                    Ok(response) => {
                        self.chat_widget.add_info_message(format!("{} {} · {} {}\n{} {}\n\n{}\n\n{}",
                            text("Memory scope:"), text(response.scope.as_deref().unwrap_or("unknown")),
                            text("State:"), text(response.status.as_deref().unwrap_or("unknown")),
                            text("Storage:"), response.root.as_deref().unwrap_or("-"),
                            response.summary.as_deref().unwrap_or(text("No stored memory summary is available.")),
                            text("This is the stored summary. Existing chat context is unchanged; new context loads the latest summary.")),
                            Some("/memory edit <correction> · /memory read on|off · /memory generate on|off".to_string()));
                    }
                    Err(error) => self.chat_widget.add_error_message(format!("{} {error:#}", text("Memory request failed:"))),
                }
            }
            "edit" | "remember" | "forget" => {
                if value.is_empty() { self.chat_widget.prepare_memory_note(); return; }
                let note = if command == "forget" { format!("Explicit user forget request: {value}") } else { value.to_string() };
                match app_server.memory_note_add(thread_id, note).await {
                    Ok(_) => self.chat_widget.add_info_message(text("Memory correction saved for later consolidation. The stored summary and existing chats have not changed yet.").to_string(), None),
                    Err(error) => self.chat_widget.add_error_message(format!("{} {error:#}", text("Memory request failed:"))),
                }
            }
            "read" | "generate" if matches!(value, "on" | "off") => {
                let on = value == "on";
                let use_memories = if command == "read" { on } else { self.config.memories.use_memories };
                let generate = if command == "generate" { on } else { self.config.memories.generate_memories };
                if !self.update_memory_settings(app_server, use_memories, generate).await { return; }
                if command == "generate" {
                    let mode = if on { codex_app_server_protocol::ThreadMemoryMode::Enabled } else { codex_app_server_protocol::ThreadMemoryMode::Disabled };
                    if let Err(error) = app_server.thread_memory_mode_set(thread_id, mode).await {
                        self.chat_widget.add_error_message(format!("{} {error:#}", text("Memory request failed:")));
                        return;
                    }
                }
                self.chat_widget.add_info_message(text("Memory settings saved. Reading changes apply to new chats. Generation changes stop new claims; already running work may finish.").to_string(), None);
            }
            "scope" if matches!(value, "project" | "global") => {
                let edits = vec![crate::config_update::replace_config_value("memories.scope", serde_json::json!(value))];
                match crate::config_update::write_config_batch(app_server.request_handle(), edits).await {
                    Ok(response) if response.status != WriteStatus::OkOverridden => {
                        self.config.memories.scope = if value == "project" { MemoryScope::Project } else { MemoryScope::Global };
                        self.chat_widget.add_info_message(text("Memory scope saved for new chats. Existing global or external memories were not moved.").to_string(), None);
                    }
                    Ok(_) => self.chat_widget.add_error_message(text("Memory scope was overridden by managed configuration.").to_string()),
                    Err(error) => self.chat_widget.add_error_message(format!("{} {error:#}", text("Memory request failed:"))),
                }
            }
            _ => self.chat_widget.add_error_message(text("Usage: /memory view | edit <correction> | read on|off | generate on|off | scope project|global").to_string()),
        }
    }
}
