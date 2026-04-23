use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

// ── JWT Claims ────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iat: u64,
    pub exp: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

// ── OAuth state store ─────────────────────────────────────

pub type OAuthStateStore = Arc<RwLock<HashMap<String, Instant>>>;

#[must_use]
pub fn new_oauth_state_store() -> OAuthStateStore {
    Arc::new(RwLock::new(HashMap::new()))
}

// ── Token issuance ────────────────────────────────────────

/// Create a JWT token with the given secret and expiry.
///
/// # Errors
/// Returns an error string if JWT encoding fails.
pub fn create_token(secret: &str, expiry_hours: u64) -> Result<String, String> {
    let now = jsonwebtoken::get_current_timestamp();
    let claims = Claims {
        sub: "cortex".into(),
        iat: now,
        exp: now + expiry_hours * 3600,
        provider: None,
        user_id: None,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| format!("token encoding error: {e}"))
}

/// Create an OAuth JWT token with provider and user info.
///
/// # Errors
/// Returns an error string if JWT encoding fails.
pub fn create_oauth_token(
    secret: &str,
    expiry_hours: u64,
    provider: &str,
    user_id: &str,
) -> Result<String, String> {
    let now = jsonwebtoken::get_current_timestamp();
    let claims = Claims {
        sub: user_id.into(),
        iat: now,
        exp: now + expiry_hours * 3600,
        provider: Some(provider.into()),
        user_id: Some(user_id.into()),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| format!("token encoding error: {e}"))
}

/// Validate a JWT token and return the decoded claims.
///
/// # Errors
/// Returns an error string if decoding or validation fails.
pub fn validate_token(token: &str, secret: &str) -> Result<Claims, String> {
    let mut validation = Validation::default();
    validation.set_required_spec_claims(&["sub", "exp", "iat"]);
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|e| e.to_string())
}

/// Simple URL encoding for query parameter values.
#[must_use]
pub fn urlencoding(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(char::from(b));
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(result, "%{b:02X}");
            }
        }
    }
    result
}

/// Generate a pseudo-random u64 using std (no external dependency).
#[must_use]
pub fn rand_u64() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    Instant::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    hasher.finish()
}

// ── Tests ─────────────────────────────────────────────────
