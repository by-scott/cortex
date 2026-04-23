/// Estimate cost in USD for a model invocation.
#[must_use]
pub fn estimate_cost(model: &str, input_tokens: usize, output_tokens: usize) -> f64 {
    let Some((input_rate, output_rate)) = model_pricing(model) else {
        return 0.0;
    };
    let input = f64::from(u32::try_from(input_tokens).unwrap_or(u32::MAX));
    let output = f64::from(u32::try_from(output_tokens).unwrap_or(u32::MAX));
    input.mul_add(input_rate, output * output_rate) / 1_000_000.0
}

/// Returns (`input_per_million_usd`, `output_per_million_usd`) for known models.
fn model_pricing(model: &str) -> Option<(f64, f64)> {
    // Prefix matching, ordered most-specific-first
    let table: &[(&str, f64, f64)] = &[
        ("claude-opus", 15.0, 75.0),
        ("claude-sonnet", 3.0, 15.0),
        ("claude-haiku", 0.25, 1.25),
        ("gpt-5.4-mini", 0.15, 0.60),
        ("gpt-5.4", 2.50, 10.0),
        ("o4-mini", 1.10, 4.40),
        ("glm-5", 1.0, 3.2),
        ("glm-4-flash", 0.01, 0.01),
        ("kimi-k2", 0.6, 2.5),
    ];

    // Exact matches first
    let exact: &[(&str, f64, f64)] = &[("o3-pro", 20.0, 80.0), ("o3", 10.0, 40.0)];

    for &(name, input, output) in exact {
        if model == name {
            return Some((input, output));
        }
    }
    for &(prefix, input, output) in table {
        if model.starts_with(prefix) {
            return Some((input, output));
        }
    }
    None
}
