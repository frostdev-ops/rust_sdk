use pywatt_sdk::message::{
    EncodedMessage, EncodedStream, EncodingFormat, Message, MessageMetadata,
};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Command {
    action: String,
    params: CommandParams,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CommandParams {
    timeout_ms: u32,
    retry: bool,
    tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Response {
    status: String,
    code: u32,
    data: Option<serde_json::Value>,
}

// Implement bincode traits for Command
impl bincode::Encode for Command {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        bincode::Encode::encode(&self.action, encoder)?;
        bincode::Encode::encode(&self.params, encoder)?;
        Ok(())
    }
}

impl bincode::Decode<()> for Command {
    fn decode<D: bincode::de::Decoder<Context = ()>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let action = String::decode(decoder)?;
        let params = CommandParams::decode(decoder)?;
        Ok(Command { action, params })
    }
}

// Implement bincode traits for CommandParams
impl bincode::Encode for CommandParams {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        bincode::Encode::encode(&self.timeout_ms, encoder)?;
        bincode::Encode::encode(&self.retry, encoder)?;
        bincode::Encode::encode(&self.tags, encoder)?;
        Ok(())
    }
}

impl bincode::Decode<()> for CommandParams {
    fn decode<D: bincode::de::Decoder<Context = ()>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let timeout_ms = u32::decode(decoder)?;
        let retry = bool::decode(decoder)?;
        let tags = Vec::<String>::decode(decoder)?;
        Ok(CommandParams {
            timeout_ms,
            retry,
            tags,
        })
    }
}

// Implement bincode traits for Response
impl bincode::Encode for Response {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        bincode::Encode::encode(&self.status, encoder)?;
        bincode::Encode::encode(&self.code, encoder)?;
        let data_json = match &self.data {
            Some(v) => serde_json::to_string(v).unwrap_or_default(),
            None => String::new(),
        };
        bincode::Encode::encode(&data_json, encoder)?;
        Ok(())
    }
}

impl bincode::Decode<()> for Response {
    fn decode<D: bincode::de::Decoder<Context = ()>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let status = String::decode(decoder)?;
        let code = u32::decode(decoder)?;
        let data_json = String::decode(decoder)?;
        let data = if !data_json.is_empty() {
            serde_json::from_str(&data_json).ok()
        } else {
            None
        };
        Ok(Response { status, code, data })
    }
}

// Helper function to get current Unix timestamp
fn get_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

async fn run_simple_example() {
    println!("Running simple message encoding/decoding example");

    // Create a command message
    let command = Command {
        action: "start_service".to_string(),
        params: CommandParams {
            timeout_ms: 5000,
            retry: true,
            tags: vec!["important".to_string(), "high-priority".to_string()],
        },
    };

    // Create metadata
    let metadata = MessageMetadata::new()
        .with_id("cmd-1")
        .with_source("client")
        .with_destination("server")
        .with_timestamp(get_timestamp());

    // Create a message with metadata
    let message = Message::with_metadata(command, metadata);

    // Encode using different formats
    let json_encoded = message.clone().encode().unwrap();
    let binary_encoded = EncodedMessage::from_message_binary(message.clone()).unwrap();
    let base64_encoded = EncodedMessage::from_message_base64(message).unwrap();

    println!(
        "Message encoded in JSON format: {} bytes",
        json_encoded.data().len()
    );
    println!(
        "Message encoded in Binary format: {} bytes",
        binary_encoded.data().len()
    );
    println!(
        "Message encoded in Base64 format: {} bytes",
        base64_encoded.data().len()
    );

    // Decode the message (JSON format)
    let decoded_message: Message<Command> = json_encoded.decode_full().unwrap();
    println!("Decoded message:");
    println!("  Action: {}", decoded_message.content().action);
    println!(
        "  Timeout: {} ms",
        decoded_message.content().params.timeout_ms
    );
    println!("  Retry: {}", decoded_message.content().params.retry);
    println!("  Tags: {:?}", decoded_message.content().params.tags);

    if let Some(metadata) = decoded_message.metadata() {
        println!("Message metadata:");
        println!("  ID: {:?}", metadata.id);
        println!("  Source: {:?}", metadata.source);
        println!("  Destination: {:?}", metadata.destination);
        println!("  Timestamp: {:?}", metadata.timestamp);
    }

    // Convert between formats
    let converted_to_binary = json_encoded.to_format(EncodingFormat::Binary).unwrap();
    println!(
        "Converted from JSON to Binary: {} bytes",
        converted_to_binary.data().len()
    );

    let converted_back = converted_to_binary.to_format(EncodingFormat::Json).unwrap();
    let decoded_after_conversion: Command = converted_back.decode().unwrap();
    println!(
        "Successfully decoded after format conversion: {}",
        decoded_after_conversion.action
    );

    // Write to a buffer and read back
    let mut buffer = Vec::new();
    json_encoded.write_to(&mut buffer).unwrap();
    println!("Wrote message to buffer: {} bytes", buffer.len());

    let mut cursor = Cursor::new(buffer);
    let read_message = EncodedMessage::read_from(&mut cursor).unwrap();
    let decoded_from_buffer: Command = read_message.decode().unwrap();
    println!(
        "Read and decoded from buffer: {}",
        decoded_from_buffer.action
    );
}

