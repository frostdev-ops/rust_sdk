use secrecy::SecretString;
use std::sync::Arc;
use tokio::task::JoinHandle;
use std::collections::HashMap;
use tokio::sync::Mutex;
use tracing::warn;

use crate::{AnnouncedEndpoint, AppState, BootstrapError, OrchestratorInit, bootstrap_module, internal_messaging::{InternalMessagingClient, PendingInternalResponses}};

#[cfg(feature = "builder")]
use crate::error::ConfigError;
#[cfg(feature = "builder")]
use crate::secret_client::SecretClient;

/// Type alias for the state builder function to reduce complexity
type StateBuilderFn<T> = Arc<dyn Fn(&OrchestratorInit, Vec<SecretString>) -> T + Send + Sync>;

/// Builder pattern for bootstrapping PyWatt modules.
pub struct ModuleBuilder<T> {
    secret_keys: Vec<String>,
    endpoints: Vec<AnnouncedEndpoint>,
    state_builder: StateBuilderFn<T>,
    state_set: bool,
}

impl<T: Send + 'static> Default for ModuleBuilder<T> {
    fn default() -> Self {
        Self {
            secret_keys: Vec::new(),
            endpoints: Vec::new(),
            state_builder: Arc::new(|_, _| unreachable!("State builder not set")),
            state_set: false,
        }
    }
}

impl<T: Send + Sync + Clone + 'static> ModuleBuilder<T> {
    /// Create a new ModuleBuilder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the secret keys to fetch initially.
    pub fn secret_keys(mut self, keys: &[&str]) -> Self {
        self.secret_keys = keys.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Set the endpoints for module announcement.
    pub fn endpoints(mut self, endpoints: Vec<AnnouncedEndpoint>) -> Self {
        self.endpoints = endpoints;
        self
    }

    /// Set the state builder function, which constructs user state from init and fetched secrets.
    pub fn state<F>(mut self, builder: F) -> Self
    where
        F: Fn(&OrchestratorInit, Vec<SecretString>) -> T + Send + Sync + 'static,
    {
        self.state_builder = Arc::new(builder);
        self.state_set = true;
        self
    }

    /// Execute the bootstrap process: handshake, secret fetch, announcement, and start IPC loop.
    pub async fn build(self) -> Result<(AppState<T>, JoinHandle<()>), BootstrapError> {
        // Ensure the state builder was set
        if !self.state_set {
            return Err(BootstrapError::Other("State builder not set".to_string()));
        }
        // Wrap the Arc<dyn Fn> into a closure that implements Fn
        let state_builder = self.state_builder.clone();
        let (mut app_state, handle) = bootstrap_module(
            self.secret_keys,
            self.endpoints,
            move |init, secrets| (state_builder)(init, secrets),
        )
        .await?;

        // Initialize InternalMessagingClient and PendingInternalResponses
        // This must happen after AppState is created and orchestrator_channel is potentially populated.
        let pending_map: PendingInternalResponses = Arc::new(Mutex::new(HashMap::new()));

        if let Some(tcp_channel) = app_state.tcp_channel.as_ref() {
            let _default_encoding = app_state.config
                .as_ref()
                .and_then(|cfg| cfg.message_format_primary)
                .unwrap_or_default();
                
            let im_client = InternalMessagingClient::new(
                app_state.module_id().to_string(),
                Some(pending_map.clone()),
                Some(tcp_channel.clone()),
            );
            app_state.internal_messaging_client = Some(im_client);
            app_state.pending_internal_responses = Some(pending_map);
        } else {
            // Log a warning if the TCP channel, which is needed for internal messaging, isn't available.
            // This might be normal if the module is not configured to connect to an orchestrator via TCP.
            warn!(
                module_id = %app_state.module_id(),
                "InternalMessagingClient not initialized: No TCP channel available after bootstrap."
            );
        }

        Ok((app_state, handle))
    }
}

/// Builder for creating an AppState instance with a fluent API.
///
/// # Example
///
/// ```rust,no_run
/// use pywatt_sdk::AppState;
/// use pywatt_sdk::secret_client::SecretClient;
/// use std::sync::Arc;
///
/// #[derive(Clone)]
/// struct MyCustomState {
///     db_url: String,
///     api_key: String,
/// }
///
/// let client = Arc::new(SecretClient::new_dummy());
/// let custom_state = MyCustomState {
///     db_url: "postgresql://localhost/db".to_string(),
///     api_key: "test-key".to_string(),
/// };
///
/// let state = AppState::builder()
///     .with_module_id("my-module".to_string())
///     .with_orchestrator_api("http://localhost:9900".to_string())
///     .with_secret_client(client)
///     .with_custom(custom_state)
///     .build();
/// ```
#[cfg(feature = "builder")]
pub struct AppStateBuilder<T> {
    module_id: Option<String>,
    orchestrator_api: Option<String>,
    secret_client: Option<Arc<SecretClient>>,
    user_state: Option<T>,
}

