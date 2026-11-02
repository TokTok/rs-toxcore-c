//! Avatars over Tox, as `Avatar` facts for the account's devices. See
//! doc/account-sync.md.

use toxcore::tox::ToxConnection;
use toxcore::types::{FileId, PublicKey, ToxFileControl};

use super::event::{BlobId, Content};
use super::fact::{Subject, Value};
use super::files;
use crate::model::Model;
use crate::msg::{Cmd, IOAction, ToxAction};

/// `TOX_FILE_KIND_AVATAR`.
pub const FILE_KIND: u32 = 1;

pub const MAX_SIZE: u64 = 64 * 1024;

/// The file ID of an empty avatar file, which says there is none.
const NONE_FILE_ID: FileId = FileId([0; 32]);

fn avatar(model: &Model, subject: &Subject) -> Option<BlobId> {
    model.account.facts.find(subject, |value| match value {
        Value::Avatar(avatar) => Some(*avatar),
        _ => None,
    })?
}

/// Records `subject`'s avatar if it changed.
fn record(model: &mut Model, subject: Subject, avatar: Option<BlobId>) -> Vec<Cmd> {
    let value = Value::Avatar(avatar);
    if model.account.facts.of(&subject).contains(&value) {
        return Vec::new();
    }
    model.record_in_account(vec![Content::Fact { subject, value }])
}

/// Its file ID is its blob ID, and an empty file says there's none.
pub fn offered(model: &mut Model, pk: PublicKey, file_id: FileId, size: u64) -> Vec<Cmd> {
    let blob = BlobId(file_id.0);
    let mut cmds = vec![Cmd::Tox(ToxAction::FileControl(
        pk,
        file_id,
        ToxFileControl::TOX_FILE_CONTROL_CANCEL,
    ))];
    match size {
        0 => cmds.extend(record(model, Subject::Friend(pk), None)),
        _ if size > MAX_SIZE => {}
        _ if model.blobs.holds(&blob) => {
            cmds.extend(record(model, Subject::Friend(pk), Some(blob)));
        }
        _ if !model.session.blob_transfers.contains_key(&blob) => {
            model.session.blob_transfers.insert(
                blob,
                files::BlobTransfer {
                    device: pk,
                    incoming: true,
                    size: Some(size),
                },
            );
            model.session.avatar_offers.insert(pk, blob);
            return vec![Cmd::IO(IOAction::ReceiveBlob(pk, blob, size))];
        }
        // Another friend is sending the same avatar.
        _ => {}
    }
    cmds
}

/// `None` if the blob from `pk` is no avatar.
pub fn stored(model: &mut Model, pk: PublicKey, blob: BlobId, ok: bool) -> Option<Vec<Cmd>> {
    if model.session.avatar_offers.get(&pk) == Some(&blob) {
        model.session.avatar_offers.remove(&pk);
        return Some(if ok {
            record(model, Subject::Friend(pk), Some(blob))
        } else {
            Vec::new()
        });
    }
    if model.session.avatar_wish == Some(blob) {
        model.session.avatar_wish = None;
        return Some(if ok {
            set(model, Some(blob))
        } else {
            Vec::new()
        });
    }
    None
}

/// An empty file says the account has none. Devices get it from the
/// `Avatar` fact instead.
pub fn send(model: &mut Model, pk: PublicKey) -> Vec<Cmd> {
    if model.config.sync_devices.contains(&pk) {
        return Vec::new();
    }
    match avatar(model, &Subject::Account) {
        Some(blob) if model.blobs.holds(&blob) => files::send(model, pk, blob, FILE_KIND),
        Some(_) => Vec::new(),
        None => vec![Cmd::Tox(ToxAction::FileSend(
            pk,
            FILE_KIND,
            0,
            String::new(),
            String::new(),
            Some(NONE_FILE_ID),
        ))],
    }
}

/// Whether a file toxxi started is the empty one `send` says no avatar with.
pub fn is_none_file(file_id: &FileId, size: u64) -> bool {
    *file_id == NONE_FILE_ID && size == 0
}

/// Makes `avatar` the account's and sends it to the friends online.
fn set(model: &mut Model, avatar: Option<BlobId>) -> Vec<Cmd> {
    let mut cmds = record(model, Subject::Account, avatar);
    let online: Vec<PublicKey> = model
        .domain
        .friends
        .iter()
        .filter(|(_, info)| info.connection != ToxConnection::TOX_CONNECTION_NONE)
        .map(|(pk, _)| *pk)
        .collect();
    for pk in online {
        cmds.extend(send(model, pk));
    }
    cmds
}

/// The blob is fetched from `device` first.
pub fn wish(model: &mut Model, device: PublicKey, avatar: Option<BlobId>) -> Vec<Cmd> {
    match avatar {
        Some(blob) if !model.blobs.holds(&blob) => {
            model.session.avatar_wish = Some(blob);
            files::want(model, device, blob)
        }
        _ => set(model, avatar),
    }
}
