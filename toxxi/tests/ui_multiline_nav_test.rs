use crossterm::event::{KeyCode, KeyModifiers};
use toxxi::model::{InputMode, Model};
use toxxi::testing::{create_test_model, send_key};

fn get_input_text(model: &Model) -> String {
    model.ui.input_state.text.clone()
}

#[test]
fn test_ui_multiline_navigation() {
    let mut model = create_test_model();

    // 0. Add some history
    model.ui.input_history.push("History 1".to_string());
    model.ui.input_history.push("History 2".to_string());

    // 1. Switch to MultiLine mode
    model.ui.input_mode = InputMode::MultiLine;

    // 2. Insert two lines of text
    // "Line 1"
    for c in "Line 1".chars() {
        send_key(&mut model, KeyCode::Char(c), KeyModifiers::NONE);
    }
    // Enter
    send_key(&mut model, KeyCode::Enter, KeyModifiers::NONE);
    // "Line 2"
    for c in "Line 2".chars() {
        send_key(&mut model, KeyCode::Char(c), KeyModifiers::NONE);
    }

    // Verify state
    model.ui.input_state.ensure_layout(80, "> ");
    let cursor_before_y = model.ui.input_state.cursor().1;
    assert_eq!(cursor_before_y, 1, "Should be on the second line");

    // 3. Press UP (Should move to previous line)
    send_key(&mut model, KeyCode::Up, KeyModifiers::NONE);

    // 4. Verify cursor moved up
    model.ui.input_state.ensure_layout(80, "> ");
    let cursor_after_y = model.ui.input_state.cursor().1;
    assert_eq!(
        cursor_after_y,
        cursor_before_y - 1,
        "Cursor should have moved up 1 line. Before: {}, After: {}",
        cursor_before_y,
        cursor_after_y
    );

    // Verify history didn't trigger (content check)
    let current_text = get_input_text(&model);
    assert!(
        current_text.contains("Line 1"),
        "Text should preserve multi-line content"
    );

    // 5. Press UP again (Should trigger History if at top)
    send_key(&mut model, KeyCode::Up, KeyModifiers::NONE);

    // Verify replaced by "History 2" (last item)
    let text = get_input_text(&model);
    assert!(
        text.contains("History 2"),
        "Should have history item. Got: {:?}",
        text
    );

    // 6. Press DOWN (Should navigate within history item if it has trailing line, or return to saved input)
    send_key(&mut model, KeyCode::Down, KeyModifiers::NONE);

    // Note: get_input_text flattens. "Line 1\nLine 2"
    let restored_text = get_input_text(&model);
    assert!(
        restored_text.contains("Line 1"),
        "Should restore original input. Got: {}",
        restored_text
    );
}
