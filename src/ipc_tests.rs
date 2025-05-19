#[cfg(test)]
mod tests {
    use crate::ipc_types::{
        AnnounceBlob, EndpointAnnounce, GetSecretRequest, InitBlob, ListenAddress,
        ModuleToOrchestrator, OrchestratorToModule, RotatedNotification, RotationAckRequest,
        SecretValueResponse,
    };
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::path::PathBuf;

    #[test]
    fn test_listen_address_serialization() {
        // Test TCP address
        let tcp_addr = ListenAddress::Tcp(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            8080,
        ));
        let json = serde_json::to_string(&tcp_addr).unwrap();
        let deserialized: ListenAddress = serde_json::from_str(&json).unwrap();

        match deserialized {
            ListenAddress::Tcp(addr) => {
                assert_eq!(addr.ip(), IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
                assert_eq!(addr.port(), 8080);
            }
            _ => panic!("Expected TCP address"),
        }

        // Test Unix socket path
        let unix_path = ListenAddress::Unix(PathBuf::from("/tmp/test.sock"));
        let json = serde_json::to_string(&unix_path).unwrap();
        let deserialized: ListenAddress = serde_json::from_str(&json).unwrap();

        match deserialized {
            ListenAddress::Unix(path) => {
                assert_eq!(path, PathBuf::from("/tmp/test.sock"));
            }
            _ => panic!("Expected Unix path"),
        }
    }

    #[test]
    fn test_init_blob_serialization() {
        let mut env = HashMap::new();
        env.insert("KEY1".to_string(), "VALUE1".to_string());
        env.insert("KEY2".to_string(), "VALUE2".to_string());

        let init = InitBlob {
            orchestrator_api: "http://localhost:8000".to_string(),
            module_id: "test-module".to_string(),
            env,
            listen: ListenAddress::Tcp(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8080,
            )),
        };

