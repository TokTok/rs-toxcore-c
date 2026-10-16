use toxxi::msg::{AppCmd, Cmd};
use toxxi::testing::create_test_model;
use toxxi::update::handle_enter;

#[test]
fn test_screenshot_command_extension() {
    let mut model = create_test_model();

    // 1. Test without extension
    let cmds = handle_enter(&mut model, "/screenshot myshot");
    assert_eq!(cmds.len(), 1);
    if let Cmd::App(AppCmd::Screenshot(path, cols, rows)) = &cmds[0] {
        assert_eq!(path, "myshot.svg");
        assert!(cols.is_none());
        assert!(rows.is_none());
    } else {
        panic!("Expected AppCmd::Screenshot, got {:?}", cmds[0]);
    }

    // 2. Test with extension
    let cmds = handle_enter(&mut model, "/screenshot myshot.svg");
    assert_eq!(cmds.len(), 1);
    if let Cmd::App(AppCmd::Screenshot(path, cols, rows)) = &cmds[0] {
        assert_eq!(path, "myshot.svg");
        assert!(cols.is_none());
        assert!(rows.is_none());
    } else {
        panic!("Expected AppCmd::Screenshot, got {:?}", cmds[0]);
    }

    // 3. Test default (timestamp)
    let cmds = handle_enter(&mut model, "/screenshot");
    assert_eq!(cmds.len(), 1);
    if let Cmd::App(AppCmd::Screenshot(path, cols, rows)) = &cmds[0] {
        assert!(path.starts_with("screenshot-"));
        assert!(path.ends_with(".svg"));
        assert!(cols.is_none());
        assert!(rows.is_none());
    } else {
        panic!("Expected AppCmd::Screenshot, got {:?}", cmds[0]);
    }

    // 4. Test alias /sc
    let cmds = handle_enter(&mut model, "/sc alias_shot");
    assert_eq!(cmds.len(), 1);
    if let Cmd::App(AppCmd::Screenshot(path, cols, rows)) = &cmds[0] {
        assert_eq!(path, "alias_shot.svg");
        assert!(cols.is_none());
        assert!(rows.is_none());
    } else {
        panic!("Expected AppCmd::Screenshot, got {:?}", cmds[0]);
    }

    // 5. Test with cols and rows
    let cmds = handle_enter(&mut model, "/screenshot myshot 120 40");
    assert_eq!(cmds.len(), 1);
    if let Cmd::App(AppCmd::Screenshot(path, cols, rows)) = &cmds[0] {
        assert_eq!(path, "myshot.svg");
        assert_eq!(*cols, Some(120));
        assert_eq!(*rows, Some(40));
    } else {
        panic!("Expected AppCmd::Screenshot, got {:?}", cmds[0]);
    }

    // 6. Test with only cols and rows (skipping filename)
    let cmds = handle_enter(&mut model, "/screenshot 120 40");
    assert_eq!(cmds.len(), 1);
    if let Cmd::App(AppCmd::Screenshot(path, cols, rows)) = &cmds[0] {
        assert!(path.starts_with("screenshot-"));
        assert_eq!(*cols, Some(120));
        assert_eq!(*rows, Some(40));
    } else {
        panic!("Expected AppCmd::Screenshot, got {:?}", cmds[0]);
    }

    // 7. Test with invalid dimension (only one number)
    let cmds = handle_enter(&mut model, "/screenshot myshot 120");
    assert_eq!(cmds.len(), 0);
    assert!(model.domain.console_messages.iter().any(|m| m.content
        == toxxi::model::MessageContent::Text(
            "Usage: /screenshot [filename] [cols rows]".to_owned()
        )));
}
