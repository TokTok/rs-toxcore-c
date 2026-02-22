use merkle_tox_core::clock::ManualTimeProvider;
use merkle_tox_core::dag::{Content, Permissions, PhysicalDeviceDhSk, PhysicalDeviceSk};
use merkle_tox_core::engine::MerkleToxEngine;
use merkle_tox_core::sync::NodeStore;
use merkle_tox_core::testing::TestRoom;
use merkle_tox_core::testing::store::InMemoryStore;
use rand::SeedableRng;
use std::sync::Arc;

use std::time::Instant;

#[test]
fn test_ratchet_state_lost_on_restart() {
    let _ = tracing_subscriber::fmt::try_init();
    // 1. Setup Environment
    let store = Arc::new(InMemoryStore::new());
    let room = TestRoom::new(2); // Alice and Bob
    let alice_id = &room.identities[0];
    let bob_id = &room.identities[1];

    let time = Arc::new(ManualTimeProvider::new(Instant::now(), 1000));
    let rng = rand::rngs::StdRng::seed_from_u64(42);

    // 2. Initialize Alice (Online)
    let mut alice_engine = MerkleToxEngine::with_full_keys(
        alice_id.device_pk,
        alice_id.master_pk,
        PhysicalDeviceSk::from(alice_id.device_sk.to_bytes()),
        PhysicalDeviceDhSk::from(merkle_tox_core::crypto::ed25519_sk_to_x25519(
            &alice_id.device_sk.to_bytes(),
        )),
        rng.clone(),
        time.clone(),
    );
    room.setup_engine(&mut alice_engine, &*store);

    // 3. Alice authors Node 1. This advances her internal ratchet state.
    // JIT piggybacking may produce an SKD before the text node.
    let effects = alice_engine
        .author_node(
            room.conv_id,
            Content::Text("Message 1".to_string()),
            vec![],
            &*store,
        )
        .expect("Failed to author node 1");
    let all_nodes1 = merkle_tox_core::testing::get_all_nodes_from_effects(&effects);
    merkle_tox_core::testing::apply_effects(effects, &*store);

    // 4. Initialize Bob (Receiver)
    let mut bob_engine = MerkleToxEngine::with_full_keys(
        bob_id.device_pk,
        bob_id.master_pk,
        PhysicalDeviceSk::from(bob_id.device_sk.to_bytes()),
        PhysicalDeviceDhSk::from(merkle_tox_core::crypto::ed25519_sk_to_x25519(
            &bob_id.device_sk.to_bytes(),
        )),
        rng.clone(),
        time.clone(),
    );
    let bob_store = Arc::new(InMemoryStore::new());
    room.setup_engine(&mut bob_engine, &*bob_store);

    // Transfer wire nodes from Alice's store to Bob's store (for encrypt-then-sign verification)
    for (hash, (cid, wire)) in store.wire_nodes.read().unwrap().iter() {
        let _ = bob_store.put_wire_node(cid, hash, wire.clone());
    }

    // Bob receives ALL of Alice's nodes (JIT SKD + text) and updates ratchet state
    for node in &all_nodes1 {
        let effects = bob_engine
            .handle_node(room.conv_id, node.clone(), &*bob_store, Some(&*bob_store))
            .expect("Bob failed to handle node");
        merkle_tox_core::testing::apply_effects(effects, &*bob_store);
    }

    // 5. Simulate Alice Restart (The "Reload")
    // We create a NEW engine instance for Alice.
    // Crucially, we pass the SAME store, simulating persistence.
    let mut alice_restarted = MerkleToxEngine::with_full_keys(
        alice_id.device_pk,
        alice_id.master_pk,
        PhysicalDeviceSk::from(alice_id.device_sk.to_bytes()),
        PhysicalDeviceDhSk::from(merkle_tox_core::crypto::ed25519_sk_to_x25519(
            &alice_id.device_sk.to_bytes(),
        )),
        rand::rngs::StdRng::seed_from_u64(999), // New session RNG
        time.clone(),
    );

    // Restore IdentityManager state manually. In production, this would
    // be rebuilt from persistent storage or a projection.
    for id in &room.identities {
        alice_restarted
            .identity_manager
            .add_member(room.conv_id, id.master_pk, 1, 0);
        id.authorize_in_engine(
            &mut alice_restarted,
            room.conv_id,
            Permissions::ALL,
            i64::MAX,
        );
    }

    // THIS IS THE TEST: Loading state from the store.
    // It *should* load the ratchet state (head_chains) so Alice knows where she left off.
    alice_restarted
        .load_conversation_state(room.conv_id, &*store)
        .expect("Failed to load state");

    // 6. Alice (Restarted) authors Node 2.
    // If she recovered her state, she will use the correct chain key derived from Node 1.
    // If she lost her state, she will default to the Epoch Root key.
    // JIT piggybacking may produce an SKD (restarted Alice has empty shared_keys_sent_to).
    let effects = alice_restarted
        .author_node(
            room.conv_id,
            Content::Text("Message 2".to_string()),
            vec![],
            &*store,
        )
        .expect("Failed to author node 2");
    let all_nodes2 = merkle_tox_core::testing::get_all_nodes_from_effects(&effects);
    merkle_tox_core::testing::transfer_wire_nodes(&effects, &*bob_store);
    merkle_tox_core::testing::apply_effects(effects, &*store);

    // 7. Bob attempts to verify all of Alice's authored nodes.
    // Bob has the correct state (he tracked Node 1).
    // If Alice used the wrong key, Bob will reject it.
    let mut any_verified = false;
    for node in &all_nodes2 {
        let effects = bob_engine
            .handle_node(room.conv_id, node.clone(), &*bob_store, Some(&*bob_store))
            .expect("Bob failed to handle node");
        if merkle_tox_core::testing::is_verified_in_effects(&effects) {
            any_verified = true;
        }
        merkle_tox_core::testing::apply_effects(effects, &*bob_store);
    }

    assert!(
        any_verified,
        "Bob should have verified the message from restarted Alice, proving that ratchet state was successfully persisted and restored."
    );
}
