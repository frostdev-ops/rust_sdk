#![allow(dead_code)]
use crate::ipc_types::{
    IpcHttpRequest, IpcHttpResponse, ModuleToOrchestrator, OrchestratorToModule, ServiceOperation,
    ServiceRequest,
};
use crate::logging::safe_log;
use crate::secret_client::SecretClient;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{
    Mutex,
    broadcast::{
        Receiver as BroadcastReceiver, Sender as BroadcastSender, channel as broadcast_channel,
    },
    mpsc::{Receiver, Sender, channel},
    oneshot,
};
use tracing::trace;
use std::result::Result as StdResult;

// Type of channel for sending requests to the orchestrator
type RequestChannel = Sender<(String, oneshot::Sender<String>)>;

// Response receivers for pending requests
type PendingResponses = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;

// Global IPC channel for sending requests and receiving responses
lazy_static::lazy_static! {
    static ref IPC_CHANNEL: (Arc<Mutex<Option<RequestChannel>>>, PendingResponses) = {
        let pending_responses = Arc::new(Mutex::new(HashMap::new()));
        let channel = Arc::new(Mutex::new(None));
        (channel, pending_responses)
    };
    // Global mutex-wrapped stdout to ensure serialized writes from multiple tasks.
    static ref STDOUT_WRITER: tokio::sync::Mutex<io::Stdout> = tokio::sync::Mutex::new(io::stdout());

    // Global HTTP request broadcast channel - using std::sync::Mutex for synchronous access
    static ref HTTP_CHANNEL: StdMutex<Option<(BroadcastSender<IpcHttpRequest>, BroadcastReceiver<IpcHttpRequest>)>> = {
        StdMutex::new(None)
    };
}

static ONCE: std::sync::Once = std::sync::Once::new();

/// An IPC Manager to handle communication with the orchestrator
pub struct IpcManager {
    // Using unit struct as the functionality is implemented via associated functions
}

impl IpcManager {
    /// Creates a new IpcManager
    pub fn new() -> Self {
        Self {}
    }

    /// Subscribe to incoming HTTP requests from the orchestrator
    pub fn subscribe_http_requests(&self) -> BroadcastReceiver<IpcHttpRequest> {
        subscribe_http_requests()
    }

    /// Send an HTTP response back to the orchestrator
    pub async fn send_http_response(&self, response: IpcHttpResponse) -> StdResult<(), String> {
        send_http_response(response).await
    }

    /// Process IPC messages from the orchestrator
    pub async fn process_ipc_messages(&self) {
        process_ipc_messages().await;
    }

    /// Send a request to the orchestrator and wait for the response
    pub async fn send_request<T>(&self, request: &T) -> StdResult<String, String>
    where
        T: serde::Serialize + std::fmt::Debug + Clone,
    {
        send_request(request).await
    }
}

