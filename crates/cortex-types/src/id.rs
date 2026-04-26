use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! branded_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(format!("{}-{}", $prefix, Uuid::now_v7()))
            }

            #[must_use]
            pub fn from_static(value: &'static str) -> Self {
                Self(value.to_string())
            }

            #[must_use]
            pub fn from_raw(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            #[must_use]
            pub const fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_tuple(stringify!($name)).field(&self.0).finish()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

branded_id!(TenantId, "tenant");
branded_id!(ActorId, "actor");
branded_id!(ClientId, "client");
branded_id!(SessionId, "session");
branded_id!(TurnId, "turn");
branded_id!(EventId, "event");
branded_id!(DeliveryId, "delivery");
branded_id!(PermissionRequestId, "permission");
branded_id!(CorpusId, "corpus");
