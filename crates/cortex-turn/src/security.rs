use cortex_types::Payload;
use regex::Regex;
use std::sync::LazyLock;

/// Compiled patterns for sensitive data detection.
static SENSITIVE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r#"(?i)(api[_-]?key|token|secret|password|authorization:\s*bearer)\s*[:=]\s*['"]?([A-Za-z0-9_\-.]{20,})['"]?"#,
        r"(?i)bearer\s+([A-Za-z0-9_\-.]{20,})",
        r#"(?i)(key|secret|token|password)\s*[:=]\s*['"]?([A-Fa-f0-9]{32,})['"]?"#,
    ]
    .iter()
    .filter_map(|p| Regex::new(p).ok())
    .collect()
});

/// Compiled advanced prompt injection patterns.
static ADVANCED_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (r"(?i)\bsystem\s*:\s*you\s+are\b", "system role override"),
        (
            r"(?i)\bignore\s+(all\s+)?above\b",
            "ignore-above instruction",
        ),
        (
            r"(?i)\b(override|bypass)\s+(safety|filter|guard)",
            "safety bypass attempt",
        ),
        (
            r"(?i)\brepeat\s+(everything|all)\s+(above|before)\b",
            "prompt extraction attempt",
        ),
        (r"(?i)\bnew\s+system\s+prompt\b", "system prompt injection"),
        (
            r"(?i)\b(translate|convert)\s+.{0,30}\s+instructions\b",
            "instruction extraction via translation",
        ),
        (
            r"(?is)<!--\s*system\s*:.*?-->",
            "html comment role override",
        ),
        (
            r"(?is)\A---\s*.*?\bsystem\s*:\s*.*?---",
            "front matter role override",
        ),
    ]
    .iter()
    .filter_map(|(p, desc)| Regex::new(p).ok().map(|r| (r, *desc)))
    .collect()
});

/// Sanitize text by redacting sensitive patterns.
///
/// Returns (`sanitized_text`, `count_of_redactions`).
#[must_use]
pub fn sanitize(text: &str) -> (String, usize) {
    let mut result = text.to_string();
    let mut count = 0;

    for pattern in SENSITIVE_PATTERNS.iter() {
        let before = result.clone();
        result = pattern.replace_all(&result, "${1} [REDACTED]").to_string();
        if result != before {
            count += 1;
        }
    }

    (result, count)
}

/// Create a `SecuritySanitized` event if redactions occurred.
#[must_use]
pub fn sanitize_with_event(text: &str) -> (String, Option<Payload>) {
    let (sanitized, count) = sanitize(text);
    let event = if count > 0 {
        Some(Payload::SecuritySanitized {
            redacted_count: count,
        })
    } else {
        None
    };
    (sanitized, event)
}

/// Compute a deterministic integrity hash for a [`Payload`].
///
/// Uses `FNV-1a` 64-bit hash of the JSON serialization.
#[must_use]
pub fn event_integrity_hash(payload: &Payload) -> String {
    let json = serde_json::to_string(payload).unwrap_or_default();
    let hash = fnv1a_hash(json.as_bytes());
    format!("{hash:016x}")
}

/// `FNV-1a` 64-bit hash.
fn fnv1a_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Detect prompt injection patterns beyond basic guardrails.
///
/// Returns a description of the detected pattern, or `None` if clean.
#[must_use]
pub fn detect_prompt_injection(input: &str) -> Option<String> {
    for (regex, description) in ADVANCED_PATTERNS.iter() {
        if regex.is_match(input) {
            return Some((*description).to_string());
        }
    }
    None
}
