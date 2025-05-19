# PyWatt SDK Message Module

The PyWatt SDK Message Module provides a standardized structure for encoding and decoding messages between modules and the orchestration component. It offers a simple API that makes it easy to transmit structured data over IPC, TCP, or any other protocol.

## Key Features

- Generic message container with optional metadata
- Multiple encoding formats (JSON, binary, base64)
- Synchronous and asynchronous I/O
- Streaming support for continuous message exchanges
- Format conversion capabilities
- Comprehensive error handling

## Basic Usage

### Creating and Encoding Messages

```rust
use pywatt_sdk::message::{Message, EncodedMessage};
use serde::{Serialize, Deserialize};

// Define a serializable type
#[derive(Serialize, Deserialize)]
struct MyData {
    command: String,
    value: i32,
}

// Create a message with content
let data = MyData {
    command: "start",
    value: 42,
};
let message = Message::new(data);

// Encode the message (default: JSON format)
let encoded = message.encode().unwrap();

// Encode in different formats
let binary_encoded = EncodedMessage::from_message_binary(message.clone()).unwrap();
let base64_encoded = EncodedMessage::from_message_base64(message).unwrap();
```

### Decoding Messages

```rust
// Decode to the original type
let decoded: MyData = encoded.decode().unwrap();
assert_eq!(decoded.command, "start");
assert_eq!(decoded.value, 42);

// Decode to a Message<T> to access metadata
let full_message: Message<MyData> = encoded.decode_full().unwrap();
```

### Using Metadata

```rust
use pywatt_sdk::message::MessageMetadata;
use chrono::Utc;

// Create metadata
let metadata = MessageMetadata::new()
    .with_id("msg-123")
    .with_source("client")
    .with_destination("server")
    .with_timestamp(Utc::now().timestamp());

// Create a message with metadata
let message_with_metadata = Message::with_metadata(data, metadata);

// Access metadata after decoding
if let Some(meta) = full_message.metadata() {
    println!("Message ID: {:?}", meta.id);
    println!("Source: {:?}", meta.source);
}
```

### Format Conversion

```rust
// Convert between formats
let json_encoded = message.encode().unwrap();
let binary_version = json_encoded.to_format(EncodingFormat::Binary).unwrap();
let back_to_json = binary_version.to_format(EncodingFormat::Json).unwrap();
```

### Writing to and Reading from Streams

```rust
use std::io::Cursor;

// Write to a buffer
let mut buffer = Vec::new();
encoded.write_to(&mut buffer).unwrap();

// Read from a buffer
let mut cursor = Cursor::new(buffer);
let read_message = EncodedMessage::read_from(&mut cursor).unwrap();
let decoded_from_stream: MyData = read_message.decode().unwrap();
```

### Async I/O

```rust
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// Write asynchronously
let mut buffer = Vec::new();
encoded.write_to_async(&mut buffer).await.unwrap();

// Read asynchronously
let mut cursor = Cursor::new(buffer);
let read_message = EncodedMessage::read_from_async(&mut cursor).await.unwrap();
```

## Streaming Messages

The `EncodedStream` type provides a higher-level API for working with continuous streams of messages.

```rust
use pywatt_sdk::message::EncodedStream;
use tokio::net::{TcpListener, TcpStream};

// Create a stream
let mut stream = EncodedStream::<MyData>::new();

// Create a stream from a reader
let tcp_stream = TcpStream::connect("127.0.0.1:8080").await.unwrap();
let message_stream = EncodedStream::<MyData>::from_reader(tcp_stream).await;

// Send a message to the stream
let message = Message::new(MyData { command: "ping", value: 1 });
stream.send(message).await.unwrap();

// Receive messages from the stream
if let Some(mut receiver) = message_stream.receiver() {
    while let Some(Ok(encoded)) = receiver.recv().await {
        let data: MyData = encoded.decode().unwrap();
        println!("Received: {}", data.command);
    }
}
```

## Error Handling

The module uses a comprehensive error type `MessageError` that wraps various error conditions:

```rust
pub enum MessageError {
    SerdeError(serde_json::Error),
    IoError(io::Error),
    EncodingError(String),
    TypeMismatch,
    InvalidFormat,
    BinaryConversionError(bincode::error::EncodeError),
    BinaryDecodingError(bincode::error::DecodeError),
    Base64Error(base64::DecodeError),
    ChannelError,
}
```

## Integration with PyWatt IPC

The message module can be used in conjunction with PyWatt's existing IPC mechanism:

```rust
use pywatt_sdk::message::{Message, EncodedMessage};
use pywatt_sdk::ipc::send_http_response;
use pywatt_sdk::ipc_types::IpcHttpResponse;

// Receive an IPC HTTP request
let http_request = /* ... */;

// Process the request
let command: MyCommand = serde_json::from_slice(&http_request.body.unwrap()).unwrap();

// Create a message with the response
let response = MyResponse { status: "success" };
let message = Message::new(response);
let encoded = message.encode().unwrap();

// Send back the response
let http_response = IpcHttpResponse {
    request_id: http_request.request_id,
    status_code: 200,
    headers: Default::default(),
    body: Some(encoded.data().to_vec()),
};
send_http_response(http_response).await.unwrap();
```

## Complete Example

See the `examples/message_example.rs` file for a complete example that demonstrates:

1. Basic message encoding and decoding
2. TCP client/server communication using messages
3. Message streaming

## Advanced Usage Patterns

### Metadata Enrichment Pipeline

You can create a pipeline that enriches messages with metadata:

```rust
use pywatt_sdk::message::{Message, MessageMetadata};
use chrono::Utc;
use uuid::Uuid;

fn enrich_message<T>(message: &mut Message<T>) {
    let meta = message.ensure_metadata();
    
    // Add message ID if not present
    if meta.id.is_none() {
        meta.id = Some(Uuid::new_v4().to_string());
    }
    
    // Add timestamp if not present
    if meta.timestamp.is_none() {
        meta.timestamp = Some(Utc::now().timestamp());
    }
    
    // Add source information
    if meta.source.is_none() {
        meta.source = Some("my-service".to_string());
    }
}

// Use the enrichment pipeline
let mut message = Message::new(MyData { /* ... */ });
enrich_message(&mut message);
let encoded = message.encode().unwrap();
```

### Message Format Selection

You can implement automatic format selection based on content type:

```rust
use pywatt_sdk::message::{Message, EncodedMessage, EncodingFormat};

enum ContentType {
    Json,
    Binary,
    Text,
}

fn select_optimal_format<T: Serialize + bincode::Encode>(
    message: Message<T>,
    content_type: ContentType,
) -> MessageResult<EncodedMessage> {
    match content_type {
        ContentType::Json => message.encode(),
        ContentType::Binary => EncodedMessage::from_message_binary(message),
        ContentType::Text => EncodedMessage::from_message_base64(message),
    }
}

// Use format selection
let message = Message::new(MyData { /* ... */ });
let encoded = select_optimal_format(message, ContentType::Binary)?;
```

### Request-Response Pattern

Implement a simple request-response pattern with correlation IDs:

```rust
use pywatt_sdk::message::{Message, MessageMetadata};
use uuid::Uuid;

// Create a request with a correlation ID
fn create_request<T: Serialize>(payload: T) -> MessageResult<EncodedMessage> {
    let metadata = MessageMetadata::new()
        .with_id(Uuid::new_v4().to_string())
        .with_timestamp(chrono::Utc::now().timestamp());
        
    let message = Message::with_metadata(payload, metadata);
    message.encode()
}

// Create a response that includes the original request's correlation ID
fn create_response<Req, Res>(
    request: &Message<Req>,
    response_payload: Res,
) -> MessageResult<EncodedMessage>
where
    Req: Serialize,
    Res: Serialize,
{
    let request_id = match request.metadata().and_then(|m| m.id.as_ref()) {
        Some(id) => id.clone(),
        None => Uuid::new_v4().to_string(),
    };
    
    let metadata = MessageMetadata::new()
        .with_id(Uuid::new_v4().to_string())
        .with_property("correlation_id", request_id)?
        .with_timestamp(chrono::Utc::now().timestamp());
        
    let message = Message::with_metadata(response_payload, metadata);
    message.encode()
}
```

### Error Handling with Metadata

Create a standardized approach to error handling with message metadata:

```rust
use pywatt_sdk::message::{Message, MessageMetadata, MessageError};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct ErrorInfo {
    code: String,
    message: String,
    details: Option<serde_json::Value>,
}

fn create_error_message<T: Serialize>(
    error_code: &str, 
    error_message: &str,
    correlation_id: Option<String>,
) -> MessageResult<EncodedMessage> {
    let error = ErrorInfo {
        code: error_code.to_string(),
        message: error_message.to_string(),
        details: None,
    };
    
    let mut metadata = MessageMetadata::new()
        .with_id(Uuid::new_v4().to_string())
        .with_timestamp(chrono::Utc::now().timestamp())
        .with_property("error", true)?;
        
    if let Some(id) = correlation_id {
        metadata = metadata.with_property("correlation_id", id)?;
    }
    
    let message = Message::with_metadata(error, metadata);
    message.encode()
}
```

## Implementation Notes

### Performance Considerations

- **JSON encoding** is human-readable but less efficient for binary data and has higher parsing overhead
- **Binary encoding** (via bincode) is significantly more compact and faster to encode/decode
- **Base64 encoding** adds ~33% overhead to binary data but allows it to be transmitted over text-only channels
- For large messages, consider using **streaming** to avoid memory spikes
- When working with small messages at high frequency, prefer **binary encoding** for best performance
- Consider **pooling** message buffers for high-throughput scenarios

### Thread Safety

All types in the message module are designed to be thread-safe:

- `Message<T>` implements `Clone` when `T: Clone`
- `EncodedMessage` implements `Clone` by default
- `EncodedStream<T>` uses tokio channels that are thread-safe
- All send/receive operations are properly synchronized

### Memory Management

To manage memory effectively, especially for large messages:

- Use `EncodedStream<T>` for large or frequent messages
- Implement backpressure handling in your send/receive loops
- Consider chunking large messages with custom metadata to track reassembly
- Reuse buffers when possible to reduce allocations
- Use the `with_capacity` pattern when creating buffers that will grow

### Serialization Compatibility

For seamless interoperation:

- Ensure types implement both `serde::Serialize` and `serde::Deserialize`
- For binary encoding, also implement `bincode::Encode` and `bincode::Decode<()>`
- Maintain backward compatibility when evolving message schemas
- Consider using version fields in your message schemas
- Document schema changes carefully

## Future Improvements

- Support for message compression algorithms
- Schema validation with custom error reporting
- Integration with OpenTelemetry for distributed tracing
- Message priority and quality-of-service flags
- Circuit breaker pattern for fault tolerance

## License

This module is part of the PyWatt SDK and is licensed under the same terms as the rest of the SDK. 