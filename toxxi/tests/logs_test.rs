use ratatui::{Terminal, backend::TestBackend};
use toxcore::types::ToxLogLevel;
use toxxi::model::{Model, WindowId};
use toxxi::testing::create_test_model;
use toxxi::ui::draw;
use toxxi::widgets::MessageContent;

fn visible_logs(model: &mut Model) -> Vec<String> {
    if !model.ui.window_ids.contains(&WindowId::Logs) {
        model.ui.window_ids.push(WindowId::Logs);
    }
    let pos = model
        .ui
        .window_ids
        .iter()
        .position(|&w| w == WindowId::Logs)
        .unwrap();
    model.set_active_window(pos);
    model.invalidate_full_window_cache(WindowId::Logs);
    let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
    terminal.draw(|f| draw(f, model)).unwrap();
    model.ui.window_state[&WindowId::Logs]
        .cached_messages
        .as_ref()
        .unwrap()
        .iter()
        .map(|m| match &m.content {
            MessageContent::Text(t) => t.clone(),
            c => panic!("unexpected log content: {:?}", c),
        })
        .collect()
}

#[test]
fn test_add_tox_log() {
    let mut model = create_test_model();

    model.add_tox_log(
        ToxLogLevel::TOX_LOG_LEVEL_INFO,
        "test.c".to_string(),
        123,
        "test_func".to_string(),
        "Test log message".to_string(),
    );

    assert_eq!(
        model
            .domain
            .tox_logs
            .get(&ToxLogLevel::TOX_LOG_LEVEL_INFO)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        visible_logs(&mut model),
        ["INFO test.c:123:test_func Test log message"]
    );
}

#[test]
fn test_tox_log_circular_buffer() {
    let mut model = create_test_model();

    for i in 0..1100 {
        model.add_tox_log(
            ToxLogLevel::TOX_LOG_LEVEL_DEBUG,
            "file.c".to_string(),
            i as u32,
            "func".to_string(),
            format!("message {}", i),
        );
    }

    let all_logs = visible_logs(&mut model);
    assert_eq!(all_logs.len(), 200);
    assert_eq!(all_logs[0], "DEBUG file.c:900:func message 900");
    assert_eq!(all_logs[199], "DEBUG file.c:1099:func message 1099");
}

#[test]
fn test_tox_log_per_level_buckets() {
    let mut model = create_test_model();

    // Fill INFO bucket
    for i in 0..300 {
        model.add_tox_log(
            ToxLogLevel::TOX_LOG_LEVEL_INFO,
            "info.c".to_string(),
            i,
            "func".to_string(),
            "info".to_string(),
        );
    }

    // Add one ERROR
    model.add_tox_log(
        ToxLogLevel::TOX_LOG_LEVEL_ERROR,
        "error.c".to_string(),
        1,
        "func".to_string(),
        "error".to_string(),
    );

    let all_logs = visible_logs(&mut model);
    assert_eq!(all_logs.len(), 201); // 200 INFO + 1 ERROR
    assert!(all_logs.iter().any(|l| l == "ERROR error.c:1:func error"));
}

#[test]
fn test_tox_log_filters() {
    let mut model = create_test_model();

    model.add_tox_log(
        ToxLogLevel::TOX_LOG_LEVEL_INFO,
        "file1.c".to_string(),
        1,
        "f1".to_string(),
        "msg1".to_string(),
    );
    model.add_tox_log(
        ToxLogLevel::TOX_LOG_LEVEL_ERROR,
        "file2.c".to_string(),
        2,
        "f2".to_string(),
        "msg2".to_string(),
    );
    model.add_tox_log(
        ToxLogLevel::TOX_LOG_LEVEL_DEBUG,
        "file1.c".to_string(),
        3,
        "f3".to_string(),
        "other".to_string(),
    );

    assert_eq!(visible_logs(&mut model).len(), 3);

    // Filter by level
    model.ui.log_filters.levels = vec![ToxLogLevel::TOX_LOG_LEVEL_ERROR];
    assert_eq!(visible_logs(&mut model), ["ERROR file2.c:2:f2 msg2"]);

    // Filter by file
    model.ui.log_filters = toxxi::model::LogFilters::default();
    model.ui.log_filters.file_pattern = Some("file1".to_string());
    assert_eq!(visible_logs(&mut model).len(), 2);

    // Filter by msg
    model.ui.log_filters = toxxi::model::LogFilters::default();
    model.ui.log_filters.msg_pattern = Some("msg".to_string());
    assert_eq!(visible_logs(&mut model).len(), 2);

    // Combined
    model.ui.log_filters.file_pattern = Some("file2".to_string());
    assert_eq!(visible_logs(&mut model), ["ERROR file2.c:2:f2 msg2"]);
}