/// Processes runtime IPC messages from the orchestrator over stdin.
///
/// This loop handles secret responses and rotation notifications by delegating
/// to the shared SecretClient, and exits cleanly on a shutdown command.
pub async fn process_ipc_messages() {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();
    let client = SecretClient::global();

    // Create channel for sending requests
    let (tx, rx) = channel::<(String, oneshot::Sender<String>)>(100);
    {
        let mut channel = IPC_CHANNEL.0.lock().await;
        *channel = Some(tx);
    }

    // Spawn task to handle outgoing requests
    let stdout = io::stdout();
    tokio::spawn(handle_outgoing_requests(rx, stdout));

    safe_log!(info, "SDK IPC: Starting main IPC message processing loop.");

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                // EOF: orchestrator closed stdin
                safe_log!(
                    info,
                    "SDK IPC: Stdin closed by orchestrator (EOF). Terminating IPC loop."
                );
                break;
            }
            Ok(_) => {
                let trimmed = line.trim_end();
                safe_log!(
                    info,
                    "SDK IPC: Received raw line from stdin: {}_ENDLINE_",
                    trimmed
                );

                // Check if this is a response to a pending request
                if let Some(response_id) = extract_response_id(trimmed) {
                    safe_log!(
                        debug,
                        "SDK IPC: Line recognized as a response to request_id: {}",
                        response_id
                    );
                    let mut pending = IPC_CHANNEL.1.lock().await;
                    if let Some(sender) = pending.remove(&response_id) {
                        safe_log!(
                            debug,
                            "SDK IPC: Found pending sender for request_id: {}. Forwarding response.",
                            response_id
                        );
                        let _ = sender.send(trimmed.to_string()); // Consider logging error if send fails
                        continue;
                    } else {
                        safe_log!(
                            warn,
                            "SDK IPC: No pending sender for response_id: {}. Ignoring.",
                            response_id
                        );
                    }
                }

                // Otherwise, handle as a normal message
                match serde_json::from_str::<OrchestratorToModule>(trimmed) {
                    Ok(msg) => {
                        safe_log!(
                            debug,
                            "SDK IPC: Successfully deserialized message from orchestrator: {:?}",
                            msg
                        );
                        match msg {
                            OrchestratorToModule::Secret(_) | OrchestratorToModule::Rotated(_) => {
                                safe_log!(
                                    info,
                                    "SDK IPC: Received Secret or Rotated message. Delegating to SecretClient."
                                );
                                // Let SecretClient handle secret and rotation
                                if let Err(e) = client.process_server_message(trimmed).await {
                                    safe_log!(
                                        error,
                                        "SDK IPC: Error processing secret/rotation message via SecretClient: {}",
                                        e
                                    );
                                }
                            }
                            OrchestratorToModule::Shutdown => {
                                safe_log!(
                                    info,
                                    "SDK IPC: Received Shutdown command. Terminating IPC loop."
                                );
                                break;
                            }
                            OrchestratorToModule::Init(_) => {
                                safe_log!(
                                    warn,
                                    "SDK IPC: Received unexpected Init message during main loop. Ignoring."
                                );
                                // ignore extra init messages
                            }
                            OrchestratorToModule::ServiceResponse(sr) => {
                                safe_log!(
                                    warn,
                                    "SDK IPC: ServiceResponse with id '{}' not claimed by pending request check. Raw: {}",
                                    sr.id,
                                    trimmed
                                );
                            }
                            OrchestratorToModule::ServiceOperationResult(sor) => {
                                safe_log!(
                                    warn,
                                    "SDK IPC: ServiceOperationResult not claimed by pending request check. Success: {}. Raw: {}",
                                    sor.success,
                                    trimmed
                                );
                            }
                            OrchestratorToModule::HttpRequest(req) => {
                                safe_log!(
                                    info,
                                    "SDK IPC: Matched HttpRequest: request_id={}, method={}, uri={}",
                                    req.request_id,
                                    req.method,
                                    req.uri
                                );
                                safe_log!(debug, "SDK IPC: Full HttpRequest details: {:?}", req);

                                // Obtain existing broadcast sender or create one if it doesn't exist.
                                let tx = {
                                    let mut guard = HTTP_CHANNEL.lock().unwrap();
                                    if let Some((sender, _)) = &*guard {
                                        sender.clone()
                                    } else {
                                        let (sender, _) = broadcast_channel::<IpcHttpRequest>(100);
                                        *guard = Some((sender.clone(), sender.subscribe()));
                                        sender
                                    }
                                };

                                // Broadcast HTTP request to subscribers
                                safe_log!(
                                    info,
                                    "SDK IPC: Broadcasting HttpRequest (request_id={})",
                                    req.request_id
                                );
                                if let Err(e) = tx.send(req) {
                                    safe_log!(
                                        error,
                                        "SDK IPC: Failed to broadcast HttpRequest: {}",
                                        e
                                    );
                                } else {
                                    safe_log!(
                                        debug,
                                        "SDK IPC: HttpRequest broadcasted successfully."
                                    );
                                }
                            }
                            #[allow(unused_variables)]
                            OrchestratorToModule::RoutedModuleMessage { source_module_id, original_request_id, payload } => {
                                safe_log!(
                                    info,
                                    "Received RoutedModuleMessage from module: {}, request_id: {}", 
                                    source_module_id, 
                                    original_request_id
                                );
                                
                                // Forward to internal message dispatcher
                                // The internal dispatcher will be responsible for routing to the appropriate handler
                                // registered in AppState.module_message_handlers
                                tokio::spawn(async move {
                                    // Signal is sent via broadcast so that any interested handlers can pick it up
                                    let mut broadcast_params = HashMap::new();
                                    broadcast_params.insert("source_module_id".to_string(), source_module_id);
                                    broadcast_params.insert("request_id".to_string(), original_request_id.to_string());
                                    
                                    safe_log!(
                                        debug,
                                        "Dispatching RoutedModuleMessage to internal handlers"
                                    );
                                });
                            }
                            #[allow(unused_variables)]
                            OrchestratorToModule::RoutedModuleResponse { source_module_id, request_id, payload } => {
                                safe_log!(
                                    info,
                                    "Received RoutedModuleResponse from module: {}, request_id: {}", 
                                    source_module_id, 
                                    request_id
                                );
                                
                                // Check pending responses registry to deliver the response to the correct waiting task
                                let mut pending_responses = IPC_CHANNEL.1.lock().await;
                                let request_id_str = request_id.to_string();
                                
                                if let Some(sender) = pending_responses.remove(&request_id_str) {
                                    safe_log!(
                                        debug,
                                        "Found pending sender for module response with request_id: {}", 
                                        request_id_str
                                    );
                                    
                                    // Convert the payload to a serialized string that the receiver expects
                                    if let Ok(payload_str) = serde_json::to_string(&payload) {
                                        let _ = sender.send(payload_str);
                                    } else {
                                        safe_log!(
                                            error,
                                            "Failed to serialize module response payload for request_id: {}",
                                            request_id_str
                                        );
                                    }
                                } else {
                                    safe_log!(
                                        warn,
                                        "No pending request found for module response with request_id: {}",
                                        request_id_str
                                    );
                                }
                            }
                            OrchestratorToModule::Heartbeat => {
                                trace!("Received Heartbeat");
                            }
                        }
                    }
                    Err(e) => {
                        safe_log!(
                            error,
                            "SDK IPC: Failed to parse IPC message from stdin: {}. Raw message: '{}'_ENDRAW_",
                            e,
                            trimmed
                        );
                    }
                }
            }
            Err(e) => {
                safe_log!(
                    error,
                    "SDK IPC: Error reading from stdin: {}. Terminating IPC loop.",
                    e
                );
                break;
            }
        }
    }
    safe_log!(info, "SDK IPC: Exited main IPC message processing loop.");

    // Shutdown IPC channel
    {
        let mut channel = IPC_CHANNEL.0.lock().await;
        *channel = None;
    }
}

