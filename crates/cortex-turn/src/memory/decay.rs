const DEPRECATION_THRESHOLD: f64 = 0.05;
const ACCESS_WEIGHT: f64 = 0.1;

/// Compute memory freshness.
///
/// Formula: `min(1.0, exp(-decay_rate * hours) + ln(1 + access_count) * 0.1)`
///
/// Higher access count provides resistance against decay.
#[must_use]
pub fn freshness(hours_since_last_access: f64, access_count: u32, decay_rate: f64) -> f64 {
    let time_decay = (-decay_rate * hours_since_last_access).exp();
    let access_bonus = f64::from(access_count).ln_1p() * ACCESS_WEIGHT;
    (time_decay + access_bonus).min(1.0)
}

/// Check if a memory should be deprecated based on freshness.
#[must_use]
pub fn should_deprecate(hours_since_last_access: f64, access_count: u32, decay_rate: f64) -> bool {
    freshness(hours_since_last_access, access_count, decay_rate) < DEPRECATION_THRESHOLD
}
