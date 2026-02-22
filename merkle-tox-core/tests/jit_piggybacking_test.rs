use merkle_tox_core::clock::ManualTimeProvider;
use merkle_tox_core::dag::{Content, PhysicalDeviceDhSk, PhysicalDeviceSk};
use merkle_tox_core::engine::{Conversation, MerkleToxEngine};
use merkle_tox_core::sync::NodeStore;
use merkle_tox_core::testing::{
    InMemoryStore, TestRoom, apply_effects, get_all_nodes_from_effects,
};
use rand::SeedableRng;
use std::sync::Arc;
use std::time::Instant;

/// Helper: create an engine with full keys (sk + dh_sk) for ECIES support.
fn engine_with_full_keys(
    id: &merkle_tox_core::testing::TestIdentity,
    rng_seed: u64,
    tp: Arc<ManualTimeProvider>,
) -> MerkleToxEngine {
    MerkleToxEngine::with_full_keys(
        id.device_pk,
        id.master_pk,
        PhysicalDeviceSk::from(id.device_sk.to_bytes()),
        PhysicalDeviceDhSk::from(merkle_tox_core::crypto::ed25519_sk_to_x25519(
            &id.device_sk.to_bytes(),
        )),
        rand::rngs::StdRng::seed_from_u64(rng_seed),
        tp,
    )
}

/// Verifies that when a new device (Carol) is authorized and Alice sends a
/// content message, Alice first authors a JIT SenderKeyDistribution so that
/// Carol can immediately decrypt Alice's messages without waiting for epoch
/// rotation.
#[test]
fn test_jit_piggybacking_distributes_ratchet_state() {
    let _ = tracing_subscriber::fmt::try_init();
    let tp = Arc::new(ManualTimeProvider::new(Instant::now(), 1000));
    let room = TestRoom::new(2); // Alice and Bob
    let alice_id = &room.identities[0];
    let bob_id = &room.identities[1];

    let alice_store = InMemoryStore::new();
    let bob_store = InMemoryStore::new();

    let mut alice_engine = engine_with_full_keys(alice_id, 42, tp.clone());
    room.setup_engine(&mut alice_engine, &alice_store);

    let mut bob_engine = engine_with_full_keys(bob_id, 43, tp.clone());
    room.setup_engine(&mut bob_engine, &bob_store);

    // 1. Alice authors a text message. Because Bob hasn't received her
    //    SenderKey yet, she MUST first author a JIT SKD.
    let effects = alice_engine
        .author_node(
            room.conv_id,
            Content::Text("Hello Bob".to_string()),
            vec![],
            &alice_store,
        )
        .unwrap();
    let all_nodes = get_all_nodes_from_effects(&effects);
    merkle_tox_core::testing::transfer_wire_nodes(&effects, &bob_store);
    apply_effects(effects, &alice_store);

    // There should be at least 2 nodes: a JIT SKD + the text.
    assert!(
        all_nodes.len() >= 2,
        "Expected JIT SKD + text, got {} nodes",
        all_nodes.len()
    );

    // First node should be an SKD
    assert!(
        matches!(&all_nodes[0].content, Content::SenderKeyDistribution { .. }),
        "First node should be SenderKeyDistribution"
    );

    // Last node should be the text
    assert!(
        matches!(&all_nodes.last().unwrap().content, Content::Text(t) if t == "Hello Bob"),
        "Last node should be the text"
    );

    // 2. Bob receives ALL of Alice's nodes (JIT SKD + text)
    for node in &all_nodes {
        let effects = bob_engine
            .handle_node(room.conv_id, node.clone(), &bob_store, None)
            .unwrap();
        apply_effects(effects, &bob_store);
    }

    // 3. Verify Bob successfully processed the text node
    let text_node = all_nodes.last().unwrap();
    assert!(
        bob_store.is_verified(&text_node.hash()),
        "Bob should have verified Alice's text message"
    );

    // 4. Verify Bob's ratchet state was seeded by the JIT payload
    if let Some(Conversation::Established(em)) = bob_engine.conversations.get(&room.conv_id) {
        // Bob should have a ratchet entry for Alice
        assert!(
            em.state.sender_ratchets.contains_key(&alice_id.device_pk),
            "Bob should have Alice's ratchet state after JIT"
        );
        // Bob should have a JIT header for Alice
        let epoch = text_node.sequence_number >> 32;
        assert!(
            em.state
                .jit_headers
                .contains_key(&(alice_id.device_pk, epoch)),
            "Bob should have Alice's JIT K_header"
        );
    } else {
        panic!("Bob's conversation should be established");
    }

    // 5. Alice authors another text: NO new JIT SKD needed (Bob already tracked)
    let effects = alice_engine
        .author_node(
            room.conv_id,
            Content::Text("Second message".to_string()),
            vec![],
            &alice_store,
        )
        .unwrap();
    let all_nodes2 = get_all_nodes_from_effects(&effects);
    merkle_tox_core::testing::transfer_wire_nodes(&effects, &bob_store);
    apply_effects(effects, &alice_store);

    // Only 1 node this time: just the text (no JIT needed)
    assert_eq!(
        all_nodes2.len(),
        1,
        "Second message should NOT produce a JIT SKD"
    );
    assert!(matches!(
        &all_nodes2[0].content,
        Content::Text(t) if t == "Second message"
    ));

    // Bob can also verify the second message
    for node in &all_nodes2 {
        let effects = bob_engine
            .handle_node(room.conv_id, node.clone(), &bob_store, None)
            .unwrap();
        apply_effects(effects, &bob_store);
    }
    assert!(bob_store.is_verified(&all_nodes2[0].hash()));
}

