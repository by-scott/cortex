use super::render::TelegramTextChunk;
use super::{TELEGRAM_API, TELEGRAM_SEND_TIMEOUT, TelegramChannel, keyboard, render};

impl TelegramChannel {
    /// Flush a text buffer to a bubble: send if new, edit if existing.
    /// Returns the (possibly new) message ID.
    pub(super) async fn flush_text_bubble(
        &self,
        chat_id: i64,
        buf: &str,
        msg_id: Option<i64>,
        text_msg_ids: &mut Vec<i64>,
    ) -> Option<i64> {
        if buf.is_empty() {
            return msg_id;
        }
        if let Some(mid) = msg_id {
            match self
                .edit_single_message_with_keyboard(chat_id, mid, buf, None)
                .await
            {
                Ok(()) => {
                    tracing::debug!(
                        chat_id,
                        message_id = mid,
                        text_len = buf.len(),
                        "[telegram] edited text message"
                    );
                    Some(mid)
                }
                Err(err) => {
                    tracing::warn!(
                        chat_id,
                        message_id = mid,
                        text_len = buf.len(),
                        "[telegram] text edit failed; sending a fresh message instead: {err}"
                    );
                    match self.send_message_get_id(chat_id, buf, None).await {
                        Ok(sent) => {
                            self.delete_message(chat_id, mid).await;
                            if !text_msg_ids.contains(&sent) {
                                text_msg_ids.push(sent);
                            }
                            Some(sent)
                        }
                        Err(send_err) => {
                            tracing::warn!(
                                chat_id,
                                message_id = mid,
                                text_len = buf.len(),
                                "[telegram] replacement text send failed, keeping previous message: {send_err}"
                            );
                            Some(mid)
                        }
                    }
                }
            }
        } else {
            match self.send_message_get_id(chat_id, buf, None).await {
                Ok(mid) => {
                    tracing::debug!(
                        chat_id,
                        message_id = mid,
                        text_len = buf.len(),
                        "[telegram] sent text message"
                    );
                    if !text_msg_ids.contains(&mid) {
                        text_msg_ids.push(mid);
                    }
                    Some(mid)
                }
                Err(err) => {
                    tracing::warn!(
                        chat_id,
                        text_len = buf.len(),
                        "[telegram] text send failed: {err}"
                    );
                    msg_id
                }
            }
        }
    }

    pub(super) async fn flush_oversized_text_bubbles(
        &self,
        chat_id: i64,
        buf: &mut String,
        msg_id: &mut Option<i64>,
        text_msg_ids: &mut Vec<i64>,
    ) {
        while render::rendered_len(buf) > render::TEXT_LIMIT {
            let Some((prefix, suffix)) = render::split_text_for_bubble(buf, render::TEXT_LIMIT)
            else {
                break;
            };
            *msg_id = self
                .flush_text_bubble(chat_id, &prefix, *msg_id, text_msg_ids)
                .await;
            *buf = suffix;
            *msg_id = None;
        }
    }

    pub(super) async fn flush_all_text_bubbles(
        &self,
        chat_id: i64,
        buf: &mut String,
        msg_id: &mut Option<i64>,
        text_msg_ids: &mut Vec<i64>,
    ) {
        self.flush_oversized_text_bubbles(chat_id, buf, msg_id, text_msg_ids)
            .await;
        if buf.is_empty() {
            return;
        }
        *msg_id = self
            .flush_text_bubble(chat_id, buf, *msg_id, text_msg_ids)
            .await;
    }