async fn run_tcp_example() {
    println!("\nRunning TCP messaging example");

    // Spawn a TCP server
    let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = server.local_addr().unwrap();

    println!("Server listening on {}", server_addr);

    // Spawn server task
    let server_handle = tokio::spawn(async move {
        if let Ok((stream, _)) = server.accept().await {
            handle_client(stream).await;
        }
    });

    // Short delay to ensure server is ready
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Connect client
    let client_stream = TcpStream::connect(server_addr).await.unwrap();
    println!("Client connected to server");

    // Run client
    run_client(client_stream).await;

    // Wait for server to finish
    let _ = server_handle.await;
}

async fn handle_client(mut stream: TcpStream) {
    // Read the encoded message directly from the stream
    let (mut reader, mut writer) = stream.split();

    match EncodedMessage::read_from_async(&mut reader).await {
        Ok(encoded) => {
            println!("Server received a message");

            // Decode the message
            if let Ok(command) = encoded.decode::<Command>() {
                println!("Server received command: {}", command.action);

                // Create a response
                let response = Response {
                    status: "success".to_string(),
                    code: 200,
                    data: Some(serde_json::json!({
                        "message": "Command processed successfully",
                        "timestamp": get_timestamp(),
                    })),
                };

                // Create a response message
                let metadata = MessageMetadata::new()
                    .with_id("resp-1")
                    .with_source("server")
                    .with_destination("client")
                    .with_timestamp(get_timestamp());

                let response_message = Message::with_metadata(response, metadata);

                // Encode and send the response
                if let Ok(encoded_response) = response_message.encode() {
                    if let Err(e) = encoded_response.write_to_async(&mut writer).await {
                        println!("Error sending response: {}", e);
                    } else {
                        println!("Server sent response");
                    }
                }
            }
        }
        Err(e) => println!("Error reading message: {}", e),
    }
}

async fn run_client(mut stream: TcpStream) {
    // Create a command
    let command = Command {
        action: "fetch_data".to_string(),
        params: CommandParams {
            timeout_ms: 3000,
            retry: true,
            tags: vec!["data".to_string(), "query".to_string()],
        },
    };

    // Create metadata
    let metadata = MessageMetadata::new()
        .with_id("cmd-2")
        .with_source("client")
        .with_destination("server")
        .with_timestamp(get_timestamp());

    // Create a message
    let message = Message::with_metadata(command, metadata);

    // Encode the message
    let encoded = message.encode().unwrap();

    // Split the stream for reading and writing
    let (mut reader, mut writer) = stream.split();

    // Send the message
    encoded.write_to_async(&mut writer).await.unwrap();
    println!("Client sent command");

    // Read the response
    let response = EncodedMessage::read_from_async(&mut reader).await.unwrap();
    let response_message: Message<Response> = response.decode_full().unwrap();

    println!("Client received response:");
    println!("  Status: {}", response_message.content().status);
    println!("  Code: {}", response_message.content().code);

    if let Some(data) = &response_message.content().data {
        println!("  Data: {}", data);
    }

    if let Some(metadata) = response_message.metadata() {
        println!("  From: {:?}", metadata.source);
        println!("  To: {:?}", metadata.destination);
    }
}

async fn run_stream_example() {
    println!("\nRunning message stream example");

    // Create a stream
    let mut stream = EncodedStream::<String>::new();

    // Create a Vec to use as a buffer
    let buffer = Vec::new();

    // Set up a writer to the buffer
    let buffer_for_writer = buffer.clone();
    stream.to_writer(buffer_for_writer).unwrap();

    // Send some messages
    for i in 1..=5 {
        let message = Message::new(format!("Message {}", i));
        stream.send(message).await.unwrap();
        println!("Sent message {}", i);
    }

    // Short wait to ensure messages are processed
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Create a reader from a copy of the buffer contents
    let mut read_stream = EncodedStream::<String>::from_reader(Cursor::new(buffer.clone())).await;

    // Read messages from the stream
    if let Some(mut receiver) = read_stream.receiver() {
        let mut count = 0;
        while let Some(Ok(encoded)) = receiver.recv().await {
            let message: String = encoded.decode().unwrap();
            println!("Received: {}", message);
            count += 1;

            if count >= 5 {
                break;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    // Run the simple example
    run_simple_example().await;

    // Run the TCP example
    run_tcp_example().await;

    // Run the streaming example
    run_stream_example().await;

    println!("\nAll examples completed successfully!");
}