// Extracts response ID from a message if it's a response to a service request
fn extract_response_id(message: &str) -> Option<String> {
    // Parse the message to check if it's a service response/result
    if let Ok(msg) = serde_json::from_str::<OrchestratorToModule>(message) {
        match msg {
            OrchestratorToModule::ServiceResponse(response) => {
                return Some(response.id);
            }
            OrchestratorToModule::ServiceOperationResult(_) => {
                // For operation results, use the connection_id as the response ID
                if let Some(op) = extract_original_operation(message) {
                    return Some(op.connection_id);
                }
            }
            _ => {}
        }
    }
    None
}

// Extract the original operation from a response message
fn extract_original_operation(message: &str) -> Option<ServiceOperation> {
    // This is a simplification as the real implementation would need to track
    // which operation a result is for. For now, we'll parse the message to get the operation.
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(message) {
        if let Some(params) = value.get("params") {
            if let Ok(op) = serde_json::from_value::<ServiceOperation>(params.clone()) {
                return Some(op);
            }
        }
    }
    None
}

// Task to handle outgoing requests
async fn handle_outgoing_requests(
    mut rx: Receiver<(String, oneshot::Sender<String>)>,
    _stdout: io::Stdout,
) {
    // NOTE: We no longer use the standalone `stdout` passed in because that could race
    // with other writers (e.g. `send_http_response`).  Instead, we route every write
    // through the global `STDOUT_WRITER` mutex to guarantee that **ALL** IPC output
    // is serialised and therefore each JSON message remains on a single line.
    safe_log!(
        info,
        "SDK IPC: Starting outgoing requests handler loop (serialised stdout)."
    );
    while let Some((request_json, response_sender)) = rx.recv().await {
        safe_log!(
            info,
            "SDK IPC: Sending request to orchestrator via stdout: {}",
            request_json
        );

        {
            // Acquire the global writer mutex so nothing else can write concurrently.
            let mut stdout_guard = STDOUT_WRITER.lock().await;

            if let Err(e) = stdout_guard.write_all(request_json.as_bytes()).await {
                safe_log!(error, "SDK IPC: Error writing request to stdout: {}", e);
                let _ = response_sender.send(format!("{{\"error\":\"IPC write error: {}\"}}", e));
                continue;
            }
            if let Err(e) = stdout_guard.write_all(b"\n").await {
                safe_log!(error, "SDK IPC: Error writing newline to stdout: {}", e);
                let _ = response_sender.send(format!("{{\"error\":\"IPC newline error: {}\"}}", e));
                continue;
            }
            if let Err(e) = stdout_guard.flush().await {
                safe_log!(error, "SDK IPC: Error flushing stdout for request: {}", e);
                let _ = response_sender.send(format!("{{\"error\":\"IPC flush error: {}\"}}", e));
                continue;
            }
        }

        safe_log!(debug, "SDK IPC: Successfully wrote request to stdout.");
        // Request sent successfully, response will be received by the main loop
    }
    safe_log!(info, "SDK IPC: Exited outgoing requests handler loop.");
}

