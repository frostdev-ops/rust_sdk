use axum::{
    http::{Request, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
};
use futures::future::{self, BoxFuture};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::de::DeserializeOwned;
use std::{
    convert::Infallible,
    marker::PhantomData,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::sync::Mutex;
use tower::{Layer, Service};

use crate::jwt_auth::is_running_as_module;
use crate::secret_client::register_for_redaction;

#[cfg(feature = "ipc")]
use crate::jwt_auth::proxy_adapter::{JwtProxyConfig, JwtProxyService, validate_token_proxy};

/// Axum middleware layer for validating Bearer JWTs with a shared HMAC secret.
///
/// The type parameter `T` represents the claims type to decode from the JWT.
/// By default, it uses `serde_json::Value` for dynamic claims.
///
/// # Example
///
/// ```rust,no_run
/// use axum::Router;
/// use pywatt_sdk::jwt_auth::middleware::JwtAuthenticationLayer;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, Serialize, Deserialize, Clone)]
/// struct MyClaims {
///     sub: String,
///     exp: usize,
///     role: String,
/// }
///
/// let app: Router = Router::new()
///     // Use with generic type parameter
///     .layer(JwtAuthenticationLayer::<MyClaims>::new("my-secret".to_string()));
/// ```
#[derive(Clone)]
pub struct JwtAuthenticationLayer<T = serde_json::Value> {
    secret_key: String,
    _marker: PhantomData<T>,
}

impl<T> JwtAuthenticationLayer<T> {
    /// Create a new JWT auth layer, registering the raw secret for redaction.
    pub fn new(secret_key: String) -> Self {
        register_for_redaction(&secret_key);
        Self {
            secret_key,
            _marker: PhantomData,
        }
    }

    /// Set validation options for JWT tokens.
    ///
    /// By default, this middleware uses HS256 algorithm with default validation options.
    /// Use this method to customize the validation behavior.
    pub fn with_validation(
        self,
        validation: Validation,
    ) -> JwtAuthenticationLayerWithValidation<T> {
        JwtAuthenticationLayerWithValidation {
            secret_key: self.secret_key,
            validation,
            _marker: PhantomData,
        }
    }
}

/// JWT Authentication layer with custom validation rules
#[derive(Clone)]
pub struct JwtAuthenticationLayerWithValidation<T = serde_json::Value> {
    secret_key: String,
    validation: Validation,
    _marker: PhantomData<T>,
}

impl<T> JwtAuthenticationLayerWithValidation<T> {
    /// Create a new JWT auth layer with custom validation
    pub fn new(secret_key: String, validation: Validation) -> Self {
        register_for_redaction(&secret_key);
        Self {
            secret_key,
            validation,
            _marker: PhantomData,
        }
    }
}

/// The actual service that checks `Authorization: Bearer <token>` and decodes the JWT.
#[derive(Clone)]
pub struct JwtAuthService<S, T = serde_json::Value> {
    inner: S,
    secret_key: String,
    validation: Validation,
    _marker: PhantomData<T>,
    #[cfg(feature = "ipc")]
    proxy_service: Option<Arc<Mutex<Option<JwtProxyService>>>>,
}

impl<S, B, T> Service<Request<B>> for JwtAuthService<S, T>
where
    S: Service<Request<B>, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S: 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
    T: DeserializeOwned + Send + Sync + Clone + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        let key = self.secret_key.clone();
        let validation = self.validation.clone();

        let token_opt = req
            .headers()
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(str::to_string);

        if let Some(token) = token_opt {
            #[cfg(feature = "ipc")]
            let proxy_service = self.proxy_service.clone();

            let mut svc = self.inner.clone();

            // Check if running as a module
            if is_running_as_module() {
                #[cfg(feature = "ipc")]
                {
                    // Use the proxy service for token validation
                    Box::pin(async move {
                        // Get or initialize the proxy service
                        let proxy_service_arc = match proxy_service {
                            Some(arc) => arc,
                            None => {
                                return Ok((
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    axum::Json(serde_json::json!({
                                        "error": "JWT proxy service not initialized"
                                    })),
                                )
                                    .into_response());
                            }
                        };

                        let mut proxy_service_lock = proxy_service_arc.lock().await;

                        if proxy_service_lock.is_none() {
                            // Initialize the proxy service
                            let config = JwtProxyConfig::default();
                            match JwtProxyService::connect(&config).await {
                                Ok(service) => {
                                    *proxy_service_lock = Some(service);
                                }
                                Err(e) => {
                                    return Ok((
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        axum::Json(serde_json::json!({
                                            "error": format!("Failed to connect to JWT service: {}", e)
                                        })),
                                    )
                                        .into_response());
                                }
                            }
                        }

                        // Use the proxy service to validate the token
                        let proxy = proxy_service_lock.as_ref().unwrap();
                        match validate_token_proxy::<T>(proxy, &token).await {
                            Ok(claims) => {
                                req.extensions_mut().insert(claims);
                                drop(proxy_service_lock); // Release the lock before awaiting
                                svc.call(req).await // This already returns Result<Response, Infallible>
                            }
                            Err(err) => {
                                Ok((
                                    StatusCode::UNAUTHORIZED,
                                    axum::Json(serde_json::json!({
                                        "error": format!("Unauthorized: {}", err)
                                    })),
                                )
                                    .into_response())
                            }
                        }
                    })
                }

                #[cfg(not(feature = "ipc"))]
                {
                    // If IPC support is not enabled but running as a module, return error
                    Box::pin(future::ok(
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            axum::Json(serde_json::json!({
                                "error": "Running as a module but IPC feature is not enabled"
                            })),
                        )
                            .into_response(),
                    ))
                }
            } else {
                // Use local JWT validation
                let decoding_key = DecodingKey::from_secret(key.as_bytes());
                match decode::<T>(&token, &decoding_key, &validation) {
                    Ok(data) => {
                        req.extensions_mut().insert(data.claims);
                        Box::pin(async move { svc.call(req).await }) // svc.call already returns Result<Response, Infallible>
                    }
                    Err(err) => {
                        Box::pin(future::ok((
                            StatusCode::UNAUTHORIZED,
                            axum::Json(serde_json::json!({
                                "error": format!("Unauthorized: {}", err)
                            })),
                        )
                            .into_response()))
                    }
                }
            }
        } else {
            Box::pin(future::ok(
                (
                    StatusCode::UNAUTHORIZED,
                    axum::Json(serde_json::json!({
                        "error": "Missing Authorization header"
                    })),
                )
                    .into_response(),
            ))
        }
    }
}

/// Tower layer implementation for the standard JWT auth with default validation.
impl<S, T> Layer<S> for JwtAuthenticationLayer<T> {
    type Service = JwtAuthService<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = false;

        JwtAuthService {
            inner,
            secret_key: self.secret_key.clone(),
            validation,
            _marker: PhantomData,
            #[cfg(feature = "ipc")]
            proxy_service: if is_running_as_module() {
                Some(Arc::new(Mutex::new(None)))
            } else {
                None
            },
        }
    }
}

/// Tower layer implementation for JWT auth with custom validation.
impl<S, T> Layer<S> for JwtAuthenticationLayerWithValidation<T> {
    type Service = JwtAuthService<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        JwtAuthService {
            inner,
            secret_key: self.secret_key.clone(),
            validation: self.validation.clone(),
            _marker: PhantomData,
            #[cfg(feature = "ipc")]
            proxy_service: if is_running_as_module() {
                Some(Arc::new(Mutex::new(None)))
            } else {
                None
            },
        }
    }
}