        let json = serde_json::to_string(&init).unwrap();
        let deserialized: InitBlob = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.orchestrator_api, "http://localhost:8000");
        assert_eq!(deserialized.module_id, "test-module");
        assert_eq!(deserialized.env.len(), 2);
        assert_eq!(deserialized.env.get("KEY1").unwrap(), "VALUE1");
        assert_eq!(deserialized.env.get("KEY2").unwrap(), "VALUE2");
    }

    #[test]
    fn test_announce_blob_serialization() {
        let endpoints = vec![
            EndpointAnnounce {
                path: "/api/v1/test".to_string(),
                methods: vec!["GET".to_string(), "POST".to_string()],
                auth: Some("jwt".to_string()),
            },
            EndpointAnnounce {
                path: "/api/v1/health".to_string(),
                methods: vec!["GET".to_string()],
                auth: None,
            },
        ];

        let announce = AnnounceBlob {
            listen: "127.0.0.1:8080".to_string(),
            endpoints,
        };

        let json = serde_json::to_string(&announce).unwrap();
        let deserialized: AnnounceBlob = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.listen, "127.0.0.1:8080");
        assert_eq!(deserialized.endpoints.len(), 2);

        assert_eq!(deserialized.endpoints[0].path, "/api/v1/test");
        assert_eq!(deserialized.endpoints[0].methods.len(), 2);
        assert_eq!(deserialized.endpoints[0].methods[0], "GET");
        assert_eq!(deserialized.endpoints[0].methods[1], "POST");
        assert_eq!(deserialized.endpoints[0].auth, Some("jwt".to_string()));

        assert_eq!(deserialized.endpoints[1].path, "/api/v1/health");
        assert_eq!(deserialized.endpoints[1].methods.len(), 1);
        assert_eq!(deserialized.endpoints[1].methods[0], "GET");
        assert_eq!(deserialized.endpoints[1].auth, None);
    }

    #[test]
    fn test_module_to_orchestrator_serialization() {
        // Test Announce message
        let endpoints = vec![EndpointAnnounce {
            path: "/api/v1/test".to_string(),
            methods: vec!["GET".to_string()],
            auth: None,
        }];

        let announce = AnnounceBlob {
            listen: "127.0.0.1:8080".to_string(),
            endpoints,
        };

        let msg = ModuleToOrchestrator::Announce(announce);
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ModuleToOrchestrator = serde_json::from_str(&json).unwrap();

        match deserialized {
            ModuleToOrchestrator::Announce(announce_blob) => {
                assert_eq!(announce_blob.listen, "127.0.0.1:8080");
                assert_eq!(announce_blob.endpoints.len(), 1);
            }
            _ => panic!("Expected Announce message"),
        }

        // Test GetSecret message
        let secret_req = GetSecretRequest {
            name: "SECRET_KEY".to_string(),
        };

        let msg = ModuleToOrchestrator::GetSecret(secret_req);
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ModuleToOrchestrator = serde_json::from_str(&json).unwrap();

        match deserialized {
            ModuleToOrchestrator::GetSecret(req) => {
                assert_eq!(req.name, "SECRET_KEY");
            }
            _ => panic!("Expected GetSecret message"),
        }

        // Test RotationAck message
        let rotation_ack = RotationAckRequest {
            rotation_id: "rotation-123".to_string(),
            status: "success".to_string(),
            message: Some("Rotation completed".to_string()),
        };

        let msg = ModuleToOrchestrator::RotationAck(rotation_ack);
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ModuleToOrchestrator = serde_json::from_str(&json).unwrap();

        match deserialized {
            ModuleToOrchestrator::RotationAck(ack) => {
                assert_eq!(ack.rotation_id, "rotation-123");
                assert_eq!(ack.status, "success");
                assert_eq!(ack.message, Some("Rotation completed".to_string()));
            }
            _ => panic!("Expected RotationAck message"),
        }
    }

    #[test]
    fn test_orchestrator_to_module_serialization() {
        // Test Init message
        let mut env = HashMap::new();
        env.insert("KEY1".to_string(), "VALUE1".to_string());

        let init = InitBlob {
            orchestrator_api: "http://localhost:8000".to_string(),
            module_id: "test-module".to_string(),
            env,
            listen: ListenAddress::Tcp(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8080,
            )),
        };

        let msg = OrchestratorToModule::Init(init);
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: OrchestratorToModule = serde_json::from_str(&json).unwrap();

        match deserialized {
            OrchestratorToModule::Init(init_blob) => {
                assert_eq!(init_blob.orchestrator_api, "http://localhost:8000");
                assert_eq!(init_blob.module_id, "test-module");
                assert_eq!(init_blob.env.len(), 1);
            }
            _ => panic!("Expected Init message"),
        }

        // Test Secret message
        let secret = SecretValueResponse {
            name: "SECRET_KEY".to_string(),
            value: "secret-value".to_string(),
            rotation_id: None,
        };

        let msg = OrchestratorToModule::Secret(secret);
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: OrchestratorToModule = serde_json::from_str(&json).unwrap();

        match deserialized {
            OrchestratorToModule::Secret(resp) => {
                assert_eq!(resp.name, "SECRET_KEY");
                assert_eq!(resp.value, "secret-value");
                assert_eq!(resp.rotation_id, None);
            }
            _ => panic!("Expected Secret message"),
        }

        // Test Rotated message
        let rotated = RotatedNotification {
            keys: vec!["SECRET1".to_string(), "SECRET2".to_string()],
            rotation_id: "rotation-123".to_string(),
        };

        let msg = OrchestratorToModule::Rotated(rotated);
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: OrchestratorToModule = serde_json::from_str(&json).unwrap();

        match deserialized {
            OrchestratorToModule::Rotated(notif) => {
                assert_eq!(notif.keys.len(), 2);
                assert_eq!(notif.keys[0], "SECRET1");
                assert_eq!(notif.keys[1], "SECRET2");
                assert_eq!(notif.rotation_id, "rotation-123");
            }
            _ => panic!("Expected Rotated message"),
        }

        // Test Shutdown message
        let msg = OrchestratorToModule::Shutdown;
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: OrchestratorToModule = serde_json::from_str(&json).unwrap();

        match deserialized {
            OrchestratorToModule::Shutdown => {}
            _ => panic!("Expected Shutdown message"),
        }
    }
}
