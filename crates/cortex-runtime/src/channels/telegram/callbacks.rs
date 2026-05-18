use std::sync::Arc;

use super::super::store::ChannelStore;
use super::keyboard::PermissionCallbackAction;
use super::{TELEGRAM_API, TelegramChannel, keyboard};

struct TelegramCallbackContext<'a> {
    callback_id: &'a str,
    data: &'a str,
    chat_id: i64,
    message_id: i64,
    user_id: String,
    user_name: &'a str,
}

impl<'a> TelegramCallbackContext<'a> {
    fn from_value(callback: &'a serde_json::Value) -> Self {
        let callback_id = callback
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let data = callback
            .get("data")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let chat_id = callback
            .get("message")
            .and_then(|message| message.get("chat"))
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let message_id = callback
            .get("message")
            .and_then(|message| message.get("message_id"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let user_id = callback
            .get("from")
            .and_then(|from| from.get("id"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
            .to_string();
        let user_name = callback
            .get("from")
            .and_then(|from| from.get("first_name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Unknown");
        Self {
            callback_id,
            data,
            chat_id,
            message_id,
            user_id,
            user_name,
        }
    }
}

impl TelegramChannel {
    /// Handle an inline-keyboard button click (`callback_query`).
    pub(super) async fn handle_callback_query(&self, callback: &serde_json::Value) {
        let ctx = TelegramCallbackContext::from_value(callback);

        // Acknowledge the callback to remove the loading spinner
        self.answer_callback_query(ctx.callback_id).await;

        if ctx.data.is_empty() || ctx.chat_id == 0 {
            return;
        }

        if let Some(action) = keyboard::parse_permission_callback(ctx.data) {
            self.handle_permission_callback(ctx.chat_id, ctx.message_id, action)
                .await;
            return;
        }

        // Special case: bare `/session switch` shows inline keyboard of sessions
        if ctx.data == "/session switch" {
            self.handle_session_switch_callback(ctx.chat_id, ctx.message_id, &ctx.user_id)
                .await;
            return;
        }

        if let Some(keyboard) = self.command_keyboard(ctx.data) {
            self.handle_builtin_callback(ctx.chat_id, ctx.message_id, ctx.data, &keyboard)
                .await;
            return;
        }

        self.handle_message_callback(&ctx).await;
    }

    async fn handle_session_switch_callback(&self, chat_id: i64, message_id: i64, user_id: &str) {
        let state = Arc::clone(&self.state);
        let actor = crate::daemon::DaemonState::channel_actor("telegram", user_id);
        let current_session = state.active_actor_session(&actor).unwrap_or_default();
        let sessions = tokio::task::spawn_blocking(move || state.visible_sessions(&actor))
            .await
            .unwrap_or_default();

        if sessions.is_empty() {
            let _ = self
                .edit_callback_message(
                    chat_id,
                    message_id,
                    "No sessions available.",
                    self.command_keyboard("/session").as_ref(),
                )
                .await;
            return;
        }

        let keyboard = keyboard::session_switch_keyboard(&sessions, Some(&current_session));

        if keyboard.is_none() {
            let _ = self
                .edit_callback_message(
                    chat_id,
                    message_id,
                    "🗂️ No other sessions to switch to.",
                    self.command_keyboard("/session").as_ref(),
                )
                .await;
            return;
        }

        let _ = self
            .edit_callback_message(chat_id, message_id, "Choose a session:", keyboard.as_ref())
            .await;
    }

    async fn handle_builtin_callback(
        &self,
        chat_id: i64,
        message_id: i64,
        data: &str,
        keyboard: &serde_json::Value,
    ) {
        let state = Arc::clone(&self.state);
        let cmd = data.to_string();
        let response = tokio::task::spawn_blocking(move || state.dispatch_command(&cmd))
            .await
            .unwrap_or_else(|e| format!("Error: {e}"));
        let message = if response.is_empty() {
            data.to_string()
        } else {
            response
        };
        let _ = self
            .edit_callback_message(chat_id, message_id, &message, Some(keyboard))
            .await;
    }

    async fn handle_message_callback(&self, ctx: &TelegramCallbackContext<'_>) {
        let state = Arc::clone(&self.state);
        let store_dir = self.store.dir().to_path_buf();
        let uname = ctx.user_name.to_string();
        let cmd = ctx.data.to_string();
        let uid = ctx.user_id.clone();
        let response = tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            super::super::handle_message(&state, &store, &uid, &uname, &cmd, "telegram")
        })
        .await
        .unwrap_or_else(|e| format!("Error: {e}"));

        if !response.is_empty() {
            let keyboard = self.root_command_keyboard_for_callback(ctx.data);
            let _ = self
                .edit_callback_message(ctx.chat_id, ctx.message_id, &response, keyboard.as_ref())
                .await;
        }
    }

    async fn handle_permission_callback(
        &self,
        chat_id: i64,
        message_id: i64,
        action: PermissionCallbackAction<'_>,
    ) {
        let (command, pending_id) = match action {
            PermissionCallbackAction::Approve(id) => (format!("/approve {id}"), id),
            PermissionCallbackAction::Deny(id) => (format!("/deny {id}"), id),
            PermissionCallbackAction::Refresh(id) => (String::new(), id),
        };

        let response = match action {
            PermissionCallbackAction::Refresh(id) => {
                self.state.pending_permission_info(id).map_or_else(
                    || keyboard::permission_resolved_text(id),
                    |info| info.prompt_text(),
                )
            }
            PermissionCallbackAction::Approve(_) | PermissionCallbackAction::Deny(_) => {
                self.state.dispatch_command(&command)
            }
        };

        let keyboard = if self.state.pending_permission_info(pending_id).is_some() {
            keyboard::permission_keyboard(pending_id)
        } else {
            keyboard::permission_resolved_keyboard(pending_id)
        };
        let _ = self
            .edit_callback_message(chat_id, message_id, &response, Some(&keyboard))
            .await;
    }

    async fn answer_callback_query(&self, callback_id: &str) {
        let url = format!("{TELEGRAM_API}/bot{}/answerCallbackQuery", self.bot_token);
        let _ = self
            .api_client
            .post(&url)
            .json(&serde_json::json!({"callback_query_id": callback_id}))
            .send()
            .await;
    }
}
