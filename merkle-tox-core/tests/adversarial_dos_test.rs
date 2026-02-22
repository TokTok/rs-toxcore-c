use merkle_tox_core::clock::ManualTimeProvider;
use merkle_tox_core::crypto::ConversationKeys;
use merkle_tox_core::dag::{Content, ConversationId, KConv, LogicalIdentityPk, PhysicalDevicePk};
use merkle_tox_core::engine::MerkleToxEngine;
use merkle_tox_core::sync::NodeStore;
use merkle_tox_core::testing::{InMemoryStore, create_signed_content_node};
use rand::{SeedableRng, rngs::StdRng};
use std::sync::Arc;
use std::time::Instant;
use tox_proto::constants::MAX_SPECULATIVE_NODES_PER_CONVERSATION;
use tox_reconcile::{IbltSketch, Tier};

#[test]
fn test_speculative_node_flooding_limit() {
    let self_pk = PhysicalDevicePk::from([1u8; 32]);
    let tp = Arc::new(ManualTimeProvider::new(Instant::now(), 0));
    let mut engine =
        MerkleToxEngine::new(self_pk, self_pk.to_logical(), StdRng::seed_from_u64(0), tp);
    let store = InMemoryStore::new();
    let conv_id = ConversationId::from([0xAAu8; 32]);

    // Seed the store with MAX-5 dummy speculative nodes to avoid iterating 1000 times.
    let seed_count = MAX_SPECULATIVE_NODES_PER_CONVERSATION - 5;
    for i in 0..seed_count {
        let mut pk_bytes = [0u8; 32];
        pk_bytes[0..4].copy_from_slice(&(i as u32).to_le_bytes());
        let pk = PhysicalDevicePk::from(pk_bytes);
        let author_pk = LogicalIdentityPk::from(pk_bytes);
        let node = create_signed_content_node(
            &conv_id,
            &ConversationKeys::derive(&KConv::from([0u8; 32])),
            author_pk,
            pk,
            vec![],
            Content::Text(format!("Seed {}", i)),
            0,
            1,
            1000 + i as i64,
        );
        store
            .put_node(&conv_id, node, false)
            .expect("seed speculative node");
    }

    // Process the last 5 through handle_node to test real behavior near the boundary.
    for i in seed_count..MAX_SPECULATIVE_NODES_PER_CONVERSATION {
        let mut pk_bytes = [0u8; 32];
        pk_bytes[0..4].copy_from_slice(&(i as u32).to_le_bytes());
        let pk = PhysicalDevicePk::from(pk_bytes);
        let author_pk = LogicalIdentityPk::from(pk_bytes);

        let node = create_signed_content_node(
            &conv_id,
            &ConversationKeys::derive(&KConv::from([0u8; 32])),
            author_pk,
            pk,
            vec![],
            Content::Text(format!("Spam {}", i)),
            0,
            1,
            1000 + i as i64,
        );
        let effects = engine.handle_node(conv_id, node, &store, None).unwrap();
        merkle_tox_core::testing::apply_effects(effects, &store);
    }

    let (_, spec_count) = store.get_node_counts(&conv_id);
    assert_eq!(spec_count, MAX_SPECULATIVE_NODES_PER_CONVERSATION);

    // Next node should be rejected
    let extra_pk = PhysicalDevicePk::from([0xFFu8; 32]);
    let extra_author = LogicalIdentityPk::from([0xFFu8; 32]);
    let extra_node = create_signed_content_node(
        &conv_id,
        &ConversationKeys::derive(&KConv::from([0u8; 32])),
        extra_author,
        extra_pk,
        vec![],
        Content::Text("Extra Spam".to_string()),
        0,
        1,
        99999,
    );
    let res = engine.handle_node(conv_id, extra_node, &store, None);
    assert!(
        res.is_err(),
        "Speculative node past limit should be rejected"
    );
    assert!(
        matches!(
            res.unwrap_err(),
            merkle_tox_core::error::MerkleToxError::Validation(
                merkle_tox_core::dag::ValidationError::TooManySpeculativeNodes
            )
        ),
        "Expected TooManySpeculativeNodes error"
    );
}

#[test]
fn test_sequence_number_replay_attack() {
    let tp = Arc::new(ManualTimeProvider::new(Instant::now(), 0));
    let store = InMemoryStore::new();
    let room = merkle_tox_core::testing::TestRoom::new(2);
    let alice = &room.identities[0];
    let observer = &room.identities[1];

    let mut engine = MerkleToxEngine::new(
        observer.device_pk,
        observer.master_pk,
        StdRng::seed_from_u64(0),
        tp.clone(),
    );
    room.setup_engine(&mut engine, &store);

    // 1. Receive a valid message with seq 2
    let admin_heads = store.get_admin_heads(&room.conv_id);
    let msg1 = create_signed_content_node(
        &room.conv_id,
        &room.keys,
        alice.master_pk,
        alice.device_pk,
        admin_heads.clone(),
        Content::Text("Original Message".to_string()),
        2, // Rank 2
        2, // Seq 2
        1000,
    );
    let effects = engine
        .handle_node(room.conv_id, msg1.clone(), &store, None)
        .unwrap();
    assert!(merkle_tox_core::testing::is_verified_in_effects(&effects));
    merkle_tox_core::testing::apply_effects(effects, &store);

    // 2. Try to replay with a DIFFERENT node but same sequence number
    let msg1_replay = create_signed_content_node(
        &room.conv_id,
        &room.keys,
        alice.master_pk,
        alice.device_pk,
        admin_heads,
        Content::Text("Replayed Sequence Number".to_string()),
        2, // Rank 2
        2, // Seq 2
        1001,
    );
    let res = engine.handle_node(room.conv_id, msg1_replay, &store, None);
    // V1 Ratchet rule: A client MUST NOT accept a message with a sequence_number
    // lower than or EQUAL TO the current known ratchet index for that sender.
    assert!(
        res.is_err(),
        "Message with duplicate sequence number should be rejected"
    );
}

