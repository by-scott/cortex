use base64::Engine;

use super::{
    BroadcastEventExt, QQ_MSG_TYPE_MARKDOWN, QQ_MSG_TYPE_MEDIA, QQ_MSG_TYPE_TEXT, QQ_TEXT_LIMIT,
    QqChannel, ReplyTarget, ReplyTargetKind,
};

impl QqChannel {
    pub(super) async fn send_event_sequence(
        &self,
        target: &ReplyTarget,
        events: &[crate::daemon::BroadcastEvent],
        initial_msg_seq: u32,
    ) {
        tracing::info!(
            "[qq] sending event sequence target={} source_message={} events={}",
            target.kind.label(),
            target.source_message_id.is_some(),
            events.len()
        );
        let mut msg_seq = initial_msg_seq;
        for event in events {
            if let Some((text, keyboard)) = super::qq_permission_delivery(event) {
                msg_seq += 1;
                if let Err(error) = self
                    .send_text_with_keyboard(target, &text, msg_seq, self.markdown, &keyboard)
                    .await
                {
                    tracing::error!("[qq] permission send failed: {error}");
                }
                continue;
            }
            tracing::info!(
                "[qq] outbound event={} target={} text_len={}",
                event.kind_name(),
                target.kind.label(),
                event.plain_text().len()
            );
            for item in super::super::channel_delivery_items(
                event,
                super::super::ChannelCapabilities::with_media(
                    if self.markdown {
                        super::super::ChannelTextCapability::Markdown
                    } else {
                        super::super::ChannelTextCapability::Plain
                    },
                    super::super::ChannelCapabilities::IMAGE
                        | super::super::ChannelCapabilities::AUDIO
                        | super::super::ChannelCapabilities::VIDEO
                        | super::super::ChannelCapabilities::FILE,
                ),
            ) {
                match item {
                    super::super::ChannelDeliveryItem::Text { text, markdown } => {
                        if text.trim().is_empty() {
                            continue;
                        }
                        for chunk in super::super::split_message(&text, QQ_TEXT_LIMIT) {
                            msg_seq += 1;
                            if let Err(error) =
                                self.send_text(target, &chunk, msg_seq, markdown).await
                            {
                                tracing::error!("[qq] send failed: {error}");
                                return;
                            }
                        }
                    }
                    super::super::ChannelDeliveryItem::Media { attachment } => {
                        msg_seq += 1;
                        if let Err(error) = self.send_media(target, &attachment, msg_seq).await {
                            tracing::error!("[qq] media send failed: {error}");
                            return;
                        }
                    }
                }
            }
        }
    }

    pub(super) async fn send_text(
        &self,
        target: &ReplyTarget,
        text: &str,
        msg_seq: u32,
        markdown: bool,
    ) -> Result<(), String> {
        self.send_text_inner(target, text, msg_seq, markdown, None)
            .await
    }

    pub(super) async fn send_text_with_keyboard(
        &self,
        target: &ReplyTarget,
        text: &str,
        msg_seq: u32,
        markdown: bool,
        keyboard: &serde_json::Value,
    ) -> Result<(), String> {
        self.send_text_inner(target, text, msg_seq, markdown, Some(keyboard))
            .await
    }

    async fn send_text_inner(
        &self,
        target: &ReplyTarget,
        text: &str,
        msg_seq: u32,
        markdown: bool,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let token = self.ensure_access_token().await?;
        let path = match &target.kind {
            ReplyTargetKind::C2c { openid } => format!("/v2/users/{openid}/messages"),
            ReplyTargetKind::Group { group_openid } => {
                format!("/v2/groups/{group_openid}/messages")
            }
        };
        let mut body = if markdown {
            serde_json::json!({
                "markdown": {"content": text},
                "msg_type": QQ_MSG_TYPE_MARKDOWN,
                "msg_seq": if target.source_message_id.is_some() { msg_seq } else { 1 },
            })
        } else {
            serde_json::json!({
                "content": text,
                "msg_type": QQ_MSG_TYPE_TEXT,
                "msg_seq": if target.source_message_id.is_some() { msg_seq } else { 1 },
            })
        };
        if let Some(msg_id) = &target.source_message_id {
            body["msg_id"] = serde_json::Value::String(msg_id.clone());
        }
        if let Some(keyboard) = keyboard {
            body["keyboard"] = keyboard.clone();
        }
        let response = self
            .client
            .post(format!("{}{}", self.api_base(), path))
            .header("Authorization", format!("QQBot {token}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("failed to send QQ message: {e}"))?;
        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(format!("QQ send failed: {status} {body}"))
        }
    }

    async fn send_media(
        &self,
        target: &ReplyTarget,
        attachment: &cortex_types::Attachment,
        msg_seq: u32,
    ) -> Result<(), String> {
        let token = self.ensure_access_token().await?;
        let file_info = self.upload_media(&token, target, attachment).await?;
        let path = match &target.kind {
            ReplyTargetKind::C2c { openid } => format!("/v2/users/{openid}/messages"),
            ReplyTargetKind::Group { group_openid } => {
                format!("/v2/groups/{group_openid}/messages")
            }
        };
        let mut body = serde_json::json!({
            "msg_type": QQ_MSG_TYPE_MEDIA,
            "media": {"file_info": file_info},
            "msg_seq": if target.source_message_id.is_some() { msg_seq } else { 1 },
        });
        if let Some(caption) = attachment.caption.as_deref().map(str::trim)
            && !caption.is_empty()
        {
            body["content"] = serde_json::Value::String(caption.to_string());
        }
        if let Some(msg_id) = &target.source_message_id {
            body["msg_id"] = serde_json::Value::String(msg_id.clone());
        }

        let response = self
            .client
            .post(format!("{}{}", self.api_base(), path))
            .header("Authorization", format!("QQBot {token}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("failed to send QQ media message: {e}"))?;
        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(format!("QQ media send failed: {status} {body}"))
        }
    }

    async fn upload_media(
        &self,
        token: &str,
        target: &ReplyTarget,
        attachment: &cortex_types::Attachment,
    ) -> Result<String, String> {
        let file_type = super::qq_media_type(attachment)?;
        let path = match &target.kind {
            ReplyTargetKind::C2c { openid } => format!("/v2/users/{openid}/files"),
            ReplyTargetKind::Group { group_openid } => {
                format!("/v2/groups/{group_openid}/files")
            }
        };

        let mut body = serde_json::json!({
            "file_type": file_type,
            "srv_send_msg": false,
        });

        if super::is_remote_media_url(&attachment.url) {
            body["url"] = serde_json::Value::String(attachment.url.clone());
        } else {
            let data = std::fs::read(&attachment.url)
                .map_err(|e| format!("failed to read attachment {}: {e}", attachment.url))?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(data);
            body["file_data"] = serde_json::Value::String(encoded);
            if attachment.media_type == "file"
                && let Some(file_name) = std::path::Path::new(&attachment.url)
                    .file_name()
                    .and_then(|name| name.to_str())
            {
                body["file_name"] = serde_json::Value::String(file_name.to_string());
            }
        }

        let response = self
            .client
            .post(format!("{}{}", self.api_base(), path))
            .header("Authorization", format!("QQBot {token}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("failed to upload QQ media: {e}"))?;
        let status = response.status();
        let payload: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("failed to decode QQ media upload response: {e}"))?;
        if !status.is_success() {
            return Err(format!("QQ media upload failed: {status} {payload}"));
        }
        payload
            .get("file_info")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| format!("QQ media upload response missing file_info: {payload}"))
    }
}
