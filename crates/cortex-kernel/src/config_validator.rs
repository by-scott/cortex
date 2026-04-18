use cortex_types::Payload;
use cortex_types::config::CortexConfig;

const TOTAL_CHECKS: u32 = 5;

/// Validate config and return a list of warnings.
#[must_use]
pub fn validate(config: &CortexConfig) -> Vec<String> {
    let mut warnings = Vec::new();

    if config.api.provider.is_empty() {
        warnings.push("api.provider is empty".into());
    }
    if config.api.max_tokens == 0 && config.context.max_tokens == 0 {
        warnings.push("no max_tokens configured (api or context)".into());
    }
    if config.embedding.provider.is_empty() {
        warnings.push("embedding.provider is empty".into());
    }
    if config.memory.consolidate_interval_hours == 0 {
        warnings.push("memory.consolidate_interval_hours is 0".into());
    }

    warnings
}

/// Compute config health score and emit a `ConfigValidated` event payload.
#[must_use]
pub fn config_health(config: &CortexConfig) -> (f64, Vec<String>, Payload) {
    let warnings = validate(config);
    let warning_count = warnings.len().min(TOTAL_CHECKS as usize);
    let score =
        1.0 - f64::from(u32::try_from(warning_count).unwrap_or(u32::MAX)) / f64::from(TOTAL_CHECKS);
    let payload = Payload::ConfigValidated {
        warning_count,
        health_score: score,
    };
    (score, warnings, payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_healthy() {
        let config = CortexConfig::default();
        let (score, warnings, _) = config_health(&config);
        // Default config has sensible defaults for all fields
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn perfect_health() {
        let mut config = CortexConfig::default();
        config.api.provider = "anthropic".into();
        config.api.model = "claude-sonnet-4-20250514".into();
        config.embedding.provider = "ollama".into();
        config.memory.consolidate_interval_hours = 24;
        let (score, warnings, _) = config_health(&config);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn health_degrades_with_warnings() {
        let mut config = CortexConfig::default();
        config.embedding.provider = String::new(); // force a warning
        let (score, warnings, _) = config_health(&config);
        assert!(!warnings.is_empty());
        assert!(score < 1.0);
    }
}
