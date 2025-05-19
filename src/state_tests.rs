#[cfg(test)]
mod tests {
    use crate::secret_client::SecretClient;
    use crate::state::AppState;
    use std::sync::Arc;

    #[test]
    fn test_appstate_basic_functionality() {
        let module_id = "test-module".to_string();
        let orchestrator_api = "http://localhost:8000".to_string();
        let secret_client = Arc::new(SecretClient::for_test());
        let user_state = "Test user state".to_string();

        let state = AppState::new(
            module_id.clone(),
            orchestrator_api.clone(),
            secret_client.clone(),
            user_state.clone(),
        );

        // Test getters
        assert_eq!(state.module_id(), module_id);
        assert_eq!(state.orchestrator_api(), orchestrator_api);

        // Can't directly compare Arc<SecretClient> for equality since SecretClient doesn't implement PartialEq
        // Instead, check if they point to the same instance
        assert!(Arc::ptr_eq(state.secret_client(), &secret_client));

        assert_eq!(state.user_state, user_state);
    }

    #[test]
    fn test_appstate_clone() {
        let module_id = "test-module".to_string();
        let orchestrator_api = "http://localhost:8000".to_string();
        let secret_client = Arc::new(SecretClient::for_test());
        let user_state = "Test user state".to_string();

        let state = AppState::new(
            module_id.clone(),
            orchestrator_api.clone(),
            secret_client.clone(),
            user_state.clone(),
        );

        let cloned_state = state.clone();

        // Test that both states have the same content
        assert_eq!(cloned_state.module_id(), state.module_id());
        assert_eq!(cloned_state.orchestrator_api(), state.orchestrator_api());
        assert_eq!(cloned_state.user_state, state.user_state);

        // Test that the secret_client is the same instance (Arc comparison)
        assert!(Arc::ptr_eq(
            cloned_state.secret_client(),
            state.secret_client()
        ));
    }

    #[cfg(feature = "builder")]
    #[test]
    fn test_appstate_builder() {
        // Import builder directly
        let builder = crate::builder::AppStateBuilder::<String>::new();

        let state = builder
            .with_module_id("test-module".to_string())
            .with_orchestrator_api("http://localhost:8000".to_string())
            .with_secret_client(Arc::new(SecretClient::for_test()))
            .with_custom("Test user state".to_string())
            .build()
            .unwrap();

        assert_eq!(state.module_id(), "test-module");
        assert_eq!(state.orchestrator_api(), "http://localhost:8000");
        assert_eq!(state.user_state, "Test user state");
    }

    // Test custom state types
    #[test]
    fn test_appstate_with_custom_state() {
        #[derive(Clone, Debug, PartialEq)]
        struct MyState {
            counter: i32,
            name: String,
        }

        let module_id = "test-module".to_string();
        let orchestrator_api = "http://localhost:8000".to_string();
        let secret_client = Arc::new(SecretClient::for_test());
        let user_state = MyState {
            counter: 42,
            name: "Test".to_string(),
        };

        let state = AppState::new(
            module_id.clone(),
            orchestrator_api.clone(),
            secret_client.clone(),
            user_state.clone(),
        );

        assert_eq!(state.user_state, user_state);
        assert_eq!(state.user_state.counter, 42);
        assert_eq!(state.user_state.name, "Test");
    }
}
