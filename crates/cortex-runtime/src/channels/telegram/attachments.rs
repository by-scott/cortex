use sha2::Digest;

use super::{MAX_DOWNLOAD_BYTES, TELEGRAM_API, TelegramChannel};

impl TelegramChannel {
    pub(super) fn default_prompt_for_attachments(
        attachments: &[cortex_types::Attachment],
    ) -> String {
        let types: Vec<&str> = attachments.iter().map(|a| a.media_type.as_str()).collect();
        if types.contains(&"image") {
            "The previous user message is an image attachment. Describe what you see in the image."
                .to_string()
        } else if types.contains(&"video") {
            "The user sent a video. Describe the content.".to_string()
        } else if types.contains(&"audio") {
            "The user sent an audio message.".to_string()
        } else {
            "The user sent a file.".to_string()
        }
    }

    /// Download a file from Telegram by `file_id`.
    ///
    /// Returns `(local_path, sha256)` on success.  Files are saved under
    /// `data/blobs/{hash16}.{ext}` inside the Cortex home directory.
    async fn download_telegram_file(&self, file_id: &str) -> Result<(String, String), String> {
        // 1. Resolve file_path via getFile
        let url = format!(
            "{TELEGRAM_API}/bot{}/getFile?file_id={file_id}",
            self.bot_token
        );
        let resp: serde_json::Value = self
            .api_client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let file_path = resp
            .get("result")
            .and_then(|r| r.get("file_path"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "getFile: missing file_path".to_string())?;

        // 2. Download the bytes
        let download_url = format!("{TELEGRAM_API}/file/bot{}/{file_path}", self.bot_token);
        let bytes = self
            .api_client
            .get(&download_url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;

        if bytes.len() > MAX_DOWNLOAD_BYTES {
            return Err(format!(
                "file too large ({} bytes, max {MAX_DOWNLOAD_BYTES})",
                bytes.len()
            ));
        }

        // 3. Save to data/blobs/{hash16}.{ext}
        let hash_full = hex::encode(sha2::Sha256::digest(&bytes));
        let hash = &hash_full[..16];
        let ext = file_path.rsplit('.').next().unwrap_or("bin");
        let blob_dir =
            cortex_kernel::CortexPaths::from_instance_home(self.state.home()).blobs_dir();
        let local = blob_dir.join(format!("{hash}.{ext}"));
        std::fs::create_dir_all(&blob_dir).map_err(|e| e.to_string())?;
        std::fs::write(&local, &bytes).map_err(|e| e.to_string())?;

        Ok((local.to_string_lossy().to_string(), hash_full))
    }

    /// Extract a voice attachment: download and transcribe via STT.
    async fn extract_voice_attachment(
        &self,
        voice: &serde_json::Value,
    ) -> Option<cortex_types::Attachment> {
        let file_id = voice
            .get("file_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let (path, sha256) = self.download_telegram_file(file_id).await.ok()?;
        let (media_config, api_key) = self.resolve_media_config();
        let transcript = crate::media::stt::transcribe(
            &media_config,
            media_config.stt_key(&api_key),
            &path,
            &self.api_client,
        )
        .await
        .unwrap_or_default();
        let mut attachment = cortex_types::Attachment::new(
            "audio",
            voice
                .get("mime_type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("audio/ogg"),
            path,
        )
        .with_taint(cortex_types::MediaTaint::External)
        .with_source_uri(format!("telegram:file:{file_id}"))
        .with_media_id(format!("telegram:{file_id}"))
        .with_sha256(sha256);
        if !transcript.is_empty() {
            attachment = attachment.with_caption(transcript);
        }
        if let Some(size) = voice.get("file_size").and_then(serde_json::Value::as_u64) {
            attachment = attachment.with_size(size);
        }
        Some(attachment)
    }

    /// Extract a video attachment.
    async fn extract_video_attachment(
        &self,
        video: &serde_json::Value,
    ) -> Option<cortex_types::Attachment> {
        let file_id = video
            .get("file_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let (path, sha256) = self.download_telegram_file(file_id).await.ok()?;
        let mut attachment = cortex_types::Attachment::new(
            "video",
            video
                .get("mime_type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("video/mp4"),
            path,
        )
        .with_taint(cortex_types::MediaTaint::External)
        .with_source_uri(format!("telegram:file:{file_id}"))
        .with_media_id(format!("telegram:{file_id}"))
        .with_sha256(sha256);
        if let Some(size) = video.get("file_size").and_then(serde_json::Value::as_u64) {
            attachment = attachment.with_size(size);
        }
        Some(attachment)
    }

    /// Get media config + API key without holding `RwLockReadGuard` across awaits.
    fn resolve_media_config(&self) -> (cortex_types::config::MediaConfig, String) {
        let cfg = self.state.config();
        let mc = cfg.media.clone();
        let api_key = cfg.api.api_key.clone();
        drop(cfg);
        (mc, api_key)
    }

    /// Extract a photo attachment (largest size from the array).
    async fn extract_photo_attachment(
        &self,
        photos: &[serde_json::Value],
    ) -> Option<cortex_types::Attachment> {
        let largest = photos.last()?;
        let file_id = largest
            .get("file_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let (path, sha256) = self.download_telegram_file(file_id).await.ok()?;

        let mut attachment = cortex_types::Attachment::new("image", "image/jpeg", path)
            .with_taint(cortex_types::MediaTaint::External)
            .with_source_uri(format!("telegram:file:{file_id}"))
            .with_media_id(format!("telegram:{file_id}"))
            .with_sha256(sha256);
        if let Some(size) = largest.get("file_size").and_then(serde_json::Value::as_u64) {
            attachment = attachment.with_size(size);
        }
        Some(attachment)
    }

    /// Extract a document attachment.
    async fn extract_document_attachment(
        &self,
        doc: &serde_json::Value,
    ) -> Option<cortex_types::Attachment> {
        let file_id = doc
            .get("file_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let (path, sha256) = self.download_telegram_file(file_id).await.ok()?;
        let mut attachment = cortex_types::Attachment::new(
            "file",
            doc.get("mime_type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("application/octet-stream"),
            path,
        )
        .with_taint(cortex_types::MediaTaint::External)
        .with_source_uri(format!("telegram:file:{file_id}"))
        .with_media_id(format!("telegram:{file_id}"))
        .with_sha256(sha256);
        if let Some(caption) = doc
            .get("file_name")
            .and_then(serde_json::Value::as_str)
            .map(String::from)
        {
            attachment = attachment.with_caption(caption);
        }
        if let Some(size) = doc.get("file_size").and_then(serde_json::Value::as_u64) {
            attachment = attachment.with_size(size);
        }
        Some(attachment)
    }

    /// Extract multimedia attachments from a Telegram message object.
    pub(super) async fn extract_attachments(
        &self,
        msg: &serde_json::Value,
    ) -> Vec<cortex_types::Attachment> {
        let mut attachments = Vec::new();

        if let Some(photos) = msg.get("photo").and_then(serde_json::Value::as_array)
            && let Some(att) = self.extract_photo_attachment(photos).await
        {
            attachments.push(att);
        }

        if let Some(voice) = msg.get("voice")
            && let Some(att) = self.extract_voice_attachment(voice).await
        {
            attachments.push(att);
        }

        if let Some(video) = msg.get("video")
            && let Some(att) = self.extract_video_attachment(video).await
        {
            attachments.push(att);
        }

        if let Some(doc) = msg.get("document")
            && let Some(att) = self.extract_document_attachment(doc).await
        {
            attachments.push(att);
        }

        let mut enriched = Vec::with_capacity(attachments.len());
        for attachment in attachments {
            enriched.push(
                super::super::enrich_inbound_attachment(&self.state, &self.api_client, attachment)
                    .await,
            );
        }
        enriched
    }
}
