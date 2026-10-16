use crossterm::event::{KeyCode, KeyModifiers};
use toxcore::tox::FriendNumber;
use toxcore::types::PublicKey;
use toxxi::model::{ConsoleMessageType, WindowId};
use toxxi::msg::{Cmd, ToxAction};
use toxxi::testing::{add_test_friend, create_test_model, send_key};

fn get_text(input: &toxxi::widgets::InputBoxState) -> String {
    input.text.clone()
}

#[test]
fn test_block_feature() {
    let mut model = create_test_model();
    let fid = FriendNumber(1);
    let pk = PublicKey([1u8; 32]);

    // 1. Add a friend so we can try to send a message
    add_test_friend(&mut model, fid, pk);
    model.set_active_window(1);
    assert_eq!(model.active_window_id(), WindowId::Friend(pk));

    // 2. Add a blocked string
    for c in "/block add hunter2".chars() {
        send_key(&mut model, KeyCode::Char(c), KeyModifiers::empty());
    }
    send_key(&mut model, KeyCode::Enter, KeyModifiers::empty());
    assert!(
        model
            .config
            .blocked_strings
            .contains(&"hunter2".to_string())
    );

    // 3. Try to send a message containing the blocked string
    for c in "don't say hunter2!".chars() {
        send_key(&mut model, KeyCode::Char(c), KeyModifiers::empty());
    }
    let cmds = send_key(&mut model, KeyCode::Enter, KeyModifiers::empty());

    // 4. Verify message was NOT sent
    assert!(cmds.is_empty());
    assert!(!model.ui.input_blocked_indices.is_empty());
    assert_eq!(get_text(&model.ui.input_state), "don't say hunter2!");

    // Check that we got an error message
    let last_msg = model.domain.console_messages.last().unwrap();
    assert_eq!(last_msg.msg_type, ConsoleMessageType::Error);

    // 5. Start editing again - blocked indices should clear
    send_key(&mut model, KeyCode::Left, KeyModifiers::empty());
    assert!(model.ui.input_blocked_indices.is_empty());

    // 6. Unblock and try again
    model.ui.input_state.clear();
    for c in "/block remove hunter2".chars() {
        send_key(&mut model, KeyCode::Char(c), KeyModifiers::empty());
    }
    send_key(&mut model, KeyCode::Enter, KeyModifiers::empty());
    assert!(
        !model
            .config
            .blocked_strings
            .contains(&"hunter2".to_string())
    );

    for c in "now I can say hunter2".chars() {
        send_key(&mut model, KeyCode::Char(c), KeyModifiers::empty());
    }
    let cmds = send_key(&mut model, KeyCode::Enter, KeyModifiers::empty());

    // 7. Verify message IS sent
    assert!(!cmds.is_empty());
    let found = cmds.iter().any(|c| {
        if let Cmd::Tox(ToxAction::SendMessage(f, _, msg, _)) = c {
            *f == pk && msg == b"now I can say hunter2"
        } else {
            false
        }
    });
    if !found {
        panic!("Expected SendMessage command, got: {:?}", cmds);
    }
    assert!(get_text(&model.ui.input_state).is_empty());
}

#[test]
fn test_block_case_insensitive() {
    let mut model = create_test_model();
    let fid = FriendNumber(1);
    let pk = PublicKey([1u8; 32]);
    add_test_friend(&mut model, fid, pk);
    model.set_active_window(1);

    model.config.blocked_strings.push("HUNTER2".to_string());

    for c in "I love hunter2".chars() {
        send_key(&mut model, KeyCode::Char(c), KeyModifiers::empty());
    }
    let cmds = send_key(&mut model, KeyCode::Enter, KeyModifiers::empty());
    assert!(
        !cmds
            .iter()
            .any(|cmd| matches!(cmd, Cmd::Tox(ToxAction::SendMessage(..))))
    );
    assert!(!model.ui.input_blocked_indices.is_empty());
}