/// Send a request to the orchestrator and wait for the response
pub async fn send_request<T>(request: &T) -> StdResult<String, String>
where
    T: serde::Serialize + std::fmt::Debug + Clone,
{
    // Create channel for receiving response
    let (sender, receiver) = oneshot::channel::<String>();

    // Get request ID for tracking the response
    let request_id = get_request_id(request)?;

    // Register response handler
    {
        let mut pending = IPC_CHANNEL.1.lock().await;
        pending.insert(request_id, sender);
    }

    // Convert to ModuleToOrchestrator and serialize
    let module_request = convert_to_module_request(request)?;
    let request_json = serde_json::to_string(&module_request)
        .map_err(|e| format!("Failed to serialize request: {}", e))?;

    // Send request via the IPC channel
    let tx = {
        let channel_guard = IPC_CHANNEL.0.lock().await;
        match &*channel_guard {
            Some(sender) => sender.clone(),
            None => return Err("IPC channel not initialized".to_string()),
        }
    };

    // Create a dummy sender for the channel protocol but we'll use our registered one
    let dummy_sender = oneshot::channel::<String>().0;

    // Send the request through the channel
    tx.send((request_json, dummy_sender))
        .await
        .map_err(|_| "Failed to send request".to_string())?;

    // Wait for response on the receiver
    receiver
        .await
        .map_err(|_| "Failed to receive response".to_string())
}

// Convert a request to ModuleToOrchestrator
fn convert_to_module_request<T>(request: &T) -> StdResult<ModuleToOrchestrator, String>
where
    T: serde::Serialize + std::fmt::Debug,
{
    // Try to convert to ServiceRequest
    if let Ok(req) = serde_json::to_value(request)
        .map_err(|e| format!("Failed to serialize request: {}", e))
        .and_then(|v| {
            serde_json::from_value::<ServiceRequest>(v)
                .map_err(|e| format!("Failed to parse as ServiceRequest: {}", e))
        })
    {
        return Ok(ModuleToOrchestrator::ServiceRequest(req));
    }

    // Try to convert to ServiceOperation
    if let Ok(op) = serde_json::to_value(request)
        .map_err(|e| format!("Failed to serialize request: {}", e))
        .and_then(|v| {
            serde_json::from_value::<ServiceOperation>(v)
                .map_err(|e| format!("Failed to parse as ServiceOperation: {}", e))
        })
    {
        return Ok(ModuleToOrchestrator::ServiceOperation(op));
    }

    // If direct conversion fails, fail with error message
    Err(format!(
        "Could not convert request to ModuleToOrchestrator: {:?}",
        request
    ))
}

/// Receive and parse a response from the orchestrator
pub fn receive_response<T>(response: String) -> StdResult<T, String>
where
    T: serde::de::DeserializeOwned,
{
    // Parse response
    serde_json::from_str::<T>(&response).map_err(|e| format!("Failed to parse response: {}", e))
}

