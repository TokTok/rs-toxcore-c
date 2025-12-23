use merkle_tox_core::Transport;
use merkle_tox_core::vfs::StdFileSystem;
use merkle_tox_fs::FsStore;
use merkle_tox_tox::ToxMerkleBridge;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use toxcore::tox::{Options, Tox};
use toxcore::types::ToxConnection;

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
}

struct TestNode {
    bridge: ToxMerkleBridge<FsStore>,
    _dir: TempDir,
}

impl TestNode {
    fn new(name: &str) -> Self {
        let dir = TempDir::new().unwrap();
        let mut opts = Options::new().unwrap();
        opts.set_udp_enabled(true);
        opts.set_tcp_port(0);
        opts.set_local_discovery_enabled(true);
        opts.set_start_port(33445);
        opts.set_end_port(65535);
        let tox = Tox::new(opts).unwrap();
        tox.set_name(name.as_bytes()).unwrap();
        let store = FsStore::new(dir.path().to_path_buf(), Arc::new(StdFileSystem)).unwrap();
        let bridge = ToxMerkleBridge::new(tox, store);
        Self { bridge, _dir: dir }
    }

    async fn iterate(&mut self) {
        let tox_mutex = {
            let node = self.bridge.node.lock().await;
            node.transport.tox.clone()
        };

        {
            let tox = tox_mutex.lock();
            let events = tox.events().unwrap();

            for event in &events {
                self.bridge.handle_event(&event).await;
            }
        }

        self.bridge.poll().await;
    }
}

#[tokio::test]
async fn test_bridge_connection_mapping() {
    init_tracing();
    let mut alice = TestNode::new("Alice");
    let mut bob = TestNode::new("Bob");

    let bob_addr = bob.bridge.node.lock().await.transport.tox.lock().address();
    let bob_dht_id = bob.bridge.node.lock().await.transport.tox.lock().dht_id();
    let bob_port = bob
        .bridge
        .node
        .lock()
        .await
        .transport
        .tox
        .lock()
        .udp_port()
        .unwrap();

    let bob_pk = {
        let node = bob.bridge.node.lock().await;
        node.transport.local_pk()
    };

    let f_number = {
        let node = alice.bridge.node.lock().await;
        let tox = node.transport.tox.lock();
        tox.bootstrap("127.0.0.1", bob_port, &bob_dht_id).unwrap();
        tox.friend_add(&bob_addr, b"Hello").unwrap().get_number()
    };

    {
        let node = bob.bridge.node.lock().await;
        let tox = node.transport.tox.lock();
        let alice_pk = alice.bridge.node.lock().await.transport.local_pk();
        tox.friend_add_norequest(&toxcore::types::PublicKey(*alice_pk.as_bytes()))
            .unwrap();
    }

    // 1. Wait for connection
    let start = Instant::now();
    loop {
        alice.iterate().await;
        bob.iterate().await;

        let status = {
            let node = alice.bridge.node.lock().await;
            let tox = node.transport.tox.lock();
            tox.friend(f_number).connection_status().unwrap()
        };

        if status != ToxConnection::TOX_CONNECTION_NONE {
            break;
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
        if start.elapsed() > Duration::from_secs(15) {
            panic!("Timed out waiting for connection");
        }
    }

    // 2. Verify Alice sees Bob as available in Merkle-Tox
    {
        let node = alice.bridge.node.lock().await;
        assert!(node.sessions.contains_key(&bob_pk));
    }

    // 3. Bob goes offline (drop him)
    drop(bob);

    // 4. Alice should detect disconnection and update Merkle-Tox
    let start = Instant::now();
    loop {
        alice.iterate().await;

        let status = {
            let node = alice.bridge.node.lock().await;
            let tox = node.transport.tox.lock();
            tox.friend(f_number).connection_status().unwrap()
        };

        if status == ToxConnection::TOX_CONNECTION_NONE {
            let node = alice.bridge.node.lock().await;
            if !node.sessions.contains_key(&bob_pk) {
                break;
            }
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
        if start.elapsed() > Duration::from_secs(15) {
            panic!("Timed out waiting for disconnection detection");
        }
    }

    // 5. Final verification
    {
        let node = alice.bridge.node.lock().await;
        assert!(
            !node.sessions.contains_key(&bob_pk),
            "Session should be removed after disconnection"
        );
    }
}
