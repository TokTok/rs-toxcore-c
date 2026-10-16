use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use toxxi::testing::{create_test_model, send_key};
use toxxi::ui::draw;

#[test]
fn test_ui_horizontal_navigation_rendering() {
    let mut model = create_test_model();

    // Setup backend
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    // 1. Insert text "ABC"
    for c in "ABC".chars() {
        send_key(&mut model, KeyCode::Char(c), KeyModifiers::NONE);
    }

    // 2. Move Cursor Left 2 times (to 'B')
    send_key(&mut model, KeyCode::Left, KeyModifiers::NONE);
    send_key(&mut model, KeyCode::Left, KeyModifiers::NONE);

    // 3. Move Right 1 time (to 'C')
    send_key(&mut model, KeyCode::Right, KeyModifiers::NONE);

    // 4. Press Right Arrow again (should stay at end, not scroll)
    send_key(&mut model, KeyCode::Right, KeyModifiers::NONE);

    model.ui.input_state.ensure_layout(80, "> ");
    let (cursor_x, cursor_y) = model.ui.input_state.cursor();
    assert_eq!(
        cursor_x, 5,
        "Cursor should be at column 5 (2 prompt + 3 text)"
    );
    assert_eq!(cursor_y, 0, "Cursor should stay on line 0");

    let vscroll = model.ui.input_state.scroll;
    assert_eq!(vscroll, 0, "Vertical scroll should be 0. Got: {}", vscroll);

    // 5. Draw
    terminal.draw(|f| draw(f, &mut model)).unwrap();

    // 6. Verify Content
    let buffer = terminal.backend().buffer();
    let input_row_idx = 22;
    let input_row: String = (0..80)
        .map(|x| buffer[(x, input_row_idx)].symbol())
        .collect();

    // "ABC" should be present
    assert!(
        input_row.contains("ABC"),
        "Input row should contain 'ABC'. Got: '{}'",
        input_row
    );

    let cursor_pos = terminal.get_cursor_position().unwrap();
    assert_eq!(
        cursor_pos.x, 6,
        "Cursor X should be 6 (1 border + 2 prompt + 3 text)"
    );
    assert_eq!(
        cursor_pos.y, 22,
        "Cursor Y should be 22 (row above bottom border)"
    );
}