    /// Send a voice/audio file to a chat.
    async fn send_voice(&self, chat_id: i64, file_path: &str) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendVoice", self.bot_token);
        let file_bytes = std::fs::read(file_path).map_err(|e| e.to_string())?;
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name("voice.ogg")
            .mime_str("audio/mpeg")
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("voice", part);
        self.api_client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Send a photo file to a chat.
    async fn send_photo(&self, chat_id: i64, file_path: &str) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendPhoto", self.bot_token);
        let file_bytes = std::fs::read(file_path).map_err(|e| e.to_string())?;
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name("image.png")
            .mime_str("image/png")
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("photo", part);
        self.api_client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn send_video(&self, chat_id: i64, file_path: &str) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendVideo", self.bot_token);
        let file_bytes = std::fs::read(file_path).map_err(|e| e.to_string())?;
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("video.mp4");
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string())
            .mime_str("video/mp4")
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("video", part);
        self.api_client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn send_document(&self, chat_id: i64, file_path: &str) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendDocument", self.bot_token);
        let file_bytes = std::fs::read(file_path).map_err(|e| e.to_string())?;
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string())
            .mime_str("application/octet-stream")
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("document", part);
        self.api_client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub(super) async fn send_response_media(
        &self,
        chat_id: i64,
        response_parts: &[cortex_types::ResponsePart],
    ) {
        for part in response_parts {
            let cortex_types::ResponsePart::Media { attachment } = part else {
                continue;
            };
            let result = match attachment.media_type.as_str() {
                "audio" => self.send_voice(chat_id, &attachment.url).await,
                "image" => self.send_photo(chat_id, &attachment.url).await,
                "video" => self.send_video(chat_id, &attachment.url).await,
                _ => self.send_document(chat_id, &attachment.url).await,
            };
            if let Err(error) = result {
                tracing::warn!("[telegram] Failed to send media: {error}");
                let _ = self.send_message(chat_id, "[media unavailable]").await;
            }
        }
    }

    async fn send_rendered_html(
        &self,
        chat_id: i64,
        html: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendMessage", self.bot_token);
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "text": html,
            "parse_mode": "HTML",
        });
        if let Some(keyboard) = keyboard {
            payload["reply_markup"] = keyboard.clone();
        }
        let resp: serde_json::Value = self
            .api_client
            .post(&url)
            .timeout(TELEGRAM_SEND_TIMEOUT)
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if resp.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(resp.to_string());
        }
        Ok(resp)
    }

    async fn send_text_plain(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendMessage", self.bot_token);
        let text = render::markdown_to_plain_text(text);
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
        });
        if let Some(keyboard) = keyboard {
            payload["reply_markup"] = keyboard.clone();
        }
        let resp: serde_json::Value = self
            .api_client
            .post(&url)
            .timeout(TELEGRAM_SEND_TIMEOUT)
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if resp.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(resp.to_string());
        }
        Ok(resp)
    }

    async fn edit_rendered_html(
        &self,
        chat_id: i64,
        message_id: i64,
        html: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/editMessageText", self.bot_token);
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": html,
            "parse_mode": "HTML",
        });
        if let Some(keyboard) = keyboard {
            payload["reply_markup"] = keyboard.clone();
        }
        let resp: serde_json::Value = self
            .api_client
            .post(&url)
            .timeout(TELEGRAM_SEND_TIMEOUT)
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if resp.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(resp.to_string());
        }
        Ok(())
    }

    async fn edit_text_plain(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/editMessageText", self.bot_token);
        let text = render::markdown_to_plain_text(text);
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text,
        });
        if let Some(keyboard) = keyboard {
            payload["reply_markup"] = keyboard.clone();
        }
        let resp: serde_json::Value = self
            .api_client
            .post(&url)
            .timeout(TELEGRAM_SEND_TIMEOUT)
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if resp.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(resp.to_string());
        }
        Ok(())
    }

    pub(super) async fn send_message(&self, chat_id: i64, text: &str) -> Result<(), String> {
        let _ = self.send_message_get_id(chat_id, text, None).await?;
        Ok(())
    }

    /// Send a message and return its `message_id` for later editing.
    pub(super) async fn send_message_get_id(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<i64, String> {
        let chunks = render::render_text_chunks(text);
        if chunks.is_empty() {
            return Err("cannot send an empty Telegram message".to_string());
        }

        let mut last_id = 0i64;
        for (idx, chunk) in chunks.iter().enumerate() {
            let chunk_keyboard = if idx + 1 == chunks.len() {
                keyboard
            } else {
                None
            };
            last_id = self
                .send_single_message_get_id(chat_id, chunk, chunk_keyboard)
                .await?;
        }
        Ok(last_id)
    }

    async fn send_single_message_get_id(
        &self,
        chat_id: i64,
        chunk: &TelegramTextChunk,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<i64, String> {
        let resp = match self
            .send_rendered_html(chat_id, &chunk.html, keyboard)
            .await
        {
            Ok(resp) => resp,
            Err(html_err) => {
                tracing::warn!("[telegram] HTML send failed, retrying plain text: {html_err}");
                self.send_text_plain(chat_id, &chunk.markdown, keyboard)
                    .await?
            }
        };
        resp.get("result")
            .and_then(|result| result.get("message_id"))
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| format!("Telegram send response missing message_id: {resp}"))
    }

    pub(super) async fn edit_message_with_keyboard(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let chunks = render::render_text_chunks(text);
        let Some(first) = chunks.first() else {
            return Err("cannot edit a Telegram message to empty text".to_string());
        };

        if chunks.len() == 1 {
            return self
                .edit_single_message_chunk(chat_id, message_id, first, keyboard)
                .await;
        }

        self.edit_single_message_chunk(chat_id, message_id, first, None)
            .await?;
        for (idx, chunk) in chunks.iter().enumerate().skip(1) {
            let chunk_keyboard = if idx + 1 == chunks.len() {
                keyboard
            } else {
                None
            };
            let _ = self
                .send_single_message_get_id(chat_id, chunk, chunk_keyboard)
                .await?;
        }
        Ok(())
    }

    pub(super) async fn edit_single_message_with_keyboard(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let chunks = render::render_text_chunks(text);
        let Some(chunk) = chunks.first() else {
            return Err("cannot edit a Telegram message to empty text".to_string());
        };
        if chunks.len() != 1 {
            return Err(format!(
                "single Telegram edit received {} rendered chunks",
                chunks.len()
            ));
        }
        self.edit_single_message_chunk(chat_id, message_id, chunk, keyboard)
            .await
    }

    async fn edit_single_message_chunk(
        &self,
        chat_id: i64,
        message_id: i64,
        chunk: &TelegramTextChunk,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        match self
            .edit_rendered_html(chat_id, message_id, &chunk.html, keyboard)
            .await
        {
            Ok(()) => Ok(()),
            Err(html_err) => {
                if html_err.contains("message is not modified") {
                    return Ok(());
                }
                tracing::warn!("[telegram] HTML edit failed, retrying plain text: {html_err}");
                self.edit_text_plain(chat_id, message_id, &chunk.markdown, keyboard)
                    .await
            }
        }
    }

    pub(super) async fn delete_message(&self, chat_id: i64, message_id: i64) {
        if message_id == 0 {
            return;
        }
        let url = format!("{TELEGRAM_API}/bot{}/deleteMessage", self.bot_token);
        let payload = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
        });
        let result = match self
            .api_client
            .post(&url)
            .timeout(TELEGRAM_SEND_TIMEOUT)
            .json(&payload)
            .send()
            .await
        {
            Ok(response) => response
                .json::<serde_json::Value>()
                .await
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        match result {
            Ok(resp) if resp.get("ok").and_then(serde_json::Value::as_bool) == Some(true) => {}
            Ok(resp) => {
                tracing::debug!(
                    chat_id,
                    message_id,
                    "[telegram] deleting stale text bubble failed: {resp}"
                );
            }
            Err(error) => {
                tracing::debug!(
                    chat_id,
                    message_id,
                    "[telegram] deleting stale text bubble failed: {error}"
                );
            }
        }
    }

    /// Send a message with an inline keyboard attached.
    pub(super) async fn send_message_with_keyboard(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: &serde_json::Value,
    ) -> Result<(), String> {
        let _ = self
            .send_message_get_id(chat_id, text, Some(keyboard))
            .await?;
        Ok(())
    }

    pub(super) async fn edit_callback_message(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        if message_id == 0 {
            if let Some(keyboard) = keyboard {
                return self
                    .send_message_with_keyboard(chat_id, text, keyboard)
                    .await;
            }
            return self.send_message(chat_id, text).await;
        }
        self.edit_message_with_keyboard(chat_id, message_id, text, keyboard)
            .await
    }

    pub(super) async fn send_permission_card(
        &self,
        chat_id: i64,
        info: &crate::daemon::PendingPermissionInfo,
    ) -> Result<(), String> {
        self.send_message_with_keyboard(
            chat_id,
            &info.prompt_text(),
            &keyboard::permission_keyboard(&info.id),
        )
        .await
    }

    pub(super) async fn send_permission_card_from_prompt(
        &self,
        chat_id: i64,
        prompt: &str,
    ) -> Result<(), String> {
        let Some(permission_id) = keyboard::parse_permission_prompt_id(prompt) else {
            return self.send_message(chat_id, prompt).await;
        };
        self.send_message_with_keyboard(
            chat_id,
            prompt,
            &keyboard::permission_keyboard(permission_id),
        )
        .await
    }
}