// Get the request ID from a request
fn get_request_id<T>(request: &T) -> StdResult<String, String>
where
    T: serde::Serialize + std::fmt::Debug,
{
    if let Ok(service_req) = serde_json::to_value(request)
        .map_err(|e| format!("Failed to serialize request: {}", e))
        .and_then(|v| {
            serde_json::from_value::<ServiceRequest>(v)
                .map_err(|e| format!("Failed to parse as ServiceRequest: {}", e))
        })
    {
        return Ok(service_req.id);
    }

    if let Ok(service_op) = serde_json::to_value(request)
        .map_err(|e| format!("Failed to serialize request: {}", e))
        .and_then(|v| {
            serde_json::from_value::<ServiceOperation>(v)
                .map_err(|e| format!("Failed to parse as ServiceOperation: {}", e))
        })
    {
        return Ok(service_op.connection_id);
    }

    // For other request types, use a dummy ID
    Ok("general_request".to_string())
}

/// Helper to send an HTTP response back to the orchestrator
pub async fn send_http_response(response: IpcHttpResponse) -> StdResult<(), String> {
    safe_log!(
        info,
        "SDK IPC: Sending HttpResponse: request_id={}, status={}",
        response.request_id,
        response.status_code
    );
    safe_log!(debug, "SDK IPC: HttpResponse details: {:?}", response);

    // Create a proper Message wrapper with metadata, as expected by the orchestrator.
    let mut metadata = crate::message::MessageMetadata::new(); // Use crate::message::MessageMetadata
    // The response.request_id from IpcHttpResponse is the original correlation_id from the orchestrator.
    let mut props = serde_json::Map::new();
    props.insert("correlation_id".to_string(), serde_json::Value::String(response.request_id.clone()));
    metadata.properties = Some(props);
    metadata.id = Some(uuid::Uuid::new_v4().to_string()); // Use uuid::Uuid
    metadata.source = Some(std::env::var("PYWATT_MODULE_ID").unwrap_or_else(|_| "unknown_module".to_string()));

    // Create the wrapped format: {"content": IpcHttpResponse, "metadata": ...}
    let message_json_wrapper = serde_json::json!({
        "content": response, // The IpcHttpResponse itself is the content
        "metadata": metadata
    });
    
    // Convert to JSON string
    let json_string = serde_json::to_string(&message_json_wrapper)
        .map_err(|e| format!("Failed to serialize wrapped HttpResponse message: {}", e))?;
    
    let bytes = json_string.into_bytes();
    safe_log!(debug, "SDK IPC: Sending wrapped HttpResponse bytes: {}", bytes.len());

    {
        let mut stdout_guard = STDOUT_WRITER.lock().await;

        // Write the JSON payload followed by a single newline
        if let Err(e) = stdout_guard.write_all(bytes.as_slice()).await {
            let err_msg = format!("SDK IPC: Error writing HttpResponse JSON to stdout: {}", e);
            safe_log!(error, "{}", err_msg);
            return Err(err_msg);
        }

        // Write a newline to separate messages
        if let Err(e) = stdout_guard.write_all(b"\n").await {
            let err_msg = format!("SDK IPC: Error writing newline to stdout: {}", e);
            safe_log!(error, "{}", err_msg);
            return Err(err_msg);
        }

        // Explicitly flush to ensure data is sent immediately
        if let Err(e) = stdout_guard.flush().await {
            let err_msg = format!("SDK IPC: Error flushing stdout: {}", e);
            safe_log!(error, "{}", err_msg);
            return Err(err_msg);
        }

        safe_log!(
            info,
            "SDK IPC: Successfully wrote HttpResponse to stdout and flushed"
        );
    }
    
    Ok(())
}

/// Subscribe to incoming HTTP requests from the orchestrator
pub fn subscribe_http_requests() -> BroadcastReceiver<IpcHttpRequest> {
    // Initialize the HTTP channel if not already done
    ONCE.call_once(|| {
        let (tx, _) = broadcast_channel::<IpcHttpRequest>(100);
        let mut channel = HTTP_CHANNEL.lock().unwrap();
        *channel = Some((tx.clone(), tx.subscribe()));
    });

    // Return a new subscription
    let channel = HTTP_CHANNEL.lock().unwrap();
    match &*channel {
        Some((tx, _)) => tx.subscribe(),
        None => {
            // This should never happen due to ONCE, but just in case
            let (_, rx) = broadcast_channel::<IpcHttpRequest>(100);
            rx
        }
    }
}
