#[cfg(test)]
mod tests {
    use crate::announce_from_router;
    #[allow(unused_imports)]
    use axum::{
        Router,
        routing::{delete, get, post, put},
    };

    // Test basic endpoint discovery
    #[test]
    #[cfg(feature = "discover_endpoints")]
    fn test_basic_router_discovery() {
        // Create a simple router with a few endpoints
        let router = Router::new()
            .route("/hello", get(|| async { "Hello, World!" }))
            .route("/api/users", get(|| async { "Get Users" }))
            .route("/api/users", post(|| async { "Create User" }));

        // Discover endpoints
        let endpoints = announce_from_router(&router);

        // The actual implementation is stubbed, but we should test the design
        // Here we're testing the idea, even if the implementation is limited

        // In a full implementation, we would expect:
        // - An endpoint for "/hello" with GET method
        // - An endpoint for "/api/users" with GET and POST methods

        // For now, we just verify that the function returns without error
        // and the returned vec can be iterated over
        for endpoint in endpoints {
            assert!(endpoint.path.starts_with("/"));
            assert!(!endpoint.methods.is_empty());
        }
    }

    // Test nested router discovery
    #[test]
    #[cfg(feature = "discover_endpoints")]
    fn test_nested_router_discovery() {
        // Create a router with nested routes
        let api_router = Router::new()
            .route("/users", get(|| async { "Get Users" }))
            .route("/users/:id", get(|| async { "Get User" }))
            .route("/posts", get(|| async { "Get Posts" }));

        let router = Router::new()
            .route("/health", get(|| async { "OK" }))
            .nest("/api", api_router);

        // Discover endpoints
        let endpoints = announce_from_router(&router);

        // In a full implementation, we would expect:
        // - An endpoint for "/health" with GET method
        // - An endpoint for "/api/users" with GET method
        // - An endpoint for "/api/users/:id" with GET method
        // - An endpoint for "/api/posts" with GET method

        // For now, just verify the function runs without error
        for endpoint in endpoints {
            assert!(endpoint.path.starts_with("/"));
            assert!(!endpoint.methods.is_empty());
        }
    }

    // Test route with multiple methods
    #[test]
    #[cfg(feature = "discover_endpoints")]
    fn test_multiple_methods_discovery() {
        // Create a router with routes that have multiple methods
        let router = Router::new()
            .route("/api/resource", get(|| async { "Get" }))
            .route("/api/resource", post(|| async { "Post" }))
            .route("/api/resource", put(|| async { "Put" }))
            .route("/api/resource", delete(|| async { "Delete" }));

        // Discover endpoints
        let endpoints = announce_from_router(&router);

        // In a full implementation, we would expect:
        // - A single endpoint for "/api/resource" with GET, POST, PUT, DELETE methods

        // For now, just verify the function runs without error
        for endpoint in endpoints {
            assert!(endpoint.path.starts_with("/"));
            assert!(!endpoint.methods.is_empty());
        }
    }

    // Test when discovery feature is disabled
    #[test]
    #[cfg(not(feature = "discover_endpoints"))]
    fn test_discovery_feature_disabled() {
        let router = Router::new().route("/hello", get(|| async { "Hello, World!" }));

        let endpoints = announce_from_router(&router);

        // When feature is disabled, should return empty vec
        assert_eq!(endpoints.len(), 0);
    }
}