#[test]
fn test_tox_log_incremental_filters() {
    let mut model = create_test_model();
    model.add_tox_log(
        ToxLogLevel::TOX_LOG_LEVEL_INFO,
        "file.c".to_string(),
        1,
        "f".to_string(),
        "msg".to_string(),
    );
    model.add_tox_log(
        ToxLogLevel::TOX_LOG_LEVEL_TRACE,
        "file.c".to_string(),
        2,
        "f".to_string(),
        "msg".to_string(),
    );

    // Initially 2 logs
    assert_eq!(visible_logs(&mut model).len(), 2);

    // Remove trace
    toxxi::update::handle_command(&mut model, "/logs level=-trace");
    assert_eq!(visible_logs(&mut model), ["INFO file.c:1:f msg"]);

    // Add trace back
    toxxi::update::handle_command(&mut model, "/logs level=+trace");
    assert_eq!(visible_logs(&mut model).len(), 2);

    // Set to only error (which doesn't exist)
    toxxi::update::handle_command(&mut model, "/logs level=error");
    assert_eq!(visible_logs(&mut model).len(), 0);

    // Clear level filter
    toxxi::update::handle_command(&mut model, "/logs level=");
    assert_eq!(visible_logs(&mut model).len(), 2);
}

#[test]
fn test_logs_clear_and_all() {
    let mut model = create_test_model();

    // 1. Add some logs and set a filter
    model.add_tox_log(
        ToxLogLevel::TOX_LOG_LEVEL_INFO,
        "file.c".to_string(),
        1,
        "f".to_string(),
        "msg".to_string(),
    );
    toxxi::update::handle_command(&mut model, "/logs level=error");

    assert_eq!(model.domain.tox_logs.values().flatten().count(), 1);
    assert_eq!(model.ui.log_filters.levels.len(), 1);

    // 2. Test /logs all (should clear filters but keep logs)
    toxxi::update::handle_command(&mut model, "/logs all");
    assert_eq!(model.ui.log_filters.levels.len(), 0);
    assert_eq!(model.domain.tox_logs.values().flatten().count(), 1);

    // 3. Test /logs clear (should clear logs)
    toxxi::update::handle_command(&mut model, "/logs clear");
    assert_eq!(model.domain.tox_logs.values().flatten().count(), 0);
}

#[test]
fn test_logs_pause_resume() {
    let mut model = create_test_model();

    // 1. Add a log
    model.add_tox_log(
        ToxLogLevel::TOX_LOG_LEVEL_INFO,
        "file.c".to_string(),
        1,
        "f".to_string(),
        "msg 1".to_string(),
    );
    assert_eq!(visible_logs(&mut model).len(), 1);

    // 2. Pause
    toxxi::update::handle_command(&mut model, "/logs pause");
    assert!(model.ui.log_filters.paused);

    // 3. Add a log while paused
    model.add_tox_log(
        ToxLogLevel::TOX_LOG_LEVEL_INFO,
        "file.c".to_string(),
        2,
        "f".to_string(),
        "msg 2".to_string(),
    );
    // Should still have only 1 log
    assert_eq!(visible_logs(&mut model), ["INFO file.c:1:f msg 1"]);

    // 4. Resume
    toxxi::update::handle_command(&mut model, "/logs resume");
    assert!(!model.ui.log_filters.paused);

    // 5. Add a log after resuming
    model.add_tox_log(
        ToxLogLevel::TOX_LOG_LEVEL_INFO,
        "file.c".to_string(),
        3,
        "f".to_string(),
        "msg 3".to_string(),
    );
    // Should have 2 logs (msg 1 and msg 3)
    assert_eq!(
        visible_logs(&mut model),
        ["INFO file.c:1:f msg 1", "INFO file.c:3:f msg 3"]
    );
}

// end of tests
