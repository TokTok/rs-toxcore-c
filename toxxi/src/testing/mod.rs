pub mod lib;

use crate::app::AppContext;
use crate::config::Config;
use crate::model::DomainState;
use crate::model::FriendInfo;
use crate::model::Model;
use crate::model::SessionState;
use crate::model::ToxSelfInfo;
use crate::model::UiState;
use crate::model::load_or_initialize;
use crate::msg::{Cmd, IOAction, Msg};
use crate::time::RealTimeProvider;
use crate::update::update;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
pub use lib::{buffer_to_string, configure_insta};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use tempfile::TempDir;
use tokio::runtime::Runtime;
use toxcore::tox::Address;
use toxcore::tox::FriendNumber;
use toxcore::tox::ToxConnection;
use toxcore::tox::ToxUserStatus;
use toxcore::types::PublicKey;

pub fn create_test_model() -> Model {
    let config = Config::default();
    let domain = DomainState::new(
        Address([0u8; 38]),
        "Tester".to_string(),
        "I am a test".to_string(),
        ToxUserStatus::TOX_USER_STATUS_NONE,
    );
    Model::new(domain, config.clone(), config)
}

pub fn add_test_friend(model: &mut Model, fid: FriendNumber, pk: PublicKey) {
    model.session.friend_numbers.insert(fid, pk);
    model.domain.friends.insert(
        pk,
        FriendInfo {
            name: format!("Friend {}", fid.0),
            public_key: Some(pk),
            status_message: "".to_string(),
            connection: ToxConnection::TOX_CONNECTION_TCP,
            is_typing: false,
        },
    );
    model.ensure_friend_window(pk);
}

pub fn send_key(model: &mut Model, code: KeyCode, modifiers: KeyModifiers) -> Vec<Cmd> {
    update(
        model,
        Msg::Input(Event::Key(KeyEvent::new(code, modifiers))),
    )
}

pub fn start(config_dir: &Path, friends: &[PublicKey]) -> Model {
    let self_info = ToxSelfInfo {
        tox_id: Address([0u8; 38]),
        name: "Me".to_owned(),
        status_msg: String::new(),
        status_type: ToxUserStatus::TOX_USER_STATUS_NONE,
    };
    let friends = friends
        .iter()
        .enumerate()
        .map(|(i, &pk)| {
            (
                FriendNumber(i as u32),
                FriendInfo {
                    name: format!("Friend {}", i),
                    public_key: Some(pk),
                    status_message: String::new(),
                    connection: ToxConnection::TOX_CONNECTION_NONE,
                    is_typing: false,
                },
            )
        })
        .collect();
    load_or_initialize(
        config_dir,
        self_info,
        friends,
        vec![],
        vec![],
        Config::default(),
        Config::default(),
    )
}

pub fn app_context(rt: &Runtime, dir: &Path) -> (AppContext, mpsc::Receiver<IOAction>) {
    let (tx_tox_action, _) = mpsc::channel();
    let (tx_io, rx_io) = mpsc::channel();
    let ctx = AppContext {
        tx_tox_action,
        tox_handle: rt.spawn(async {}),
        savedata_path: None,
        config_dir: dir.to_path_buf(),
        tx_io,
        screenshots_dir: dir.join("screenshots"),
    };
    (ctx, rx_io)
}

pub struct TestContext {
    pub temp_dir: TempDir,
    pub config_dir: PathBuf,
}

impl Default for TestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TestContext {
    pub fn new() -> Self {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().to_path_buf();
        Self {
            temp_dir,
            config_dir,
        }
    }

    pub fn create_model(&self) -> Model {
        let config = Config::default();
        let domain = DomainState {
            tox_id: Address([1u8; 38]), // Dummy address
            self_name: "Tester".to_string(),
            self_status_message: "".to_string(),
            self_status_type: ToxUserStatus::TOX_USER_STATUS_NONE,
            self_connection_status: ToxConnection::TOX_CONNECTION_NONE,
            friends: HashMap::new(),
            conversations: HashMap::new(),
            console_messages: Vec::new(),
            tox_logs: HashMap::new(),
            pending_items: Vec::new(),
            next_internal_id: crate::model::InternalMessageId(1),
            file_transfers: HashMap::new(),
        };

        Model {
            domain,
            ui: UiState::new(),
            session: SessionState::default(),
            saved_config: config.clone(),
            config,
            tick_count: 0,
            time_provider: Arc::new(RealTimeProvider::new(None)),
            events: Default::default(),
            message_events: HashMap::new(),
            account: Default::default(),
            blobs: Default::default(),
        }
    }
}
