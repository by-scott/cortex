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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_validate_token() {
        let secret = "test-secret-key";
        let token = create_token(secret, 1).unwrap();
        let claims = validate_token(&token, secret).unwrap();
        assert_eq!(claims.sub, "cortex");
        assert!(claims.exp > claims.iat);
        assert!(claims.provider.is_none());
        assert!(claims.user_id.is_none());
    }

    #[test]
    fn test_wrong_secret_rejected() {
        let token = create_token("correct-secret", 1).unwrap();
        let result = validate_token(&token, "wrong-secret");
        assert!(result.is_err());
    }

    #[test]
    fn test_expired_token_rejected() {
        let secret = "test-secret";
        let now = jsonwebtoken::get_current_timestamp();
        let claims = Claims {
            sub: "cortex".into(),
            iat: now - 7200,
            exp: now - 3600,
            provider: None,
            user_id: None,
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();
        let result = validate_token(&token, secret);
        assert!(result.is_err());
    }

    #[test]
    fn test_malformed_token_rejected() {
        let result = validate_token("not-a-jwt", "secret");
        assert!(result.is_err());
    }

    #[test]
    fn test_create_oauth_token() {
        let secret = "test-secret";
        let token = create_oauth_token(secret, 1, "github", "octocat").unwrap();
        let claims = validate_token(&token, secret).unwrap();
        assert_eq!(claims.sub, "octocat");
        assert_eq!(claims.provider, Some("github".into()));
        assert_eq!(claims.user_id, Some("octocat".into()));
    }

    #[test]
    fn test_old_token_without_oauth_fields_still_valid() {
        #[derive(Serialize)]
        struct OldClaims {
            sub: String,
            iat: u64,
            exp: u64,
        }

        let secret = "test-secret";
        let now = jsonwebtoken::get_current_timestamp();
        let old_claims = OldClaims {
            sub: "cortex".into(),
            iat: now,
            exp: now + 3600,
        };
        let token = encode(
            &Header::default(),
            &old_claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();

        let claims = validate_token(&token, secret).unwrap();
        assert_eq!(claims.sub, "cortex");
        assert!(claims.provider.is_none());
        assert!(claims.user_id.is_none());
    }

    #[test]
    fn test_oauth_state_store() {
        let store = new_oauth_state_store();
        let state = "test-state-123".to_string();

        {
            let mut s = store.write().unwrap();
            s.insert(state.clone(), Instant::now());
        }

        {
            let contains = store.read().unwrap().contains_key(&state);
            assert!(contains);
        }

        {
            let mut s = store.write().unwrap();
            let first_remove = s.remove(&state).is_some();
            let second_remove = s.remove(&state).is_none();
            drop(s);
            assert!(first_remove);
            assert!(second_remove);
        }
    }

    #[test]
    fn test_urlencoding() {
        assert_eq!(urlencoding("hello"), "hello");
        assert_eq!(urlencoding("hello world"), "hello%20world");
        assert_eq!(urlencoding("user:email"), "user%3Aemail");
    }
}
