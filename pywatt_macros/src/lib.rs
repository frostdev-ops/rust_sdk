use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, Ident, ExprArray, Token, LitStr, LitBool};
use syn::parse::{Parse, ParseStream};

/// Parses the arguments for the #[pywatt_sdk::module] attribute.
struct ModuleArgs {
    /// List of secret keys to prefetch.
    secrets: Vec<syn::Expr>,
    /// Whether to auto-subscribe to secret rotations.
    rotate: bool,
    /// Endpoints to announce.
    endpoints: Vec<syn::Expr>,
    /// Custom health endpoint path.
    health: Option<syn::LitStr>,
    /// Whether to enable Prometheus metrics endpoint.
    metrics: bool,
    /// Version prefix for announcement paths.
    version: Option<syn::LitStr>,
    /// State builder function: fn(&OrchestratorInit, Vec<SecretString>) -> T
    state_fn: Option<syn::Expr>,
}

impl Parse for ModuleArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut secrets = Vec::new();
        let mut rotate = false;
        let mut endpoints = Vec::new();
        let mut health = None;
        let mut metrics = false;
        let mut version = None;
        let mut state_fn = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            if key == "secrets" {
                let arr: ExprArray = input.parse()?;
                secrets = arr.elems.into_iter().collect();
            } else if key == "rotate" {
                let lit: LitBool = input.parse()?;
                rotate = lit.value;
            } else if key == "endpoints" {
                let arr: ExprArray = input.parse()?;
                endpoints = arr.elems.into_iter().collect();
            } else if key == "health" || key == "health_path" {
                let lit: LitStr = input.parse()?;
                health = Some(lit);
            } else if key == "metrics" {
                let lit: LitBool = input.parse()?;
                metrics = lit.value;
            } else if key == "version" {
                let lit: LitStr = input.parse()?;
                version = Some(lit);
            } else if key == "state" {
                let expr: syn::Expr = input.parse()?;
                state_fn = Some(expr);
            } else {
                let _skip: syn::Expr = input.parse()?;
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(ModuleArgs { secrets, rotate, endpoints, health, metrics, version, state_fn })
    }
}

#[proc_macro_attribute]
pub fn module(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse attribute arguments
    let args = parse_macro_input!(attr as ModuleArgs);
    let func = parse_macro_input!(item as ItemFn);
    
    module_impl(args, func).into()
}