#[cfg(feature = "builder")]
impl<T> Default for AppStateBuilder<T> {
    fn default() -> Self {
        Self {
            module_id: None,
            orchestrator_api: None,
            secret_client: None,
            user_state: None,
        }
    }
}

#[cfg(feature = "builder")]
impl<T> AppStateBuilder<T> {
    /// Create a new AppStateBuilder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the module ID.
    pub fn with_module_id(mut self, module_id: String) -> Self {
        self.module_id = Some(module_id);
        self
    }

    /// Set the orchestrator API URL.
    pub fn with_orchestrator_api(mut self, orchestrator_api: String) -> Self {
        self.orchestrator_api = Some(orchestrator_api);
        self
    }

    /// Set the secret client.
    pub fn with_secret_client(mut self, secret_client: Arc<SecretClient>) -> Self {
        self.secret_client = Some(secret_client);
        self
    }

    /// Set the custom user state.
    pub fn with_custom(mut self, user_state: T) -> Self {
        self.user_state = Some(user_state);
        self
    }

    /// Build the AppState instance.
    ///
    /// Returns an error if any required field is missing.
    pub fn build(self) -> Result<AppState<T>, ConfigError> {
        let module_id = self
            .module_id
            .ok_or_else(|| ConfigError::MissingEnvVar("module_id".to_string()))?;
        let orchestrator_api = self
            .orchestrator_api
            .ok_or_else(|| ConfigError::MissingEnvVar("orchestrator_api".to_string()))?;
        let secret_client = self
            .secret_client
            .ok_or_else(|| ConfigError::MissingEnvVar("secret_client".to_string()))?;
        let user_state = self
            .user_state
            .ok_or_else(|| ConfigError::MissingEnvVar("user_state".to_string()))?;

        Ok(AppState::new(
            module_id,
            orchestrator_api,
            secret_client,
            user_state,
        ))
    }

    /// Alias for `build()` - provided for backward compatibility.
    ///
    /// Use `build()` for new code.
    #[deprecated(since = "0.2.5", note = "Use `build()` instead")]
    pub fn try_build(self) -> Result<AppState<T>, ConfigError> {
        self.build()
    }
}

#[cfg(test)]
#[cfg(feature = "builder")]
mod tests {
    use super::AppStateBuilder;
    use crate::error::ConfigError;
    use crate::secret_client::client::SecretClient;
    use crate::state::AppState;
    use std::sync::Arc;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct CustomState(String);

    #[test]
    fn build_success_when_all_fields_set() {
        let custom_state = CustomState("state-val".to_string());
        let client = Arc::new(SecretClient::new_dummy());
        let app_state: AppState<CustomState> = AppStateBuilder::new()
            .with_module_id("mod1".to_string())
            .with_orchestrator_api("http://localhost:9900".to_string())
            .with_secret_client(client.clone())
            .with_custom(custom_state.clone())
            .build()
            .unwrap();
        assert_eq!(app_state.module_id(), "mod1");
        assert_eq!(app_state.orchestrator_api(), "http://localhost:9900");
        assert!(
            Arc::ptr_eq(app_state.secret_client(), &client),
            "SecretClient Arc should be the same instance"
        );
        assert_eq!(app_state.user_state, custom_state);
    }

    #[test]
    fn missing_module_id_returns_error() {
        let custom_state = CustomState("state-val".to_string());
        let client = Arc::new(SecretClient::new_dummy());
        let err = AppStateBuilder::new()
            .with_orchestrator_api("http://localhost:9900".to_string())
            .with_secret_client(client.clone())
            .with_custom(custom_state.clone())
            .build()
            .unwrap_err();
        assert_eq!(err, ConfigError::MissingEnvVar("module_id".to_string()));
    }

    #[test]
    fn missing_orchestrator_api_returns_error() {
        let custom_state = CustomState("state-val".to_string());
        let client = Arc::new(SecretClient::new_dummy());
        let err = AppStateBuilder::new()
            .with_module_id("mod1".to_string())
            .with_secret_client(client.clone())
            .with_custom(custom_state.clone())
            .build()
            .unwrap_err();
        assert_eq!(
            err,
            ConfigError::MissingEnvVar("orchestrator_api".to_string())
        );
    }

