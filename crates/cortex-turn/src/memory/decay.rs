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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_is_high() {
        let f = freshness(0.0, 0, 0.05);
        assert!(f > 0.9);
    }

    #[test]
    fn old_is_low() {
        let f = freshness(500.0, 0, 0.05);
        assert!(f < 0.2);
    }

    #[test]
    fn access_resists_decay() {
        let f_no_access = freshness(100.0, 0, 0.05);
        let f_with_access = freshness(100.0, 50, 0.05);
        assert!(f_with_access > f_no_access);
    }

    #[test]
    fn capped_at_one() {
        let f = freshness(0.0, 1000, 0.05);
        assert!((f - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn deprecate_old_unused() {
        assert!(should_deprecate(2000.0, 0, 0.05));
    }

    #[test]
    fn no_deprecate_recent() {
        assert!(!should_deprecate(1.0, 5, 0.05));
    }
}
