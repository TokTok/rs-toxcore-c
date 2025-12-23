use merkle_tox_core::dag::{ConversationId, PhysicalDevicePk};
use merkle_tox_core::error::MerkleToxResult;
use merkle_tox_core::node::MerkleToxNode;
use merkle_tox_core::sync::{BlobStore, NodeStore};
use merkle_tox_core::{ProtocolMessage, Transport, TransportError};
use parking_lot::ReentrantMutex;
use std::sync::Arc;
use std::time::Instant;
use toxcore::tox::Tox;
use toxcore::tox::events::Event;
use toxcore::types::PublicKey as ToxPublicKey;
use tracing::debug;

use tokio::sync::Mutex;

pub const TOX_CUSTOM_PACKET_ID: u8 = 200;

/// A transport implementation that uses real Tox custom packets.
#[derive(Clone)]
pub struct ToxTransport {
    pub tox: Arc<ReentrantMutex<Tox>>,
}

impl Transport for ToxTransport {
    fn local_pk(&self) -> PhysicalDevicePk {
        PhysicalDevicePk::from(self.tox.lock().public_key().0)
    }

    fn send_raw(&self, to: PhysicalDevicePk, mut data: Vec<u8>) -> Result<(), TransportError> {
        let tox = self.tox.lock();
        let friend = tox
            .lookup_friend(&ToxPublicKey(*to.as_bytes()))
            .map_err(|_| TransportError::PeerNotFound(hex::encode(to.as_bytes())))?;

        data.insert(0, TOX_CUSTOM_PACKET_ID);
        friend
            .send_lossy_packet(&data)
            .map_err(|e| TransportError::Other(format!("{:?}", e)))?;
        Ok(())
    }
}

/// A bridge between the Tox network and the Merkle-Tox engine.
pub struct ToxMerkleBridge<S: NodeStore + BlobStore> {
    pub node: Arc<Mutex<MerkleToxNode<ToxTransport, S>>>,
}

impl<S: NodeStore + BlobStore> ToxMerkleBridge<S> {
    pub fn new(tox: Tox, store: S) -> Self {
        let self_pk = PhysicalDevicePk::from(tox.public_key().0);
        let transport = ToxTransport {
            tox: Arc::new(ReentrantMutex::new(tox)),
        };
        let engine = merkle_tox_core::engine::MerkleToxEngine::new(
            self_pk,
            self_pk.to_logical(),
            rand::SeedableRng::from_entropy(),
            Arc::new(merkle_tox_core::clock::SystemTimeProvider),
        );
        let node = MerkleToxNode::new(
            engine,
            transport,
            store,
            Arc::new(merkle_tox_core::clock::SystemTimeProvider),
        );
        Self {
            node: Arc::new(Mutex::new(node)),
        }
    }

    pub fn with_node(node: Arc<Mutex<MerkleToxNode<ToxTransport, S>>>) -> Self {
        Self { node }
    }

    /// Initiates history synchronization for a conversation with a specific friend.
    pub async fn start_sync(
        &self,
        friend_pk: PhysicalDevicePk,
        conversation_id: ConversationId,
    ) -> MerkleToxResult<()> {
        let mut node_lock = self.node.lock().await;
        let MerkleToxNode {
            ref mut engine,
            ref store,
            ..
        } = *node_lock;

        let effects = engine.start_sync(conversation_id, Some(friend_pk), store);

        let now = node_lock.time_provider.now_instant();
        let now_ms = node_lock.time_provider.now_system_ms() as u64;
        let mut dummy_wakeup = now;
        for effect in effects {
            let _ = node_lock.process_effect(effect, now, now_ms, &mut dummy_wakeup);
        }

        Ok(())
    }

    /// Offers a single Tox event to the bridge for processing.
    /// Returns Some(pk) if the bridge handled the event as a Merkle-Tox protocol event.
    pub async fn handle_event(&self, event: &Event<'_>) -> Option<ToxPublicKey> {
        let mut node = self.node.lock().await;
        Self::handle_event_locked(&mut node, event)
    }

    /// Internal version of handle_event that works on an already-locked node.
    pub fn handle_event_locked(
        node: &mut MerkleToxNode<ToxTransport, S>,
        event: &Event<'_>,
    ) -> Option<ToxPublicKey> {
        match event {
            Event::FriendConnectionStatus(e) => {
                let status = e.connection_status();
                let pk = node
                    .transport
                    .tox
                    .lock()
                    .friend(e.friend_number())
                    .public_key()
                    .ok();

                if let Some(pk) = pk {
                    let peer_pk = PhysicalDevicePk::from(pk.0);
                    if status != toxcore::tox::ToxConnection::TOX_CONNECTION_NONE {
                        debug!(
                            "Tox friend {:?} connected, initiating MT handshake",
                            peer_pk
                        );
                        node.set_peer_available(peer_pk, true);
                        let caps = ProtocolMessage::CapsAnnounce {
                            version: 1,
                            features: 0,
                        };
                        node.send_message(peer_pk, caps);
                    } else {
                        debug!("Tox friend {:?} disconnected", peer_pk);
                        node.set_peer_available(peer_pk, false);
                    }
                    return Some(pk);
                }
            }
            Event::FriendLossyPacket(e) => {
                let data = e.data();
                if data.first() == Some(&TOX_CUSTOM_PACKET_ID) {
                    let pk = node
                        .transport
                        .tox
                        .lock()
                        .friend(e.friend_number())
                        .public_key()
                        .ok();
                    if let Some(pk) = pk {
                        node.handle_packet(PhysicalDevicePk::from(pk.0), &data[1..]);
                        return Some(pk);
                    }
                }
            }
            _ => {}
        }
        None
    }

    /// Background polling for retransmissions and pacing.
    /// Returns the next scheduled wakeup time.
    pub async fn poll(&self) -> Instant {
        self.node.lock().await.poll()
    }
}
