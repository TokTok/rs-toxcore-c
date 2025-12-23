use crate::crypto::ConversationKeys;
use crate::dag::{
    ChainKey, ConversationId, KConv, MerkleNode, MessageKey, NodeAuth, NodeHash, PhysicalDevicePk,
    WireNode,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub struct Pending {
    pub speculative_nodes: HashSet<NodeHash>,
    pub vouchers: HashMap<NodeHash, HashSet<PhysicalDevicePk>>,
}

#[derive(Clone)]
pub struct Established {
    pub epochs: HashMap<u64, ConversationKeys>,
    pub sender_ratchets: HashMap<PhysicalDevicePk, (u64, ChainKey, Option<NodeHash>, u64)>, // (last_seq, next_chain_key, last_node_hash, epoch_id)
    pub current_epoch: u64,
    pub message_count: u32,
    pub last_rotation_time_ms: i64,
    pub vouchers: HashMap<NodeHash, HashSet<PhysicalDevicePk>>,
}

#[derive(Clone)]
pub struct ConversationData<S> {
    pub id: ConversationId,
    pub state: S,
}

#[derive(Clone)]
pub enum Conversation {
    Pending(ConversationData<Pending>),
    Established(ConversationData<Established>),
}

impl Conversation {
    pub fn id(&self) -> ConversationId {
        match self {
            Conversation::Pending(c) => c.id,
            Conversation::Established(c) => c.id,
        }
    }

    pub fn is_established(&self) -> bool {
        matches!(self, Conversation::Established(_))
    }

    pub fn vouchers(&self) -> &HashMap<NodeHash, HashSet<PhysicalDevicePk>> {
        match self {
            Conversation::Pending(c) => &c.state.vouchers,
            Conversation::Established(c) => &c.state.vouchers,
        }
    }

    pub fn vouchers_mut(&mut self) -> &mut HashMap<NodeHash, HashSet<PhysicalDevicePk>> {
        match self {
            Conversation::Pending(c) => &mut c.state.vouchers,
            Conversation::Established(c) => &mut c.state.vouchers,
        }
    }
}

impl ConversationData<Pending> {
    pub fn new(id: ConversationId) -> Self {
        Self {
            id,
            state: Pending {
                speculative_nodes: HashSet::new(),
                vouchers: HashMap::new(),
            },
        }
    }

    pub fn establish(
        self,
        initial_k_conv: KConv,
        now_ms: i64,
        epoch: u64,
    ) -> ConversationData<Established> {
        let mut epochs = HashMap::new();
        epochs.insert(epoch, ConversationKeys::derive(&initial_k_conv));
        ConversationData {
            id: self.id,
            state: Established {
                epochs,
                sender_ratchets: HashMap::new(),
                current_epoch: epoch,
                message_count: 0,
                last_rotation_time_ms: now_ms,
                vouchers: self.state.vouchers,
            },
        }
    }

    pub fn record_speculative(&mut self, hash: NodeHash) {
        self.state.speculative_nodes.insert(hash);
    }
}

impl ConversationData<Established> {
    pub fn new(id: ConversationId, initial_k_conv: KConv, now_ms: i64) -> Self {
        let mut epochs = HashMap::new();
        epochs.insert(0, ConversationKeys::derive(&initial_k_conv));
        Self {
            id,
            state: Established {
                epochs,
                sender_ratchets: HashMap::new(),
                current_epoch: 0,
                message_count: 0,
                last_rotation_time_ms: now_ms,
                vouchers: HashMap::new(),
            },
        }
    }

    pub fn current_epoch(&self) -> u64 {
        self.state.current_epoch
    }

    pub fn get_keys(&self, epoch: u64) -> Option<&ConversationKeys> {
        self.state.epochs.get(&epoch)
    }

    pub fn rotate(&mut self, new_k_conv: KConv, now_ms: i64) -> u64 {
        self.state.current_epoch += 1;
        self.state.epochs.insert(
            self.state.current_epoch,
            ConversationKeys::derive(&new_k_conv),
        );
        self.state.sender_ratchets.clear(); // Safe because seq resets to 1 in new epoch
        self.state.message_count = 0;
        self.state.last_rotation_time_ms = now_ms;
        self.state.current_epoch
    }

    pub fn add_epoch(&mut self, epoch: u64, k_conv: KConv) {
        self.state
            .epochs
            .insert(epoch, ConversationKeys::derive(&k_conv));
        if epoch > self.state.current_epoch {
            self.state.current_epoch = epoch;
            self.state.sender_ratchets.clear();
            self.state.message_count = 0;
        }
    }

    pub fn peek_keys(
        &self,
        sender_pk: &PhysicalDevicePk,
        seq: u64,
    ) -> Option<(MessageKey, ChainKey)> {
        let epoch = seq >> 32;
        let counter = seq & 0xFFFFFFFF;

        if let Some(&(last_seq, ref next_key, _, last_epoch)) =
            self.state.sender_ratchets.get(sender_pk)
            && seq == last_seq + 1
            && last_epoch == epoch
        {
            let k_msg = crate::crypto::ratchet_message_key(next_key);
            let k_next = crate::crypto::ratchet_step(next_key);
            return Some((k_msg, k_next));
        }

        // Re-initialize from the node's epoch root and step counter-1 times.
        let keys = self.get_keys(epoch)?;
        let mut chain_key = crate::crypto::ratchet_init_sender(&keys.k_conv, sender_pk);

        for _ in 1..counter {
            chain_key = crate::crypto::ratchet_step(&chain_key);
        }

        let k_msg = crate::crypto::ratchet_message_key(&chain_key);
        let k_next = crate::crypto::ratchet_step(&chain_key);

        Some((k_msg, k_next))
    }

    pub fn commit_node_key(
        &mut self,
        sender_pk: PhysicalDevicePk,
        seq: u64,
        next_chain_key: ChainKey,
        node_hash: NodeHash,
        epoch_id: u64,
    ) -> Option<NodeHash> {
        self.state
            .sender_ratchets
            .insert(sender_pk, (seq, next_chain_key, Some(node_hash), epoch_id))
            .and_then(|(_, _, h, _)| h)
    }

    pub fn get_sender_last_seq(&self, sender_pk: &PhysicalDevicePk) -> u64 {
        self.state
            .sender_ratchets
            .get(sender_pk)
            .and_then(|&(seq, _, _, epoch)| {
                if epoch == self.state.current_epoch {
                    Some(seq)
                } else {
                    None
                }
            })
            .unwrap_or(0)
    }

    pub fn verify_node_mac(&self, conversation_id: &ConversationId, node: &MerkleNode) -> bool {
        let mac = match &node.authentication {
            NodeAuth::Mac(m) => m,
            _ => return true, // Signatures are "authentic" for this layer
        };

        if let Some((k_msg, _)) = self.peek_keys(&node.sender_pk, node.sequence_number) {
            let keys = ConversationKeys::derive(&KConv::from(*k_msg.as_bytes()));
            if keys.verify_mac(&node.serialize_for_auth(conversation_id), mac) {
                return true;
            }
        }

        let auth_data = node.serialize_for_auth(conversation_id);
        let mut epochs: Vec<_> = self.state.epochs.keys().copied().collect();
        epochs.sort_unstable_by(|a, b| b.cmp(a));

        for epoch in epochs {
            if let Some(keys) = self.state.epochs.get(&epoch) {
                // Try root key (for 1-on-1 Genesis and legacy tests)
                if keys.verify_mac(&auth_data, mac) {
                    tracing::debug!("Node verified using root key of epoch {}", epoch);
                    return true;
                }

                // Try initializing from this epoch's root
                let mut chain_key =
                    crate::crypto::ratchet_init_sender(&keys.k_conv, &node.sender_pk);
                let counter = node.sequence_number & 0xFFFFFFFF;
                for _ in 1..counter {
                    chain_key = crate::crypto::ratchet_step(&chain_key);
                }

                let k_msg = crate::crypto::ratchet_message_key(&chain_key);
                let msg_keys = ConversationKeys::derive(&KConv::from(*k_msg.as_bytes()));
                if msg_keys.verify_mac(&auth_data, mac) {
                    tracing::debug!("Node verified using linear ratchet from epoch {}", epoch);
                    return true;
                }
            }
        }

        tracing::debug!(
            "MAC verification failed for node {} (sender={}, seq={})",
            hex::encode(node.hash().as_bytes()),
            hex::encode(node.sender_pk.as_bytes()),
            node.sequence_number
        );
        false
    }

    pub fn unpack_node(
        &self,
        wire: &WireNode,
        candidate_devices: &[PhysicalDevicePk],
    ) -> Option<MerkleNode> {
        let mut epochs: Vec<_> = self.state.epochs.keys().copied().collect();
        epochs.sort_unstable_by(|a, b| b.cmp(a));

        for epoch in epochs {
            if let Some(keys) = self.state.epochs.get(&epoch) {
                // Try root key (for KeyWraps and Admin nodes)
                if let Ok(node) = MerkleNode::unpack_wire(wire, keys) {
                    return Some(node);
                }

                // Trial decrypt with all candidate devices
                for &sender_pk in candidate_devices {
                    if let Some(&(_, ref next_key, _, epoch_id)) =
                        self.state.sender_ratchets.get(&sender_pk)
                        && epoch_id == epoch
                    {
                        let k_msg = crate::crypto::ratchet_message_key(next_key);
                        let msg_keys = ConversationKeys::derive(&KConv::from(*k_msg.as_bytes()));
                        if let Ok(node) = MerkleNode::unpack_wire(wire, &msg_keys)
                            && node.sender_pk == sender_pk
                        {
                            return Some(node);
                        }
                    }

                    // Also try re-initializing from this epoch's root for each candidate device (seq=1)
                    let chain_key = crate::crypto::ratchet_init_sender(&keys.k_conv, &sender_pk);
                    let k_msg = crate::crypto::ratchet_message_key(&chain_key);
                    let msg_keys = ConversationKeys::derive(&KConv::from(*k_msg.as_bytes()));
                    if let Ok(node) = MerkleNode::unpack_wire(wire, &msg_keys)
                        && node.sender_pk == sender_pk
                    {
                        return Some(node);
                    }
                }
            }
        }

        None
    }
}
