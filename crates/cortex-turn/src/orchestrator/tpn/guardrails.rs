use cortex_types::{Attachment, MediaTaint, Payload};

use crate::guardrails::{ExternalContentSource, assess_external_content};

pub fn external_input_observed_payload(tool_name: &str, output: &str) -> Payload {
    let assessment = assess_external_content(ExternalContentSource::ToolOutput, output);
    let summary = assessment.summary_for_journal(output);
    Payload::ExternalInputObserved {
        source: format!("tool:{tool_name}"),
        trust: assessment.journal_trust().to_string(),
        summary,
    }
}

pub fn tool_output_guardrail_payload(tool_name: &str, output: &str) -> Option<Payload> {
    let assessment = assess_external_content(ExternalContentSource::ToolOutput, output);
    if let Some(finding) = assessment.finding {
        Some(Payload::GuardrailTriggered {
            category: format!("{:?}", finding.category),
            reason: finding.reason,
            source: format!("tool_output:{tool_name}"),
        })
    } else {
        None
    }
}

pub fn untrusted_tool_result_for_history(tool_name: &str, output: &str) -> String {
    let assessment = assess_external_content(ExternalContentSource::ToolOutput, output);
    let safe_output = assessment.safe_evidence_text(output);
    format!("[tool-output:{tool_name}; trust=untrusted; instructions=inert]\n{safe_output}")
}

pub(super) fn sdk_attachment_to_core(attachment: cortex_sdk::Attachment) -> Attachment {
    let mut converted =
        Attachment::new(attachment.media_type, attachment.mime_type, attachment.url)
            .with_taint(MediaTaint::Generated);
    if let Some(caption) = attachment.caption {
        converted = converted.with_caption(caption);
    }
    if let Some(size) = attachment.size {
        converted = converted.with_size(size);
    }
    converted
}
