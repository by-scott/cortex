//! Pairing authentication for messaging channels.

use super::store::{ChannelStore, PendingPair};

/// Action to take after checking a user's pairing status.
pub enum PairingAction {
    /// User is paired -- proceed with message handling.
    Allowed,
    /// User needs admin approval. Contains message to send.
    SendPairingPrompt(String),
    /// User is blacklisted or policy denies access.
    Denied,
}

/// Check if a user is allowed to interact. Returns the action to take.
#[must_use]
pub fn check_user(
    store: &ChannelStore,
    user_id: &str,
    user_name: &str,
    platform: &str,
) -> PairingAction {
    let policy = store.policy();

    // Blacklist takes highest priority
    if policy.blacklist.iter().any(|b| b == user_id) {
        return PairingAction::Denied;
    }

    // Whitelist bypasses pairing in all modes
    if policy.whitelist.iter().any(|w| w == user_id) {
        return PairingAction::Allowed;
    }

    match policy.mode.as_str() {
        "open" => PairingAction::Allowed,
        "whitelist" => {
            if store.is_paired(user_id) {
                PairingAction::Allowed
            } else {
                PairingAction::Denied
            }
        }
        _ => check_pairing_mode(store, &policy, user_id, user_name, platform),
    }
}

/// Handle the default "pairing" mode logic.
fn check_pairing_mode(
    store: &ChannelStore,
    policy: &super::store::ChannelPolicy,
    user_id: &str,
    user_name: &str,
    session_prefix: &str,
) -> PairingAction {
    if store.is_paired(user_id) {
        return PairingAction::Allowed;
    }

    // Already pending — remind to wait
    let pending = store.pending_pairs();
    if let Some(existing) = pending.iter().find(|p| p.user_id == user_id) {
        return PairingAction::SendPairingPrompt(pairing_prompt(
            session_prefix,
            user_id,
            &existing.code,
            true,
        ));
    }

    // New user -- generate pairing code
    let code = generate_pair_code();
    let mut pending = pending;

    // Enforce max pending limit
    if pending.len() >= policy.max_pending {
        pending.remove(0);
    }

    pending.push(PendingPair {
        user_id: user_id.to_string(),
        user_name: user_name.to_string(),
        code: code.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    });
    store.save_pending_pairs(&pending);

    tracing::info!(
        user_id = user_id,
        user_name = user_name,
        code = code.as_str(),
        "New pairing request. Approve with: cortex channel approve {platform} {user_id}; approve and subscribe with: cortex channel approve {platform} {user_id} --subscribe",
        platform = session_prefix,
    );

    PairingAction::SendPairingPrompt(pairing_prompt(session_prefix, user_id, &code, false))
}

fn pairing_prompt(platform: &str, user_id: &str, code: &str, pending: bool) -> String {
    let intro = if pending {
        "Your pairing request is already pending."
    } else {
        "Welcome! This Cortex instance requires pairing."
    };
    format!(
        "{intro}\n\n\
         Pairing code: {code}\n\n\
         Ask the administrator to run one of:\n\
         Pair only: `cortex channel approve {platform} {user_id}`\n\
         Pair and subscribe: `cortex channel approve {platform} {user_id} --subscribe`"
    )
}

fn generate_pair_code() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    let n = hasher.finish();
    let chars: Vec<char> = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789".chars().collect();
    let len = chars.len();
    (0..6)
        .map(|i| chars[((n >> (i * 5)) & 0x1F) as usize % len])
        .collect()
}
