const CORRECTION_MARKERS: &[&str] = &[
    "i apologize",
    "i was wrong",
    "let me correct",
    "that's incorrect",
    "that was incorrect",
    "i made a mistake",
    "sorry, that",
    "actually, i should",
];

const PREFERENCE_PATTERNS: &[&str] = &[
    "i prefer",
    "don't do",
    "please stop",
    "always ",
    "never ",
    "i like when",
    "i don't like",
    "from now on",
    "going forward",
    "in the future",
];

const MIN_KEYWORD_LEN: usize = 4;
const MIN_NEW_KEYWORDS: usize = 2;

/// Detect if the assistant's response contains a self-correction signal.
#[must_use]
pub fn detect_correction(response: &str) -> bool {
    let lower = response.to_lowercase();
    CORRECTION_MARKERS.iter().any(|m| lower.contains(m))
}

/// Detect if the user's input expresses a preference signal.
#[must_use]
pub fn detect_preference(input: &str) -> bool {
    let lower = input.to_lowercase();
    PREFERENCE_PATTERNS.iter().any(|p| lower.contains(p))
}

/// Detect if the user's input introduces a new domain (keywords absent from profile).
#[must_use]
pub fn detect_new_domain(input: &str, user_profile: &str) -> bool {
    let profile_lower = user_profile.to_lowercase();
    let keywords = extract_keywords(input);
    let new_count = keywords
        .iter()
        .filter(|k| !profile_lower.contains(k.as_str()))
        .count();
    new_count >= MIN_NEW_KEYWORDS
}

fn extract_keywords(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|w| w.len() >= MIN_KEYWORD_LEN)
        .filter_map(|w| {
            let lower = w.to_lowercase();
            if seen.insert(lower.clone()) {
                Some(lower)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correction_detected() {
        assert!(detect_correction("I apologize for the confusion"));
    }

    #[test]
    fn correction_not_detected() {
        assert!(!detect_correction("Here is the result"));
    }

    #[test]
    fn preference_detected() {
        assert!(detect_preference("I prefer using TypeScript"));
    }

    #[test]
    fn preference_not_detected() {
        assert!(!detect_preference("Show me the code"));
    }

    #[test]
    fn new_domain_detected() {
        assert!(detect_new_domain(
            "I'm working on kubernetes deployment with helm charts",
            "python developer focused on data science"
        ));
    }

    #[test]
    fn known_domain_not_detected() {
        assert!(!detect_new_domain(
            "python data science analysis",
            "python developer focused on data science analysis"
        ));
    }
}
