use cortex_turn::context::SummaryCache;
use cortex_turn::meta::MetaMonitor;
use cortex_types::{Message as CortexMessage, SessionMetadata};

pub struct DaemonSession {
    pub meta: SessionMetadata,
    pub history: Vec<CortexMessage>,
    pub turn_count: usize,
    pub turns_since_extract: usize,
    pub monitor: MetaMonitor,
    pub summary_cache: SummaryCache,
}

pub fn restore_failed_turn_history(
    history: &mut Vec<CortexMessage>,
    history_len_before_turn: usize,
    input: &crate::turn_executor::TurnInput<'_>,
    error: &str,
) {
    history.truncate(history_len_before_turn);
    history.push(failed_turn_user_message(input));
    history.push(CortexMessage::assistant(format!(
        "Turn failed before a final assistant response. Error: {error}"
    )));
}

fn failed_turn_user_message(input: &crate::turn_executor::TurnInput<'_>) -> CortexMessage {
    let mut message = if input.inline_images.is_empty() {
        CortexMessage::user(input.text)
    } else {
        CortexMessage::user_with_images(input.text, input.inline_images.to_vec())
    };
    message.attachments = input.attachments.to_vec();
    message
}
