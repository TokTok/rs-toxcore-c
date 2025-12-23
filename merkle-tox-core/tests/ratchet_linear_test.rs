use merkle_tox_core::clock::ManualTimeProvider;
use merkle_tox_core::dag::{Content, PhysicalDeviceSk};
use merkle_tox_core::engine::{Conversation, Effect, MerkleToxEngine};
use merkle_tox_core::sync::NodeStore;
use merkle_tox_core::testing::{InMemoryStore, TestRoom};
use rand::SeedableRng;
use std::sync::Arc;
use std::time::Instant;

#[test]
fn test_epoch_prefixed_sequence_numbers() {
    let tp = Arc::new(ManualTimeProvider::new(Instant::now(), 1000));
    let store = InMemoryStore::new();
    let room = TestRoom::new(1);
    let alice = &room.identities[0];

    let mut engine = MerkleToxEngine::with_sk(
        alice.device_pk,
        alice.master_pk,
        PhysicalDeviceSk::from(alice.device_sk.to_bytes()),
        rand::rngs::StdRng::seed_from_u64(42),
        tp.clone(),
    );
    room.setup_engine(&mut engine, &store);
    let conv_id = room.conv_id;

    // 1. Author node in Epoch 0
    let effects = engine
        .author_node(conv_id, Content::Text("Msg 1".into()), vec![], &store)
        .unwrap();
    let node1 = effects
        .iter()
        .find_map(|e| {
            if let Effect::WriteStore(_, n, _) = e {
                Some(n.clone())
            } else {
                None
            }
        })
        .unwrap();
    // Seq should be (0 << 32) | 1 = 1
    assert_eq!(node1.sequence_number, 1);
    merkle_tox_core::testing::apply_effects(effects, &store);

    // 2. Rotate to Epoch 1
    let effects = engine.rotate_conversation_key(conv_id, &store).unwrap();
    merkle_tox_core::testing::apply_effects(effects, &store);

    // 3. Author node in Epoch 1
    let effects = engine
        .author_node(conv_id, Content::Text("Msg 2".into()), vec![], &store)
        .unwrap();
    let node2 = effects
        .iter()
        .find_map(|e| {
            if let Effect::WriteStore(_, n, _) = e {
                Some(n.clone())
            } else {
                None
            }
        })
        .unwrap();
    // Seq should be (1 << 32) | 1
    assert_eq!(node2.sequence_number, (1u64 << 32) | 1);
    merkle_tox_core::testing::apply_effects(effects, &store);

    // 4. Author another node in Epoch 1
    let effects = engine
        .author_node(conv_id, Content::Text("Msg 3".into()), vec![], &store)
        .unwrap();
    let node3 = effects
        .iter()
        .find_map(|e| {
            if let Effect::WriteStore(_, n, _) = e {
                Some(n.clone())
            } else {
                None
            }
        })
        .unwrap();
    // Seq should be (1 << 32) | 2
    assert_eq!(node3.sequence_number, (1u64 << 32) | 2);
}

