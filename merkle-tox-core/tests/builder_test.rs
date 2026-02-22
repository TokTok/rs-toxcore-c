use merkle_tox_core::builder::NodeBuilder;
use merkle_tox_core::crypto::ConversationKeys;
use merkle_tox_core::dag::{
    Content, ControlAction, Ed25519Signature, KConv, LogicalIdentityPk, NodeAuth,
};

#[test]
fn test_group_genesis_pow() {
    let sk = ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]);
    let creator_pk = LogicalIdentityPk::from(sk.verifying_key().to_bytes());
    let node =
        NodeBuilder::new_group_genesis("Test Room".to_string(), creator_pk, 0, 123456789, &sk);

    assert!(node.validate_pow());
    if let Content::Control(ControlAction::Genesis { title, .. }) = node.content {
        assert_eq!(title, "Test Room");
    } else {
        panic!("Wrong content type");
    }
}

#[test]
fn test_1on1_genesis() {
    let pk_a = LogicalIdentityPk::from([1u8; 32]);
    let pk_b = LogicalIdentityPk::from([2u8; 32]);
    let keys = ConversationKeys::derive(&KConv::from([0u8; 32]));

    let node1 = NodeBuilder::new_1on1_genesis(pk_a, pk_b, &keys);
    let node2 = NodeBuilder::new_1on1_genesis(pk_b, pk_a, &keys);

    assert_eq!(
        node1.hash(),
        node2.hash(),
        "Genesis node should be deterministic and commutative"
    );
    assert_eq!(node1.parents.len(), 0);

    // EphemeralSignature check (1-on-1 genesis uses MAC-derived pseudo-sig in first 32 bytes)
    if let NodeAuth::EphemeralSignature(sig) = node1.authentication {
        assert_ne!(sig, Ed25519Signature::from([0u8; 64]));
    } else {
        panic!("Should have EphemeralSignature");
    }
}

// end of file
