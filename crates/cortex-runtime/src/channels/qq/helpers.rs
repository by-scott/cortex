pub(in crate::channels::qq) fn qq_media_type(
    attachment: &cortex_types::Attachment,
) -> Result<i64, String> {
    match attachment.media_type.as_str() {
        "image" => Ok(1),
        "video" => Ok(2),
        "audio" => Ok(3),
        "file" => Ok(4),
        other => Err(format!("unsupported QQ media type: {other}")),
    }
}

pub(in crate::channels::qq) fn is_remote_media_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("data:")
}

pub(in crate::channels::qq) trait BroadcastEventExt {
    fn kind_name(&self) -> &'static str;
}

impl BroadcastEventExt for crate::daemon::BroadcastEvent {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Observer { .. } => "observer",
            Self::Boundary => "boundary",
            Self::Trace { .. } => "trace",
            Self::Done { .. } => "done",
            Self::Error(_) => "error",
            Self::PermissionRequested(_) => "permission",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::channels::qq) enum QqInboundRoute {
    SendPairingPrompt,
    Denied,
    SlashCommand,
    Turn,
}

pub(in crate::channels::qq) fn qq_inbound_route(
    text: &str,
    pairing_action: &super::super::pairing::PairingAction,
) -> QqInboundRoute {
    match pairing_action {
        super::super::pairing::PairingAction::Allowed if text.starts_with('/') => {
            QqInboundRoute::SlashCommand
        }
        super::super::pairing::PairingAction::Allowed => QqInboundRoute::Turn,
        super::super::pairing::PairingAction::SendPairingPrompt(_) => {
            QqInboundRoute::SendPairingPrompt
        }
        super::super::pairing::PairingAction::Denied => QqInboundRoute::Denied,
    }
}

pub(in crate::channels::qq) fn qq_reply_message_id(data: &serde_json::Value) -> Option<String> {
    ["msg_id", "message_id", "id", "event_id"]
        .into_iter()
        .find_map(|key| data.get(key).and_then(serde_json::Value::as_str))
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
}

pub(in crate::channels::qq) fn strip_self_mentions(
    text: &str,
    mentions: Option<&serde_json::Value>,
) -> String {
    let mut cleaned = text.to_string();
    let Some(mentions) = mentions.and_then(serde_json::Value::as_array) else {
        return cleaned.trim().to_string();
    };
    for mention in mentions {
        let openid = mention
            .get("member_openid")
            .or_else(|| mention.get("id"))
            .or_else(|| mention.get("user_openid"))
            .and_then(serde_json::Value::as_str);
        let Some(openid) = openid else {
            continue;
        };
        if mention.get("is_you").and_then(serde_json::Value::as_bool) == Some(true) {
            cleaned = cleaned.replace(&format!("<@{openid}>"), "");
            cleaned = cleaned.replace(&format!("<@!{openid}>"), "");
        } else if let Some(name) = mention
            .get("nickname")
            .or_else(|| mention.get("username"))
            .and_then(serde_json::Value::as_str)
        {
            cleaned = cleaned.replace(&format!("<@{openid}>"), &format!("@{name}"));
            cleaned = cleaned.replace(&format!("<@!{openid}>"), &format!("@{name}"));
        }
    }
    cleaned.trim().to_string()
}
