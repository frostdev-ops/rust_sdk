/// Events emitted by `SecretProvider` implementations that support watching.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecretEvent {
    /// Indicates that one or more secrets have been updated or rotated.
    /// Contains a list of the keys that were affected.
    Rotated(Vec<String>),
    /// Indicates that a new secret key has been added.
    Added(String),
    /// Indicates that an existing secret key has been removed.
    Removed(String),
}