// Non-proc-macro version that can be tested
fn module_impl(args: ModuleArgs, func: ItemFn) -> proc_macro2::TokenStream {
    let func_name = &func.sig.ident;
    let secrets = &args.secrets;
    let rotate = args.rotate;
    let endpoints = &args.endpoints;
    let health_path = match &args.health {
        Some(lit) => quote! { #lit },
        None => quote! { "/health" },
    };
    let metrics_enabled = args.metrics;
    let version_prefix = match &args.version {
        Some(lit) => quote! { Some(#lit.to_string()) },
        None => quote! { None },
    };
    // State builder function: use provided function or default to Default::default()
    let state_fn_tokens = if let Some(expr) = args.state_fn {
        quote! { #expr }
    } else {
        quote! { |_, _| Default::default() }
    };

    let expanded = quote! {
        #func

        #[tokio::main]
        async fn main() -> ::pywatt_sdk::Result<()> {
            // 1. Initialize logging
            ::pywatt_sdk::init_module();

            // 2. Handshake with orchestrator
            let init = ::pywatt_sdk::read_init().await?;
            // Import extension trait for listen_to_string()
            use ::pywatt_sdk::OrchestratorInitExt;

            // 3. Create secret client
            let client = ::pywatt_sdk::get_module_secret_client(&init.orchestrator_api, &init.module_id).await?;

            // 4. Prefetch secrets
            let secret_keys: &[&str] = &[#(#secrets),*];
            let secrets = if !secret_keys.is_empty() {
                ::pywatt_sdk::get_secrets(&client, secret_keys).await?
            } else {
                Vec::new()
            };

            // 5. Optional secret rotation subscription
            if #rotate {
                let _ = ::pywatt_sdk::subscribe_secret_rotations(
                    client.clone(),
                    secret_keys.iter().map(|s| s.to_string()).collect(),
                    |_, new_secret| { /* secrets auto-registered for redaction */ }
                );
            }

            // 6. Build user state via provided function
            let user_state = #state_fn_tokens(&init, secrets);

            // 7. Build AppState
            let mut app_state = ::pywatt_sdk::AppState::new(
                init.module_id.clone(),
                init.orchestrator_api.clone(),
                client.clone(),
                user_state,
            );

            // 8. Register module and start heartbeat
            // Parse host and port from `orchestrator_api`
            let api_str = init.orchestrator_api.as_str();
            let host_port = api_str.split("//").nth(1).unwrap_or(api_str);
            let mut parts = host_port.split(':');
            let host = parts.next().unwrap();
            let port: u16 = parts.next().unwrap().parse().unwrap();
            let config = ::pywatt_sdk::ConnectionConfig::new(host, port);
            let module_info = ::pywatt_sdk::ModuleInfo::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_DESCRIPTION"),
            );
            let registered = ::pywatt_sdk::register_module(config.clone(), module_info).await?;
            let _hb = ::pywatt_sdk::start_heartbeat_loop(
                registered,
                std::time::Duration::from_secs(30),
                || (::pywatt_sdk::HealthStatus::Healthy, None),
            );

            // 9. Build the Axum router
            let mut router = #func_name(app_state.clone()).await;
            router = router.layer(::axum::Extension(app_state.clone()));

            // 10. Health endpoint
            #[cfg(feature = "router_ext")]
            {
                use ::axum::routing::get;
                let path: &str = #health_path;
                if path == "/health" {
                    router = router.with_default_health();
                } else {
                    router = router.route(
                        path,
                        get(|| async {
                            let info = ::serde_json::json!({
                                "git": ::pywatt_sdk::build::GIT_HASH,
                                "time": ::pywatt_sdk::build::BUILD_TIME_UTC,
                                "rustc": ::pywatt_sdk::build::RUSTC_VERSION,
                                "status": "OK"
                            });
                            ::axum::Json(info)
                        }),
                    );
                }
            }

            // 11. Metrics endpoint
            #[cfg(all(feature = "router_ext", feature = "metrics"))]
            if #metrics_enabled {
                router = router.with_prometheus_metrics();
            }

            // 12. Announce endpoints with optional version prefix
            let mut eps: Vec<::pywatt_sdk::EndpointAnnounce> = vec![#(#endpoints),*];
            if let #version_prefix = #version_prefix {
                if let Some(prefix) = #version_prefix {
                    for ep in &mut eps {
                        if ep.path != #health_path {
                            ep.path = format!("/{prefix}{}", ep.path);
                        }
                    }
                }
            }
            let announce = ::pywatt_sdk::ModuleAnnounce {
                listen: init.listen_to_string(),
                endpoints: eps,
            };
            ::pywatt_sdk::send_announce(&announce)?;

            // 13. Serve HTTP over TCP
            match init.listen {
                ::pywatt_sdk::ipc_types::ListenAddress::Tcp(addr) => {
                    let listener = ::tokio::net::TcpListener::bind(addr).await?;
                    ::axum::serve(listener, router.into_make_service()).await?;
                }
                _ => {
                    ::tracing::error!("unsupported listen address");
                    return Err(::pywatt_sdk::Error::Config(
                        ::pywatt_sdk::error::ConfigError::Invalid(
                            "Only TCP listen is supported".into()
                        )
                    ));
                }
            }

            Ok(())
        }
    };

    expanded
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;
    

    #[test]
    fn it_compiles() {
        let input = quote! {
            #[pywatt_sdk::module]
            async fn foo(state: AppState<()>) -> Router { Router::new() }
        };
        
        // Parse the input to match what module_impl expects
        let attr = proc_macro2::TokenStream::new(); // Empty attributes for test
        let args = syn::parse2::<ModuleArgs>(attr).unwrap();
        let func = syn::parse2::<ItemFn>(input.clone()).unwrap();
        
        // Call the implementation function directly
        let _ = module_impl(args, func);
    }
}