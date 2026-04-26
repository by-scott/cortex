use std::collections::BTreeMap;

use cortex_types::{ActorId, AuthContext, ClientId, TenantId, TransportCapabilities};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedClient {
    pub context: AuthContext,
    pub capabilities: TransportCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngressCredential {
    context: AuthContext,
    capabilities: TransportCapabilities,
    digest_hex: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IngressRegistry {
    credentials: BTreeMap<IngressKey, IngressCredential>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngressError {
    EmptyToken,
    InvalidToken,
    UnknownClient,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct IngressKey {
    tenant: TenantId,
    actor: ActorId,
    client: ClientId,
}

impl IngressRegistry {
    /// # Errors
    /// Returns an error when the supplied bearer token is empty.
    pub fn register(
        &mut self,
        context: AuthContext,
        token: &str,
        capabilities: TransportCapabilities,
    ) -> Result<(), IngressError> {
        if token.is_empty() {
            return Err(IngressError::EmptyToken);
        }
        let key = IngressKey::from_context(&context);
        self.credentials.insert(
            key,
            IngressCredential {
                context,
                capabilities,
                digest_hex: digest_token(token),
            },
        );
        Ok(())
    }

    /// # Errors
    /// Returns an error when the client is unknown or the bearer token does not
    /// match the stored digest.
    pub fn authenticate(
        &self,
        tenant_id: &TenantId,
        actor_id: &ActorId,
        client_id: &ClientId,
        token: &str,
    ) -> Result<AuthenticatedClient, IngressError> {
        let key = IngressKey {
            tenant: tenant_id.clone(),
            actor: actor_id.clone(),
            client: client_id.clone(),
        };
        let Some(credential) = self.credentials.get(&key) else {
            return Err(IngressError::UnknownClient);
        };
        if constant_time_eq(
            credential.digest_hex.as_bytes(),
            digest_token(token).as_bytes(),
        ) {
            Ok(AuthenticatedClient {
                context: credential.context.clone(),
                capabilities: credential.capabilities.clone(),
            })
        } else {
            Err(IngressError::InvalidToken)
        }
    }

    #[must_use]
    pub fn registered_clients(&self) -> usize {
        self.credentials.len()
    }
}

impl IngressKey {
    fn from_context(context: &AuthContext) -> Self {
        Self {
            tenant: context.tenant_id.clone(),
            actor: context.actor_id.clone(),
            client: context.client_id.clone(),
        }
    }
}

fn digest_token(token: &str) -> String {
    hex_lower(&Sha256::digest(token.as_bytes()))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();
    for index in 0..max_len {
        let left_byte = left.get(index).copied().unwrap_or_default();
        let right_byte = right.get(index).copied().unwrap_or_default();
        diff |= usize::from(left_byte ^ right_byte);
    }
    diff == 0
}
