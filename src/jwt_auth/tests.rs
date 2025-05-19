use crate::secret_client::logging::{clear_redaction_registry, is_registered_for_redaction};
use axum::{
    Router,
    body::Body,
    extract::Extension,
    http::{Request, StatusCode, header::AUTHORIZATION},
    routing::get,
};
use jsonwebtoken::{Algorithm, EncodingKey, Header, Validation, encode};
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use serde_json::json; // Used in json! macro
use tower::ServiceExt; // for `.oneshot()` // Import the global registry and test helper functions

use super::middleware::JwtAuthenticationLayer;
use super::body_to_string;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct TestClaims {
    sub: String,
    role: Option<String>,
}

#[tokio::test]
async fn missing_header_is_unauthorized() {
    // Test with a type parameter for strong typing
    let layer = JwtAuthenticationLayer::<TestClaims>::new("secret".to_string());
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(layer);

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn invalid_token_is_unauthorized() {
    // Test with dynamic JSON typing
    let layer = JwtAuthenticationLayer::<serde_json::Value>::new("secret".to_string());
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(layer);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(AUTHORIZATION, "Bearer bad.token.here")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn valid_token_passes_and_claims_are_inserted() {
    let secret = "mysecret";
    // Test with strongly typed claims
    let layer = JwtAuthenticationLayer::<TestClaims>::new(secret.to_string());
    let app = Router::new()
        .route(
            "/claims",
            get(|Extension(claims): Extension<TestClaims>| async move { claims.sub }),
        )
        .layer(layer);

    // Create a token with test claims (include exp so default validation passes)
    let claims = serde_json::json!({
        "sub": "test-user",
        "role": "admin",
        "exp": 4102444800u64  // far future timestamp (2100-01-01)
    });

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/claims")
                .header(AUTHORIZATION, format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_str = body_to_string(response.into_body()).await;
    assert_eq!(body_str, "test-user");
}

#[tokio::test]
async fn custom_validation_rules_are_applied() {
    let secret = "mysecret";

    // Create a layer with custom validation that requires the "aud" claim
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_required_spec_claims(&["aud"]);
    validation.set_audience(&["my-app"]);

    let layer =
        JwtAuthenticationLayer::<TestClaims>::new(secret.to_string()).with_validation(validation);

    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(layer);

    // Create a token missing the required "aud" claim
    let claims = serde_json::json!({
        "sub": "test-user",
        "exp": 4102444800u64
    });

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(AUTHORIZATION, format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should be unauthorized because a required claim is missing
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn expired_token_is_unauthorized_when_validation_enabled() {
    let secret = "mysecret-exp";

    // Enable expiration validation
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true; // Explicitly enable expiration check

    let layer =
        JwtAuthenticationLayer::<TestClaims>::new(secret.to_string()).with_validation(validation);

    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(layer);

    // Create a token with an expired timestamp (e.g., 1970-01-01)
    let claims = serde_json::json!({
        "sub": "test-user-expired",
        "exp": 1u64 // Very old timestamp
    });

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(AUTHORIZATION, format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should be unauthorized because the token is expired
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Optional: Check the body for the specific error if desired
    // let body = response.into_body();
    // let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    // let body_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // assert!(body_json["error"].as_str().unwrap().contains("ExpiredSignature"));
}

#[tokio::test]
async fn invalid_signature_is_unauthorized() {
    let correct_secret = "correct_secret";
    let wrong_secret = "wrong_secret";

    // Layer uses the correct secret
    let layer = JwtAuthenticationLayer::<TestClaims>::new(correct_secret.to_string());
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(layer);

    // Create a token signed with the WRONG secret
    let claims = serde_json::json!({
        "sub": "test-user-sig",
        "exp": 4102444800u64 // Non-expired
    });

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(wrong_secret.as_bytes()), // Sign with wrong key
    )
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(AUTHORIZATION, format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should be unauthorized because the signature is invalid
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Optional: Check the body for the specific error
    // let body = response.into_body();
    // let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    // let body_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // assert!(body_json["error"].as_str().unwrap().contains("InvalidSignature"));
}

#[tokio::test]
async fn wrong_algorithm_is_unauthorized() {
    let secret = "algo_secret";

    // Layer expects HS256 by default
    let layer = JwtAuthenticationLayer::<TestClaims>::new(secret.to_string());
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(layer);

    // Create a token signed with HS512
    let claims = serde_json::json!({
        "sub": "test-user-algo",
        "exp": 4102444800u64 // Non-expired
    });

    let token = encode(
        &Header::new(Algorithm::HS512), // Use wrong algorithm
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(AUTHORIZATION, format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should be unauthorized because the algorithm doesn't match
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Optional: Check the body for the specific error
    // let body = response.into_body();
    // let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    // let body_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // assert!(body_json["error"].as_str().unwrap().contains("InvalidAlgorithm"));
}

#[tokio::test]
async fn valid_token_with_optional_claim_present() {
    let secret = "optional_present_secret";
    let layer = JwtAuthenticationLayer::<TestClaims>::new(secret.to_string());
    let app = Router::new()
        .route(
            "/claims",
            get(|Extension(claims): Extension<TestClaims>| async move {
                format!("{}:{}", claims.sub, claims.role.unwrap_or_default())
            }),
        )
        .layer(layer);

    // Token includes the optional 'role' claim
    let claims = serde_json::json!({
        "sub": "test-user-opt-present",
        "role": "tester",
        "exp": 4102444800u64
    });
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/claims")
                .header(AUTHORIZATION, format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body_str = body_to_string(response.into_body()).await;
    assert_eq!(body_str, "test-user-opt-present:tester");
}

#[tokio::test]
async fn valid_token_with_optional_claim_missing() {
    let secret = "optional_missing_secret";
    let layer = JwtAuthenticationLayer::<TestClaims>::new(secret.to_string());
    let app = Router::new()
        .route(
            "/claims",
            get(|Extension(claims): Extension<TestClaims>| async move {
                format!("{}:{}", claims.sub, claims.role.unwrap_or_default())
            }),
        )
        .layer(layer);

    // Token omits the optional 'role' claim
    let claims = serde_json::json!({
        "sub": "test-user-opt-missing",
        "exp": 4102444800u64
    });
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/claims")
                .header(AUTHORIZATION, format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body_str = body_to_string(response.into_body()).await;
    // Role should be empty string as per unwrap_or_default()
    assert_eq!(body_str, "test-user-opt-missing:");
}

#[tokio::test]
async fn compatibility_alias_works() {
    let secret = "compat_secret";
    // Use the JwtAuthLayer type alias
    let layer = crate::jwt_auth::JwtAuthLayer::<TestClaims>::new(secret.to_string());
    let app = Router::new()
        .route(
            "/claims",
            get(|Extension(claims): Extension<TestClaims>| async move { claims.sub }),
        )
        .layer(layer);

    let claims = serde_json::json!({
        "sub": "test-user-compat",
        "role": "user",
        "exp": 4102444800u64
    });
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/claims")
                .header(AUTHORIZATION, format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body_str = body_to_string(response.into_body()).await;
    assert_eq!(body_str, "test-user-compat");
}

#[tokio::test]
async fn layer_registers_secret_for_redaction() {
    let secret = "super-secret-key-for-redaction-test";

    // Ensure it's not registered before
    assert!(
        !is_registered_for_redaction(secret),
        "Secret should not be registered initially"
    );

    let _layer = JwtAuthenticationLayer::<TestClaims>::new(secret.to_string());

    // Check if the secret was added using the helper function
    assert!(
        is_registered_for_redaction(secret),
        "Secret should be registered for redaction"
    );

    // Clean up the registry using the helper function
    // Note: This might clear other secrets if tests run in parallel.
    // A dedicated test secret and removing only that might be safer, but clear() is simpler for now.
    clear_redaction_registry();
    assert!(
        !is_registered_for_redaction(secret),
        "Secret should be removed after cleanup"
    );
}