#[test]
fn test_per_sender_ratchet_isolation() {
    let tp = Arc::new(ManualTimeProvider::new(Instant::now(), 1000));
    let alice_store = InMemoryStore::new();
    let bob_store = InMemoryStore::new();
    let room = TestRoom::new(2); // Alice and Bob
    let alice = &room.identities[0];
    let bob = &room.identities[1];
    let conv_id = room.conv_id;

    let mut alice_engine = MerkleToxEngine::with_sk(
        alice.device_pk,
        alice.master_pk,
        PhysicalDeviceSk::from(alice.device_sk.to_bytes()),
        rand::rngs::StdRng::seed_from_u64(42),
        tp.clone(),
    );
    room.setup_engine(&mut alice_engine, &alice_store);

    let mut bob_engine = MerkleToxEngine::with_sk(
        bob.device_pk,
        bob.master_pk,
        PhysicalDeviceSk::from(bob.device_sk.to_bytes()),
        rand::rngs::StdRng::seed_from_u64(43),
        tp.clone(),
    );
    room.setup_engine(&mut bob_engine, &bob_store);

    // 1. Alice authors Msg A1
    let effects = alice_engine
        .author_node(conv_id, Content::Text("A1".into()), vec![], &alice_store)
        .unwrap();
    let node_a1 = effects
        .iter()
        .find_map(|e| {
            if let Effect::WriteStore(_, n, _) = e {
                Some(n.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(node_a1.sequence_number, 1);
    merkle_tox_core::testing::apply_effects(effects, &alice_store);

    // 2. Bob authors Msg B1
    let effects = bob_engine
        .author_node(conv_id, Content::Text("B1".into()), vec![], &bob_store)
        .unwrap();
    let node_b1 = effects
        .iter()
        .find_map(|e| {
            if let Effect::WriteStore(_, n, _) = e {
                Some(n.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(node_b1.sequence_number, 1); // Bob's first node
    merkle_tox_core::testing::apply_effects(effects, &bob_store);

    // 3. Alice receives Bob's node
    let effects = alice_engine
        .handle_node(conv_id, node_b1.clone(), &alice_store, None)
        .unwrap();
    merkle_tox_core::testing::apply_effects(effects, &alice_store);

    // 4. Verify Alice's internal state tracks Bob's ratchet
    let em = match alice_engine.conversations.get(&conv_id).unwrap() {
        Conversation::Established(em) => em,
        _ => panic!("Expected established conversation"),
    };

    assert!(em.state.sender_ratchets.contains_key(&alice.device_pk));
    assert!(em.state.sender_ratchets.contains_key(&bob.device_pk));

    let alice_state = em.state.sender_ratchets.get(&alice.device_pk).unwrap();
    let bob_state = em.state.sender_ratchets.get(&bob.device_pk).unwrap();

    assert_eq!(alice_state.0, 1); // last_seq
    assert_eq!(bob_state.0, 1); // last_seq
}

#[test]
fn test_out_of_order_reverification() {
    let tp = Arc::new(ManualTimeProvider::new(Instant::now(), 1000));
    let alice_store = InMemoryStore::new();
    let bob_store = InMemoryStore::new();
    let room = TestRoom::new(2);
    let alice = &room.identities[0];
    let bob = &room.identities[1];
    let conv_id = room.conv_id;

    let mut alice_engine = MerkleToxEngine::with_sk(
        alice.device_pk,
        alice.master_pk,
        PhysicalDeviceSk::from(alice.device_sk.to_bytes()),
        rand::rngs::StdRng::seed_from_u64(42),
        tp.clone(),
    );
    room.setup_engine(&mut alice_engine, &alice_store);

    let mut bob_engine = MerkleToxEngine::with_sk(
        bob.device_pk,
        bob.master_pk,
        PhysicalDeviceSk::from(bob.device_sk.to_bytes()),
        rand::rngs::StdRng::seed_from_u64(43),
        tp.clone(),
    );
    room.setup_engine(&mut bob_engine, &bob_store);

    // Bob authors B1 and B2
    let effects = bob_engine
        .author_node(conv_id, Content::Text("B1".into()), vec![], &bob_store)
        .unwrap();
    let node_b1 = effects
        .iter()
        .find_map(|e| {
            if let Effect::WriteStore(_, n, _) = e {
                Some(n.clone())
            } else {
                None
            }
        })
        .unwrap()
        .clone();
    merkle_tox_core::testing::apply_effects(effects, &bob_store);

    let effects = bob_engine
        .author_node(conv_id, Content::Text("B2".into()), vec![], &bob_store)
        .unwrap();
    let node_b2 = effects
        .iter()
        .find_map(|e| {
            if let Effect::WriteStore(_, n, _) = e {
                Some(n.clone())
            } else {
                None
            }
        })
        .unwrap()
        .clone();
    merkle_tox_core::testing::apply_effects(effects, &bob_store);

    // Alice receives B2 FIRST (out of order sequence)
    let effects = alice_engine
        .handle_node(conv_id, node_b2.clone(), &alice_store, None)
        .unwrap();
    // B2 should be speculative because B1 is missing (cannot derive key)
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::EmitEvent(merkle_tox_core::NodeEvent::NodeSpeculative { .. })
    )));
    merkle_tox_core::testing::apply_effects(effects, &alice_store);
    assert!(!alice_store.is_verified(&node_b2.hash()));

    // Alice receives B1
    let effects = alice_engine
        .handle_node(conv_id, node_b1.clone(), &alice_store, None)
        .unwrap();
    // B1 should be verified
    merkle_tox_core::testing::apply_effects(effects, &alice_store);
    assert!(alice_store.is_verified(&node_b1.hash()));

    // B2 should now be automatically verified via reverify_speculative_for_conversation
    assert!(alice_store.is_verified(&node_b2.hash()));
}
