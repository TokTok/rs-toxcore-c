use crate::dag::{Content, ControlAction, ConversationId};
use crate::engine::processor::VerifiedNode;
use crate::engine::{Conversation, Effect, MerkleToxEngine};
use crate::error::MerkleToxResult;
use crate::sync::NodeStore;
use tracing::info;

impl MerkleToxEngine {
    /// Applies the administrative and cryptographic side-effects of a verified node.
    pub fn apply_side_effects(
        &mut self,
        conversation_id: ConversationId,
        node: &VerifiedNode,
        store: &dyn NodeStore,
    ) -> MerkleToxResult<Vec<Effect>> {
        let (node_ref, content) = (node.node(), node.content());
        let mut effects = Vec::new();

        let overlay = crate::engine::EngineStore {
            store,
            cache: &self.pending_cache,
        };

        // Special case: RatchetSnapshot for self-recovery
        if let Content::RatchetSnapshot { epoch, ciphertext } = content
            && let Some(sk) = &self.self_dh_sk
            && let Ok(wrapped_keys) =
                tox_proto::deserialize::<Vec<crate::dag::WrappedKey>>(ciphertext)
        {
            for wrapped in wrapped_keys {
                if wrapped.recipient_pk == self.self_pk {
                    // Unwrap ratchet chain key
                    if let Some(k_conv) = crate::crypto::ConversationKeys::unwrap_from(
                        sk.as_bytes(),
                        &node_ref.sender_pk,
                        &wrapped.ciphertext,
                    ) {
                        let chain_key = k_conv.to_chain_key();
                        if let Some(Conversation::Established(em)) =
                            self.conversations.get_mut(&conversation_id)
                            && em.get_keys(*epoch).is_some()
                        {
                            info!("Resuming ratchet from snapshot in epoch {}", epoch);
                            em.commit_node_key(
                                node_ref.sender_pk,
                                node_ref.sequence_number,
                                chain_key.clone(),
                                node.hash(),
                                *epoch,
                            );
                            effects.push(Effect::WriteRatchetKey(
                                conversation_id,
                                node.hash(),
                                chain_key,
                                *epoch,
                            ));
                        }
                    }
                    break;
                }
            }
        }

        // Apply Administrative Actions
        match content {
            Content::Control(ControlAction::Genesis {
                creator_pk,
                created_at,
                ..
            }) => {
                self.identity_manager.add_member(
                    conversation_id,
                    *creator_pk,
                    0, // Role: Owner/Admin
                    *created_at,
                );
            }
            Content::Control(ControlAction::AuthorizeDevice { cert }) => {
                self.identity_manager.authorize_device(
                    conversation_id,
                    node_ref.author_pk,
                    cert,
                    node_ref.network_timestamp,
                    node_ref.topological_rank,
                )?;
            }
            Content::Control(ControlAction::RevokeDevice {
                target_device_pk, ..
            }) => {
                self.identity_manager.revoke_device(
                    conversation_id,
                    *target_device_pk,
                    node_ref.topological_rank,
                );
                // Membership change should trigger rotation if we are admin
            }
            Content::Control(ControlAction::Invite(invite)) => {
                self.identity_manager.add_member(
                    conversation_id,
                    invite.invitee_pk,
                    invite.role,
                    node_ref.network_timestamp,
                );
            }
            Content::Control(ControlAction::Leave(logical_pk)) => {
                self.identity_manager.remove_member(
                    conversation_id,
                    *logical_pk,
                    node_ref.topological_rank,
                );
            }
            Content::Control(ControlAction::Rekey { new_epoch }) => {
                info!(
                    "Conversation {:?} rotated to epoch {}",
                    conversation_id, new_epoch
                );
            }
            Content::Control(action @ ControlAction::Announcement { .. }) => {
                self.peer_announcements
                    .insert(node_ref.sender_pk, action.clone());
            }
            _ => {}
        }

        // Advance ratchet if keys are available
        if let Some(Conversation::Established(em)) = self.conversations.get_mut(&conversation_id) {
            if let Some((_, k_next)) = em.peek_keys(&node_ref.sender_pk, node_ref.sequence_number) {
                tracing::debug!(
                    "Advancing ratchet for node {} (sender={}, seq={})",
                    hex::encode(node.hash().as_bytes()),
                    hex::encode(node_ref.sender_pk.as_bytes()),
                    node_ref.sequence_number
                );
                let prev_hash = em.commit_node_key(
                    node_ref.sender_pk,
                    node_ref.sequence_number,
                    k_next.clone(),
                    node.hash(),
                    em.current_epoch(),
                );
                effects.push(Effect::WriteRatchetKey(
                    conversation_id,
                    node.hash(),
                    k_next,
                    em.current_epoch(),
                ));

                // Purge previous ratchet key from persistent storage.
                if let Some(prev) = prev_hash {
                    tracing::debug!(
                        "Purging old ratchet key for previous node {}",
                        hex::encode(prev.as_bytes())
                    );
                    effects.push(Effect::DeleteRatchetKey(conversation_id, prev));
                }
            } else {
                tracing::debug!(
                    "Ratchet NOT advanced for node {}: peek_keys returned None",
                    hex::encode(node.hash().as_bytes())
                );
            }
        }

        effects.extend(update_heads(conversation_id, node, &overlay)?);
        Ok(effects)
    }
}

fn update_heads(
    conversation_id: ConversationId,
    node: &VerifiedNode,
    overlay: &crate::engine::EngineStore,
) -> MerkleToxResult<Vec<Effect>> {
    let node_ref = node.node();
    let hash = node.hash();
    let mut effects = Vec::new();

    if node_ref.node_type() == crate::dag::NodeType::Admin {
        let mut heads = overlay.get_admin_heads(&conversation_id);
        heads.retain(|h| !node_ref.parents.contains(h));
        if !heads.contains(&hash) {
            heads.push(hash);
        }
        overlay.set_admin_heads(&conversation_id, heads.clone())?;
        effects.push(Effect::UpdateHeads(conversation_id, heads, true));
    } else {
        let mut heads = overlay.get_heads(&conversation_id);
        heads.retain(|h| !node_ref.parents.contains(h));
        if !heads.contains(&hash) {
            heads.push(hash);
        }
        overlay.set_heads(&conversation_id, heads.clone())?;
        effects.push(Effect::UpdateHeads(conversation_id, heads, false));
    }

    Ok(effects)
}
