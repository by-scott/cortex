use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResult {
    pub overall_pass: bool,
    pub checks: Vec<CheckResult>,
    pub rolled_back: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateCheckResult {
    pub passed: bool,
    pub checks: Vec<CheckResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_result_roundtrip() {
        let r = VerifyResult {
            overall_pass: true,
            checks: vec![CheckResult {
                name: "fmt".into(),
                passed: true,
                output: String::new(),
            }],
            rolled_back: false,
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: VerifyResult = serde_json::from_str(&json).unwrap();
        assert!(back.overall_pass);
    }
}