    #[test]
    fn missing_secret_client_returns_error() {
        let custom_state = CustomState("state-val".to_string());
        let err = AppStateBuilder::new()
            .with_module_id("mod1".to_string())
            .with_orchestrator_api("http://localhost:9900".to_string())
            .with_custom(custom_state.clone())
            .build()
            .unwrap_err();
        assert_eq!(err, ConfigError::MissingEnvVar("secret_client".to_string()));
    }

    #[test]
    fn missing_user_state_returns_error() {
        let client = Arc::new(SecretClient::new_dummy());
        let err = AppStateBuilder::<CustomState>::new()
            .with_module_id("mod1".to_string())
            .with_orchestrator_api("http://localhost:9900".to_string())
            .with_secret_client(client.clone())
            .build()
            .unwrap_err();
        assert_eq!(err, ConfigError::MissingEnvVar("user_state".to_string()));
    }

    #[test]
    #[allow(deprecated)]
    fn try_build_alias_behaves_like_build() {
        let custom_state = CustomState("state-val".to_string());
        let client = Arc::new(SecretClient::new_dummy());
        let result_build = AppStateBuilder::new()
            .with_module_id("mod1".to_string())
            .with_orchestrator_api("http://localhost:9900".to_string())
            .with_secret_client(client.clone())
            .with_custom(custom_state.clone())
            .build();
        let result_try = AppStateBuilder::new()
            .with_module_id("mod1".to_string())
            .with_orchestrator_api("http://localhost:9900".to_string())
            .with_secret_client(client.clone())
            .with_custom(custom_state.clone())
            .try_build();
        assert_eq!(result_build.is_ok(), result_try.is_ok());
    }
}

#[cfg(test)]
#[cfg(feature = "builder")]
mod module_builder_tests {
    use super::ModuleBuilder;
    use crate::OrchestratorInit;
    use crate::{EndpointAnnounce, bootstrap::BootstrapError};
    use secrecy::{ExposeSecret, SecretString};
    use std::sync::{Arc, Mutex};

    // Dummy struct for testing state
    #[derive(Clone)]
    struct TestState;

    #[tokio::test]
    async fn build_without_state_fails() {
        let builder: ModuleBuilder<TestState> = ModuleBuilder::new()
            .secret_keys(&["KEY1"])
            .endpoints(vec![EndpointAnnounce {
                path: "/test".to_string(),
                methods: vec!["GET".to_string()],
                auth: None,
            }]);

        let result = builder.build().await;

        assert!(
            matches!(result, Err(BootstrapError::Other(msg)) if msg == "State builder not set")
        );
    }

    #[tokio::test]
    async fn test_successful_build_configuration() {
        // Capture the secret keys and orchestrator init passed to the state builder
        let captured_keys = Arc::new(Mutex::new(Vec::new()));
        let captured_init = Arc::new(Mutex::new(None));

        let captured_keys_clone = captured_keys.clone();
        let captured_init_clone = captured_init.clone();

        // Create a mock state builder function that records what it was called with
        let state_builder = move |init: &OrchestratorInit, keys: Vec<SecretString>| {
            // Capture the keys passed to the builder
            let mut keys_lock = captured_keys_clone.lock().unwrap();
            *keys_lock = keys.iter().map(|s| s.expose_secret().to_string()).collect();

            // Capture the orchestrator init
            let mut init_lock = captured_init_clone.lock().unwrap();
            *init_lock = Some(init.clone());

            // Return a test state
            TestState
        };

        // Create a ModuleBuilder with our mock state builder
        let builder: ModuleBuilder<TestState> = ModuleBuilder::new()
            .secret_keys(&["KEY1", "KEY2"])
            .endpoints(vec![
                EndpointAnnounce {
                    path: "/test1".to_string(),
                    methods: vec!["GET".to_string()],
                    auth: None,
                },
                EndpointAnnounce {
                    path: "/test2".to_string(),
                    methods: vec!["POST".to_string()],
                    auth: None,
                },
            ])
            .state(state_builder);

        // We can't actually call .build() since it tries to connect to the orchestrator,
        // but we can verify that the builder configuration is captured correctly by
        // examining the builder's fields directly

        // Verify the secret keys were captured correctly
        assert_eq!(
            builder.secret_keys,
            vec!["KEY1".to_string(), "KEY2".to_string()]
        );

        // Verify the endpoints were captured correctly
        assert_eq!(builder.endpoints.len(), 2);
        assert_eq!(builder.endpoints[0].path, "/test1");
        assert_eq!(builder.endpoints[0].methods, vec!["GET"]);
        assert_eq!(builder.endpoints[1].path, "/test2");
        assert_eq!(builder.endpoints[1].methods, vec!["POST"]);

        // Verify the state builder was set
        assert!(builder.state_set);
    }
}
