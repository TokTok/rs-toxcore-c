use merkle_tox_core::ProtocolMessage;
use merkle_tox_core::clock::ManualTimeProvider;
use merkle_tox_core::dag::{ConversationId, KConv, PhysicalDevicePk};
use merkle_tox_core::engine::MerkleToxEngine;
use merkle_tox_core::node::MerkleToxNode;
use merkle_tox_core::sync::NodeStore;
use merkle_tox_core::testing::{InMemoryStore, SimulatedTransport, VirtualHub};
use rand::{SeedableRng, rngs::StdRng};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[test]
fn test_peer_availability_silences_traffic() {
    let _ = tracing_subscriber::fmt::try_init();
    let time_provider = Arc::new(ManualTimeProvider::new(Instant::now(), 1000));
    let hub = Arc::new(VirtualHub::new(time_provider.clone()));

    let alice_pk = PhysicalDevicePk::from([1u8; 32]);
    let _alice_rx = hub.register(alice_pk);
    let alice_transport = SimulatedTransport::new(alice_pk, hub.clone());
    let alice_store = InMemoryStore::new();
    let alice_node = MerkleToxNode::new(
        MerkleToxEngine::new(
            alice_pk,
            alice_pk.to_logical(),
            StdRng::seed_from_u64(1),
            time_provider.clone(),
        ),
        alice_transport,
        alice_store,
        time_provider.clone(),
    );
    let alice_node = Arc::new(Mutex::new(alice_node));

    let bob_pk = PhysicalDevicePk::from([2u8; 32]);
    let bob_rx = hub.register(bob_pk);

    let conv_id = ConversationId::from([0x42u8; 32]);
    let k_conv = KConv::from([0xAAu8; 32]);

    // Setup Alice with a conversation and Bob as a sync peer
    {
        let mut a = alice_node.lock().unwrap();
        a.store.put_conversation_key(&conv_id, 0, k_conv).unwrap();

        let effects = {
            let MerkleToxNode { engine, store, .. } = &mut *a;
            engine.load_conversation_state(conv_id, store).unwrap();
            engine.start_sync(conv_id, Some(bob_pk), store)
        };

        let now = a.time_provider.now_instant();
        let now_ms = a.time_provider.now_system_ms() as u64;
        let mut dummy = now;
        for effect in effects {
            a.process_effect(effect, now, now_ms, &mut dummy).unwrap();
        }
    }

    // Alice should initially want to send packets (pings or handshakes)
    {
        let mut a = alice_node.lock().unwrap();
        a.poll();
        assert!(
            bob_rx.try_recv().is_ok(),
            "Alice should send initial packets to Bob"
        );
    }

    // Now mark Bob as offline
    {
        let mut a = alice_node.lock().unwrap();
        a.set_peer_available(bob_pk, false);
    }

    // Flush any remaining packets in hub
    hub.poll();
    while bob_rx.try_recv().is_ok() {}

    // Alice polls again. Should NOT send anything to Bob.
    {
        let mut a = alice_node.lock().unwrap();
        time_provider.advance(Duration::from_secs(120)); // Advance past ping intervals
        a.poll();
        assert!(
            bob_rx.try_recv().is_err(),
            "Alice should NOT send packets to an offline peer"
        );
    }

    // Now mark Bob as online again
    {
        let mut a = alice_node.lock().unwrap();
        a.set_peer_available(bob_pk, true);
        // Bridge would also send CapsAnnounce
        a.send_message(
            bob_pk,
            ProtocolMessage::CapsAnnounce {
                version: 1,
                features: 0,
            },
        );
    }

    // Alice polls again. Should resume sending
    {
        let mut a = alice_node.lock().unwrap();
        a.poll();
        assert!(
            bob_rx.try_recv().is_ok(),
            "Alice should resume sending packets when peer is online"
        );
    }
}
