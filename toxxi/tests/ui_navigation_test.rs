use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use toxcore::tox::{FriendNumber, GroupNumber};
use toxcore::types::PublicKey;
use toxxi::model::WindowId;
use toxxi::testing::{create_test_model, send_key};
use toxxi::ui::draw;

#[test]
fn test_ui_navigation_rendering() {
    let mut model = create_test_model();

    // Setup windows
    let f1_num = FriendNumber(1);
    let f1_pk = PublicKey([1u8; 32]);
    model.session.friend_numbers.insert(f1_num, f1_pk);

    // Manually add FriendInfo to domain so the UI can resolve the name "Alice".
    // ensure_friend_window only creates the Conversation entry, not the FriendInfo.
    model.domain.friends.insert(
        f1_pk,
        toxxi::model::FriendInfo {
            name: "Alice".to_string(),
            public_key: Some(f1_pk),
            status_message: "".to_string(),
            connection: toxcore::tox::ToxConnection::TOX_CONNECTION_NONE,
            is_typing: false,
        },
    );

    model.ensure_friend_window(f1_pk);
    // Also update conversation name (ensure_friend_window might use default "Friend ..." if called before friend info is set)
    if let Some(conv) = model.domain.conversations.get_mut(&WindowId::Friend(f1_pk)) {
        conv.name = "Alice".to_string();
    }

    let g1_num = GroupNumber(1);
    let g1_chat_id = toxcore::types::GroupChatId([1u8; 32]);
    model.session.group_numbers.insert(g1_num, g1_chat_id);

    model.ensure_group_window(g1_chat_id);
    if let Some(conv) = model
        .domain
        .conversations
        .get_mut(&WindowId::Group(g1_chat_id))
    {
        conv.name = "Rustacean Station".to_string();
    }

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    // 1. Start at Console
    terminal.draw(|f| draw(f, &mut model)).unwrap();
    let buffer = terminal.backend().buffer();
    let header_row: String = (0..80).map(|x| buffer[(x, 0)].symbol()).collect();
    assert!(
        header_row.contains("Tox ID:"),
        "Should be on Console window, got header: {}",
        header_row
    );

    // 2. Next Window (Ctrl-n) -> Friend 1 ("Alice")
    send_key(&mut model, KeyCode::Char('n'), KeyModifiers::CONTROL);
    terminal.draw(|f| draw(f, &mut model)).unwrap();

    let buffer = terminal.backend().buffer();
    let header_row: String = (0..80).map(|x| buffer[(x, 0)].symbol()).collect();
    assert!(
        header_row.contains("Alice"),
        "Should be on Friend window 'Alice', got header: {}",
        header_row
    );

    // 3. Next Window (Ctrl-n) -> Group 1 ("Rustacean Station")
    send_key(&mut model, KeyCode::Char('n'), KeyModifiers::CONTROL);
    terminal.draw(|f| draw(f, &mut model)).unwrap();

    let buffer = terminal.backend().buffer();
    let header_row: String = (0..80).map(|x| buffer[(x, 0)].symbol()).collect();
    assert!(
        header_row.contains("Rustacean Station"),
        "Should be on Group window, got header: {}",
        header_row
    );

    // 4. Previous Window (Ctrl-p) -> Back to Alice
    send_key(&mut model, KeyCode::Char('p'), KeyModifiers::CONTROL);

    terminal.draw(|f| draw(f, &mut model)).unwrap();

    let buffer = terminal.backend().buffer();
    let header_row: String = (0..80).map(|x| buffer[(x, 0)].symbol()).collect();
    assert!(
        header_row.contains("Alice"),
        "Should be back on Alice, got header: {}",
        header_row
    );

    // 5. Jump to Console (Alt-0)
    send_key(&mut model, KeyCode::Char('0'), KeyModifiers::ALT);
    terminal.draw(|f| draw(f, &mut model)).unwrap();

    let buffer = terminal.backend().buffer();
    let header_row: String = (0..80).map(|x| buffer[(x, 0)].symbol()).collect();
    assert!(
        header_row.contains("Tox ID:"),
        "Should jump to Console, got header: {}",
        header_row
    );
}
