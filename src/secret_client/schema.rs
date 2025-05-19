// This crate now delegates IPC message schemas to the shared `ipc_types` crate.
// Re-export the needed types so existing callers don't have to change paths immediately.

// Adjust path to top-level ipc_types module
pub use crate::ipc_types::{
    ClientRequest, GetSecretRequest, RotatedNotification, RotationAckRequest as RotationAck,
    ServerResponse,
};

// SecretClient's custom wrappers that include `secrecy::SecretString` stay here.
// Note: SecretString import is unused now, as we deserialize to String.
use serde::{Deserialize, Serialize};

/// Response to GetSecret request
#[derive(Debug, Clone, Deserialize)]
pub struct GetSecretResponse {
    /// The name of the secret that was requested
    pub name: String,
    /// The secret value
    pub value: String,
    /// Unique ID for rotation tracking (if part of rotation flow)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation_id: Option<String>,
}

// Implement Serialize manually to avoid leaking the secret contents when the module accidentally
// logs the struct.
impl Serialize for GetSecretResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("GetSecretResponse", 3)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("value", "[REDACTED]")?;
        state.serialize_field("rotation_id", &self.rotation_id)?;
        state.end()
    }
}
