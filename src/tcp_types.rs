//! Type definitions for TCP-based communication in PyWatt.
//!
//! This module provides type definitions for TCP-based communication, including 
//! connection configuration, reconnection policies, connection states, and 
//! error types.

use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;
use thiserror::Error;

use crate::message::MessageError;

/// Configuration for a TCP connection.
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// Host to connect to
    pub host: String,
    /// Port to connect to
    pub port: u16,
    /// Timeout for connection attempts and operations
    pub timeout: Duration,
    /// TLS configuration for secure connections
    pub tls_config: Option<TlsConfig>,
    /// Policy for reconnection attempts
    pub reconnect_policy: ReconnectPolicy,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 9000,
            timeout: Duration::from_secs(5),
            tls_config: None,
            reconnect_policy: ReconnectPolicy::ExponentialBackoff {
                initial_delay: Duration::from_millis(100),
                max_delay: Duration::from_secs(30),
                multiplier: 2.0,
            },
        }
    }
}

impl ConnectionConfig {
    /// Create a new connection configuration.
    pub fn new<S: Into<String>>(host: S, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            ..Default::default()
        }
    }

    /// Set the timeout for connection attempts and operations.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the TLS configuration.
    pub fn with_tls_config(mut self, tls_config: TlsConfig) -> Self {
        self.tls_config = Some(tls_config);
        self
    }

    /// Set the reconnection policy.
    pub fn with_reconnect_policy(mut self, policy: ReconnectPolicy) -> Self {
        self.reconnect_policy = policy;
        self
    }

    /// Get the socket address from the configuration.
    pub fn socket_addr(&self) -> std::io::Result<SocketAddr> {
        let addr_str = format!("{}:{}", self.host, self.port);
        addr_str.parse().map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid socket address: {}", e),
            )
        })
    }
}

/// TLS configuration for secure connections.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Path to CA certificate file
    pub ca_cert_path: Option<String>,
    /// Path to client certificate file
    pub client_cert_path: Option<String>,
    /// Path to client key file
    pub client_key_path: Option<String>,
    /// Whether to verify the server certificate
    pub verify_server: bool,
    /// Hostname to verify in the server certificate
    pub server_name: Option<String>,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            verify_server: true,
            server_name: None,
        }
    }
}

impl TlsConfig {
    /// Create a new TLS configuration.
    pub fn new() -> Self {
        Default::default()
    }

    /// Set the CA certificate path.
    pub fn with_ca_cert_path<S: Into<String>>(mut self, path: S) -> Self {
        self.ca_cert_path = Some(path.into());
        self
    }

    /// Set the client certificate path.
    pub fn with_client_cert_path<S: Into<String>>(mut self, path: S) -> Self {
        self.client_cert_path = Some(path.into());
        self
    }

    /// Set the client key path.
    pub fn with_client_key_path<S: Into<String>>(mut self, path: S) -> Self {
        self.client_key_path = Some(path.into());
        self
    }

    /// Set whether to verify the server certificate.
    pub fn with_verify_server(mut self, verify: bool) -> Self {
        self.verify_server = verify;
        self
    }

    /// Set the server name for SNI.
    pub fn with_server_name<S: Into<String>>(mut self, name: S) -> Self {
        self.server_name = Some(name.into());
        self
    }
}

/// Reconnection policy for handling connection failures.
#[derive(Debug, Clone)]
pub enum ReconnectPolicy {
    /// No reconnection - fail immediately
    None,
    /// Reconnect with a fixed interval between attempts
    FixedInterval {
        /// Delay between reconnection attempts
        delay: Duration,
        /// Maximum number of attempts, or None for unlimited
        max_attempts: Option<u32>,
    },
    /// Reconnect with exponential backoff
    ExponentialBackoff {
        /// Initial delay for the first reconnection attempt
        initial_delay: Duration,
        /// Maximum delay between reconnection attempts
        max_delay: Duration,
        /// Multiplier for the delay after each failed attempt
        multiplier: f64,
    },
}

/// Current state of a TCP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connection is connected
    Connected,
    /// Connection is disconnected
    Disconnected,
    /// Connection is in the process of connecting
    Connecting,
    /// Connection has failed and will not reconnect
    Failed,
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionState::Connected => write!(f, "Connected"),
            ConnectionState::Disconnected => write!(f, "Disconnected"),
            ConnectionState::Connecting => write!(f, "Connecting"),
            ConnectionState::Failed => write!(f, "Failed"),
        }
    }
}

/// Network-related errors.
#[derive(Debug, Error)]
pub enum NetworkError {
    /// Connection error
    #[error("Connection error: {0}")]
    ConnectionError(String),

    /// Connection timeout
    #[error("Connection timeout after {0:?}")]
    ConnectionTimeout(Duration),

    /// Connection closed
    #[error("Connection closed")]
    ConnectionClosed,

    /// Connection failed
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Message error
    #[error("Message error: {0}")]
    MessageError(#[from] MessageError),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// TLS error
    #[error("TLS error: {0}")]
    TlsError(String),

    /// Reconnection failed
    #[error("Reconnection failed after {0} attempts: {1}")]
    ReconnectionFailed(u32, String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Channel error
    #[error("Channel error: {0}")]
    ChannelError(String),
}

/// Result type for network operations
pub type NetworkResult<T> = Result<T, NetworkError>; 