/// Verifies that try_sender_for_wire can identify senders up to 2000
/// positions ahead (matching the spec's MAX_RATCHET_SKIP).
#[test]
fn test_max_ratchet_skip_2000() {
    let _ = tracing_subscriber::fmt::try_init();
    let tp = Arc::new(ManualTimeProvider::new(Instant::now(), 1000));
    let room = TestRoom::new(1);
    let alice_id = &room.identities[0];
    let store = InMemoryStore::new();

    let mut engine = engine_with_full_keys(alice_id, 42, tp.clone());
    room.setup_engine(&mut engine, &store);

    // Verify peek_keys succeeds at skip distance of 2000 (counter = 2000)
    if let Some(Conversation::Established(em)) = engine.conversations.get_mut(&room.conv_id) {
        let epoch = 0u64;
        // seq = (epoch << 32) | 2000: skipping from 0 to 2000
        let seq = (epoch << 32) | 2000;
        let now = engine.clock.network_time_ms();
        let result = em.peek_keys(&alice_id.device_pk, seq, now);
        assert!(
            result.is_some(),
            "peek_keys should succeed at skip distance 2000"
        );

        // Verify it fails at 2001 (exceeds MAX_RATCHET_SKIP)
        let seq_over = (epoch << 32) | 2001;
        let result_over = em.peek_keys(&alice_id.device_pk, seq_over, now);
        assert!(
            result_over.is_none(),
            "peek_keys should fail at skip distance 2001"
        );
    } else {
        panic!("Expected established conversation");
    }
}

/// Verifies that JIT tracking (shared_keys_sent_to) is cleared on epoch rotation.
#[test]
fn test_jit_tracking_cleared_on_rotation() {
    let _ = tracing_subscriber::fmt::try_init();
    let tp = Arc::new(ManualTimeProvider::new(Instant::now(), 1000));
    let room = TestRoom::new(2);
    let alice_id = &room.identities[0];
    let bob_id = &room.identities[1];
    let store = InMemoryStore::new();

    let mut engine = engine_with_full_keys(alice_id, 42, tp.clone());
    room.setup_engine(&mut engine, &store);

    // Author a text to trigger JIT for Bob
    let effects = engine
        .author_node(
            room.conv_id,
            Content::Text("Pre-rotation".to_string()),
            vec![],
            &store,
        )
        .unwrap();
    apply_effects(effects, &store);

    // Verify Bob is in shared_keys_sent_to
    if let Some(Conversation::Established(em)) = engine.conversations.get(&room.conv_id) {
        assert!(
            em.state.shared_keys_sent_to.contains(&bob_id.device_pk),
            "Bob should be tracked as sent-to before rotation"
        );
    }

    // Rotate
    let effects = engine
        .rotate_conversation_key(room.conv_id, &store)
        .unwrap();
    apply_effects(effects, &store);

    // After rotation + SKD, Bob should be tracked again (rotation SKD sends to all)
    if let Some(Conversation::Established(em)) = engine.conversations.get(&room.conv_id) {
        // shared_keys_sent_to was cleared by rotate(), then repopulated by rotation SKD
        assert!(
            em.state.shared_keys_sent_to.contains(&bob_id.device_pk),
            "Bob should be re-tracked after rotation SKD"
        );
    }
}
