//! Feature flag testing module
//!
//! This module is for testing feature-flag-gated code paths.
//! The code in this module is not meant to be executed directly,
//! but to test that feature flag combinations compile correctly.

// We include this module only in test builds
#[cfg(test)]
mod tests {
    use axum::Router;
    #[cfg(feature = "cors")]
    use crate::ext::RouterExt;
    #[cfg(feature = "jwt_auth")]
    use axum::routing::get;

    // Test JWT auth feature
    #[cfg(feature = "jwt_auth")]
    #[test]
    #[allow(deprecated)]
    fn test_jwt_auth_compiles() {
        // Just a compilation test
        use crate::jwt_auth::JwtAuthLayer;
        use crate::jwt_auth::RouterJwtExt;
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Serialize, Deserialize, Clone)]
        struct TestClaims {
            sub: String,
        }

        // Test with type parameter
        let _app1: Router<()> = Router::new().with_jwt::<TestClaims>("secret".to_string());

        // Test with default parameter
        let _app2: Router<()> = Router::new().with_jwt::<serde_json::Value>("secret".to_string());

        // Test with layer directly
        // Provide explicit route type to avoid type inference issues
        let router: Router<()> = Router::new().route("/", get(|| async { "Hello" }));
        let _app3 = router.layer(JwtAuthLayer::<TestClaims>::new("secret".to_string()));
    }

    // Test builder feature
    #[cfg(feature = "builder")]
    #[test]
    fn test_builder_compiles() {
        use crate::AppState;
        use crate::secret_client::SecretClient;
        use std::sync::Arc;

        #[derive(Clone)]
        #[allow(dead_code)]
        struct TestFeatures {
            database: bool,
            cache: bool,
            #[allow(dead_code)]
            custom_feature: bool,
        }

        #[derive(Clone)]
        struct TestState {
            #[allow(dead_code)]
            value: String,
        }

        let client = Arc::new(SecretClient::new_dummy());
        let state = TestState {
            value: "test".to_string(),
        };

        let _app_state = AppState::builder()
            .with_module_id("test".to_string())
            .with_orchestrator_api("http://localhost".to_string())
            .with_secret_client(client)
            .with_custom(state);

        // Not calling build() as it would require real instances
    }

    // Test router extensions feature
    #[cfg(feature = "router_ext")]
    #[test]
    fn test_router_ext_compiles() {
        // Specify the state type parameter explicitly to resolve type annotations
        let _router: Router<()> = Router::new();
        #[cfg(feature = "cors")]
        let _with_cors = _router.with_cors_preflight();
    }

    // Additional feature tests can be added here as needed

    #[tokio::test]
    #[cfg(feature = "builder")]
    async fn test_module_builder_basic() {
        use crate::AnnouncedEndpoint;
        use crate::ModuleBuilder;

        // Mock initialization and secret fetching (not strictly needed for this test)
        let _builder = ModuleBuilder::<()>::new()
            .secret_keys(&["TEST_SECRET"])
            .endpoints(vec![AnnouncedEndpoint {
                path: "/test".to_string(),
                methods: vec!["GET".to_string()],
                auth: None,
            }])
            .state(|_init, _secrets| ());

        // TODO: Add assertions once the builder does more or returns observable state
        // For now, this test just ensures the builder pattern compiles and runs.
    }

    #[tokio::test]
    #[cfg(feature = "router_ext")]
    async fn test_router_extensions() {
        let _router: Router<()> = Router::new();
        // TODO: Add tests for discover_endpoints_mw and other extensions if any
        // For now, this test just ensures it compiles.
    }

    // ------------------------------------------------------------------
    // Shared helpers
    // ------------------------------------------------------------------

    /// Initialize logging for tests – minimal no-op to satisfy compilation.
    #[allow(dead_code)]
    fn setup_logging() {
        // In real tests we might configure tracing-subscriber, but for
        // compile-time checks a no-op is sufficient and avoids duplicated init.
    }

    // This helper is intentionally **not** a #[tokio::test] to avoid executing
    // incomplete logic. It only needs to compile.
    #[allow(dead_code)]
    async fn test_serve_module_with_state_and_router() {
        setup_logging();
        let _router: Router<()> = Router::new();
        // ... existing code ...
    }
}
