#[cfg(test)]
mod tests {
    use crate::{AnnouncedEndpoint, ModuleAnnounce, announce::send_announce};
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    // Mock for stdout to capture output
    #[allow(dead_code)]
    struct MockStdout {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for MockStdout {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut buffer = self.buffer.lock().unwrap();
            buffer.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    // Test sending an announce message
    #[test]
    fn test_send_announce() {
        // Create a simple announce message
        let announce = ModuleAnnounce {
            listen: "tcp://127.0.0.1:8080".to_string(),
            endpoints: vec![AnnouncedEndpoint {
                path: "/api/test".to_string(),
                methods: vec!["GET".to_string()],
                auth: None,
            }],
        };

        // We can't easily capture stdout in the test, so we'll just verify
        // that the function doesn't panic or return an error
        let result = send_announce(&announce);
        assert!(result.is_ok());

        // For a more thorough test, we would need to use a custom test harness
        // that captures stdout, or refactor the function to take a writer
    }

    // Test the announce serialization format
    #[test]
    fn test_announce_serialization() {
        let announce = ModuleAnnounce {
            listen: "tcp://127.0.0.1:8080".to_string(),
            endpoints: vec![
                AnnouncedEndpoint {
                    path: "/api/test".to_string(),
                    methods: vec!["GET".to_string()],
                    auth: None,
                },
                AnnouncedEndpoint {
                    path: "/api/protected".to_string(),
                    methods: vec!["POST".to_string()],
                    auth: Some("jwt".to_string()),
                },
            ],
        };

        // Verify the JSON serialization format
        let json = serde_json::to_string(&announce).unwrap();

        // Check that it contains the expected fields
        assert!(json.contains("listen"));
        assert!(json.contains("tcp://127.0.0.1:8080"));
        assert!(json.contains("endpoints"));
        assert!(json.contains("/api/test"));
        assert!(json.contains("GET"));
        assert!(json.contains("/api/protected"));
        assert!(json.contains("POST"));
        assert!(json.contains("jwt"));

        // Verify we can deserialize back to the same struct
        let deserialized: ModuleAnnounce = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.listen, announce.listen);
        assert_eq!(deserialized.endpoints.len(), announce.endpoints.len());
        assert_eq!(deserialized.endpoints[0].path, announce.endpoints[0].path);
        assert_eq!(deserialized.endpoints[1].path, announce.endpoints[1].path);
    }
}
