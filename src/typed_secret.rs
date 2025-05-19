use crate::secret_client::{RequestMode, SecretClient, SecretError};
use secrecy::ExposeSecret;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

/// A type-safe wrapper for secret values.
///
/// `Secret<T>` provides a safe way to store sensitive data of various types
/// while preventing accidental exposure through debug logging or error messages.
///
/// # Examples
///
/// ```
/// use pywatt_sdk::typed_secret::Secret;
///
/// // Creating a secret integer
/// let secret_int = Secret::new(42);
/// assert_eq!(*secret_int.expose_secret(), 42);
///
/// // Debug representation is redacted
/// assert_eq!(format!("{:?}", secret_int), "Secret([REDACTED])");
///
/// // String secrets
/// let secret_string = Secret::new(String::from("my-api-key"));
/// assert_eq!(secret_string.expose_secret(), "my-api-key");
/// ```
#[derive(Clone)]
pub struct Secret<T> {
    value: T,
}

impl<T> Secret<T> {
    /// Create a new Secret wrapping the given value.
    pub fn new(value: T) -> Self {
        Self { value }
    }

    /// Expose the secret value.
    ///
    /// This method is deliberately verbose to discourage casual access to the secret value.
    /// Only use it when absolutely necessary, typically just before using the value.
    pub fn expose_secret(&self) -> &T {
        &self.value
    }

    /// Maps a `Secret<T>` to `Secret<U>` by applying a function to the contained value.
    ///
    /// This allows transforming the contained type while maintaining the secrecy wrapper.
    ///
    /// # Examples
    ///
    /// ```
    /// use pywatt_sdk::typed_secret::Secret;
    ///
    /// let secret_string = Secret::new("42".to_string());
    /// let secret_int = secret_string.map(|s| s.parse::<i32>().unwrap());
    /// assert_eq!(*secret_int.expose_secret(), 42);
    /// ```
    pub fn map<U, F>(self, f: F) -> Secret<U>
    where
        F: FnOnce(T) -> U,
    {
        Secret::new(f(self.value))
    }
}

impl<T: fmt::Debug> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret([REDACTED])")
    }
}

/// Error when parsing a secret to a typed value.
#[derive(Debug, Error)]
pub enum TypedSecretError {
    /// Error from the secret client
    #[error(transparent)]
    SecretError(#[from] SecretError),

    /// Error parsing the secret value into the target type
    #[error("failed to parse secret: {0}")]
    ParseError(String),
}

/// Get and parse a secret into the given type.
///
/// This function retrieves a secret using the provided `SecretClient` and attempts
/// to parse it into the specified type `T`. The type `T` must implement `FromStr`.
///
/// # Examples
///
/// ```no_run
/// use pywatt_sdk::{
///     secret_client::SecretClient,
///     typed_secret::{get_typed_secret, Secret},
/// };
/// use std::sync::Arc;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Initialize client
///     let client = Arc::new(SecretClient::new("http://localhost:9000", "my-module").await?);
///
///     // Get secrets of various types
///     let port: Secret<u16> = get_typed_secret(&client, "APP_PORT").await?;
///     let api_key: Secret<String> = get_typed_secret(&client, "API_KEY").await?;
///     let debug_mode: Secret<bool> = get_typed_secret(&client, "DEBUG_MODE").await?;
///
///     // Use the secrets (only when necessary)
///     let server_addr = format!("0.0.0.0:{}", port.expose_secret());
///
///     // The debug mode can affect logging behavior
///     if *debug_mode.expose_secret() {
///         println!("Running in debug mode");
///     }
///
///     // API key is automatically redacted in logs
///     println!("API key: {:?}", api_key); // Prints "API key: Secret([REDACTED])"
///
///     Ok(())
/// }
/// ```
///
/// # Errors
///
/// This function returns a `TypedSecretError` which can be either:
/// - `TypedSecretError::SecretError` if the secret can't be retrieved
/// - `TypedSecretError::ParseError` if the secret can't be parsed into the requested type
pub async fn get_typed_secret<T, S>(
    client: &SecretClient,
    key: S,
) -> Result<Secret<T>, TypedSecretError>
where
    S: AsRef<str>,
    T: FromStr,
    T::Err: fmt::Display,
{
    // Get the secret as a string
    let secret_string = client
        .get_secret(key.as_ref(), RequestMode::CacheThenRemote)
        .await?;

    // Parse it into the target type
    let parse_result = secret_string
        .expose_secret()
        .parse::<T>()
        .map_err(|e| TypedSecretError::ParseError(e.to_string()))?;

    Ok(Secret::new(parse_result))
}

/// Convenience function to get a secret as a string.
///
/// This is a type-specialized version of `get_typed_secret` that always returns a `Secret<String>`.
///
/// # Examples
///
/// ```no_run
/// use pywatt_sdk::{
///     secret_client::SecretClient,
///     typed_secret::get_string_secret,
/// };
/// use std::sync::Arc;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let client = Arc::new(SecretClient::new("http://localhost:9000", "my-module").await?);
///     let api_key = get_string_secret(&client, "API_KEY").await?;
///
///     // Use the api key
///     let headers = format!("Authorization: Bearer {}", api_key.expose_secret());
///
///     Ok(())
/// }
/// ```
pub async fn get_string_secret<S>(
    client: &SecretClient,
    key: S,
) -> Result<Secret<String>, TypedSecretError>
where
    S: AsRef<str>,
{
    // Get the secret as a string
    let secret_string = client
        .get_secret(key.as_ref(), RequestMode::CacheThenRemote)
        .await?;

    // Convert to a Secret<String>
    Ok(Secret::new(secret_string.expose_secret().to_string()))
}

/// Convenience function to get a secret as an integer.
///
/// # Examples
///
/// ```no_run
/// use pywatt_sdk::{
///     secret_client::SecretClient,
///     typed_secret::get_int_secret,
/// };
/// use std::sync::Arc;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let client = Arc::new(SecretClient::new("http://localhost:9000", "my-module").await?);
///     // Specify the integer type as the first generic parameter
///     let port = get_int_secret::<u16, &str>(&client, "PORT").await?;
///
///     println!("Port: {}", port.expose_secret());
///
///     Ok(())
/// }
/// ```
pub async fn get_int_secret<T, S>(
    client: &SecretClient,
    key: S,
) -> Result<Secret<T>, TypedSecretError>
where
    S: AsRef<str>,
    T: FromStr + std::fmt::Debug,
    T::Err: fmt::Display,
{
    get_typed_secret(client, key).await
}

/// Convenience function to get a secret as a boolean.
///
/// # Examples
///
/// ```no_run
/// use pywatt_sdk::{
///     secret_client::SecretClient,
///     typed_secret::get_bool_secret,
/// };
/// use std::sync::Arc;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let client = Arc::new(SecretClient::new("http://localhost:9000", "my-module").await?);
///     let feature_enabled = get_bool_secret(&client, "FEATURE_ENABLED").await?;
///
///     if *feature_enabled.expose_secret() {
///         println!("Feature is enabled");
///     }
///
///     Ok(())
/// }
/// ```
pub async fn get_bool_secret<S>(
    client: &SecretClient,
    key: S,
) -> Result<Secret<bool>, TypedSecretError>
where
    S: AsRef<str>,
{
    get_typed_secret(client, key).await
}

// Add Serialize implementation for Secret<T> where T: Serialize
impl<T: Serialize> Serialize for Secret<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.value.serialize(serializer)
    }
}