#[test]
fn test_authorized_node_flooding_limit() {
    let tp = Arc::new(ManualTimeProvider::new(Instant::now(), 0));
    let store = InMemoryStore::new();
    let room = merkle_tox_core::testing::TestRoom::new(2);
    let alice = &room.identities[0];
    let observer = &room.identities[1];

    let mut engine = MerkleToxEngine::new(
        observer.device_pk,
        observer.master_pk,
        StdRng::seed_from_u64(0),
        tp.clone(),
    );
    room.setup_engine(&mut engine, &store);

    let max = tox_proto::constants::MAX_VERIFIED_NODES_PER_DEVICE;

    // Seed the store's last_sequence_number near the limit so we only need
    // a few handle_node iterations instead of 1000.
    // Sequence numbers and topological ranks are independent: ranks must
    // be max(parent_rank)+1, but sequence numbers can start anywhere.
    let seq_start = max - 5;
    store
        .last_sequence_numbers
        .write()
        .unwrap()
        .insert(alice.device_pk, seq_start);

    // Process 5 nodes near the sequence boundary through handle_node.
    let mut parents = store.get_admin_heads(&room.conv_id);
    for (idx, seq) in (seq_start..max).enumerate() {
        let rank = idx as u64 + 2; // rank starts at 2 (parent admin nodes have rank 1)
        let node = create_signed_content_node(
            &room.conv_id,
            &room.keys,
            alice.master_pk,
            alice.device_pk,
            parents,
            Content::Text(format!("Authorized Flood {}", seq)),
            rank,
            seq + 1,
            1000 + seq as i64,
        );
        parents = vec![node.hash()];
        let effects = engine
            .handle_node(room.conv_id, node, &store, None)
            .unwrap();
        assert!(
            merkle_tox_core::testing::is_verified_in_effects(&effects),
            "Node seq={} should be verified",
            seq + 1
        );
        merkle_tox_core::testing::apply_effects(effects, &store);
        engine.clear_pending();
    }

    // Next node should be rejected
    let extra_node = create_signed_content_node(
        &room.conv_id,
        &room.keys,
        alice.master_pk,
        alice.device_pk,
        parents.clone(),
        Content::Text("One too many".to_string()),
        7, // rank continues
        max + 1,
        999999,
    );
    let res = engine.handle_node(room.conv_id, extra_node, &store, None);
    assert!(res.is_err());
    assert!(matches!(
        res.unwrap_err(),
        merkle_tox_core::error::MerkleToxError::Validation(
            merkle_tox_core::dag::ValidationError::TooManyVerifiedNodes
        )
    ));
}

#[test]
fn test_iblt_peeling_torture() {
    // Create a large sketch that is mathematically designed to be non-decodable.
    // We can do this by inserting many elements into a small tier, or by manually
    // corrupting cells so they look pure but fail checksum.

    let mut sketch = IbltSketch::new(Tier::Large.cell_count());

    // Fill every cell with count 2. No cell will ever be pure.
    for cell in &mut sketch.cells {
        cell.count = 2;
        cell.id_sum = [0xAAu8; 32];
        cell.hash_sum = 12345;
    }

    let res = sketch.decode();

    assert!(res.is_err(), "Corrupted sketch should fail decoding");

    if let Err(tox_reconcile::iblt::ReconciliationError::DecodingFailed(stats)) = res {
        // We expect:
        // 1. Initial scan: Tier::Large.cell_count() iterations (1024)
        // 2. Peeling: 0 iterations (no pure cells)
        // 3. Final check: scans until it finds the first non-empty cell (iteration 1, so total 1025)
        assert_eq!(stats.cells_peeled, 0);
        assert_eq!(stats.iterations, 1025);
    } else {
        panic!("Expected DecodingFailed error");
    }
}

#[test]
fn test_dos_experimental() {
    let tp = Arc::new(ManualTimeProvider::new(Instant::now(), 0));
    let store = InMemoryStore::new();
    let room = merkle_tox_core::testing::TestRoom::new(2);
    let alice = &room.identities[0];
    let observer = &room.identities[1];

    let mut engine = MerkleToxEngine::new(
        observer.device_pk,
        observer.master_pk,
        StdRng::seed_from_u64(0),
        tp.clone(),
    );
    room.setup_engine(&mut engine, &store);

    let mut parents = store.get_admin_heads(&room.conv_id);
    for i in 1..200 {
        let start_create = std::time::Instant::now();
        let node = create_signed_content_node(
            &room.conv_id,
            &room.keys,
            alice.master_pk,
            alice.device_pk,
            parents,
            Content::Text(format!("Authorized Flood {}", i)),
            i + 1, // Rank starts at 2
            i + 1, // Start from 2
            1000 + i as i64,
        );
        parents = vec![node.hash()];
        let start_handle = std::time::Instant::now();
        let effects = engine
            .handle_node_internal_ext(room.conv_id, node, &store, None, false)
            .unwrap();
        let start_apply = std::time::Instant::now();
        merkle_tox_core::testing::apply_effects(effects, &store);
        let end = std::time::Instant::now();
        if i % 20 == 0 {
            println!(
                "i: {}, create: {:?}, handle: {:?}, apply: {:?}",
                i,
                start_handle.duration_since(start_create),
                start_apply.duration_since(start_handle),
                end.duration_since(start_apply)
            );
        }
    }
}