// Add Deserialize implementation for Secret<String>
impl<'de> Deserialize<'de> for Secret<String> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Secret::new(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_client::client::SecretClient;

    #[tokio::test]
    async fn get_typed_secret_parses_valid_int() {
        let client = SecretClient::new_dummy();

        // Manually populate the cache with a test secret
        client.insert_test_secret("int_key", "42").await;

        let result: Result<Secret<i32>, TypedSecretError> =
            get_typed_secret(&client, "int_key").await;

        assert!(result.is_ok());
        let secret = result.unwrap();
        assert_eq!(*secret.expose_secret(), 42);
    }

    #[tokio::test]
    async fn get_typed_secret_parses_valid_float() {
        let client = SecretClient::new_dummy();

        // Use a precise value that can be represented exactly in f64
        let test_value = 3.5;
        client
            .insert_test_secret("float_key", &test_value.to_string())
            .await;

        let result: Result<Secret<f64>, TypedSecretError> =
            get_typed_secret(&client, "float_key").await;

        assert!(result.is_ok());
        let secret = result.unwrap();

        // Compare the parsed value with the expected value
        assert_eq!(*secret.expose_secret(), test_value);
    }

    #[tokio::test]
    async fn get_typed_secret_parses_valid_bool() {
        let client = SecretClient::new_dummy();

        // Manually populate the cache with test secrets
        client.insert_test_secret("bool_true", "true").await;
        client.insert_test_secret("bool_false", "false").await;

        let result_true: Result<Secret<bool>, TypedSecretError> =
            get_typed_secret(&client, "bool_true").await;
        let result_false: Result<Secret<bool>, TypedSecretError> =
            get_typed_secret(&client, "bool_false").await;

        assert!(result_true.is_ok());
        assert!(result_false.is_ok());
        assert!(*result_true.unwrap().expose_secret());
        assert!(!(*result_false.unwrap().expose_secret()));
    }

    #[tokio::test]
    async fn get_typed_secret_returns_error_on_invalid_format() {
        let client = SecretClient::new_dummy();

        // Manually populate the cache with an invalid integer
        client
            .insert_test_secret("invalid_int", "not_an_integer")
            .await;

        let result: Result<Secret<i32>, TypedSecretError> =
            get_typed_secret(&client, "invalid_int").await;

        assert!(result.is_err());
        match result {
            Err(TypedSecretError::ParseError(_)) => {} // Expected error type
            _ => panic!("Expected ParseError, got different error or success"),
        }
    }

    #[tokio::test]
    async fn get_typed_secret_returns_error_on_missing_key() {
        let client = SecretClient::new_dummy();

        // Key does not exist in cache
        let result: Result<Secret<i32>, TypedSecretError> =
            get_typed_secret(&client, "missing_key").await;

        assert!(result.is_err());
        match result {
            Err(TypedSecretError::SecretError(_)) => {} // Expected error type
            _ => panic!("Expected SecretError, got different error or success"),
        }
    }

    #[tokio::test]
    async fn secret_debug_impl_redacts_value() {
        let secret = Secret::new(42);
        let debug_str = format!("{:?}", secret);
        assert_eq!(debug_str, "Secret([REDACTED])");
    }
}
