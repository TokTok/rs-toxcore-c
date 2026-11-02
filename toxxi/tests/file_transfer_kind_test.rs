use toxcore::tox::FriendNumber;
use toxcore::types::{FileId, PublicKey};
use toxxi::msg::{Cmd, Msg, ToxAction, ToxEvent};
use toxxi::testing::{add_test_friend, create_test_model};
use toxxi::update::update;

#[test]
fn test_ignores_unknown_file_transfers() {
    let mut model = create_test_model();
    let fid = FriendNumber(1);
    let pk = PublicKey([1u8; 32]);
    let file_id = FileId([100u8; 32]);

    add_test_friend(&mut model, fid, pk);

    let event = ToxEvent::FileRecv(fid, file_id, 2, 1024, "unknown".to_string());

    let cmds = update(&mut model, Msg::Tox(event));

    // Should verify:
    // 1. Transfer NOT added to model
    assert!(!model.domain.file_transfers.contains_key(&file_id));

    // 2. CANCEL command sent
    let has_cancel = cmds.iter().any(|c| matches!(c, Cmd::Tox(ToxAction::FileControl(f, n, control))
        if *f == pk && *n == file_id && *control == toxcore::types::ToxFileControl::TOX_FILE_CONTROL_CANCEL));

    assert!(
        has_cancel,
        "Should send CANCEL for a file of an unknown kind"
    );
}

#[test]
fn test_accepts_data_file_transfers() {
    let mut model = create_test_model();
    let fid = FriendNumber(1);
    let pk = PublicKey([1u8; 32]);
    let file_id = FileId([101u8; 32]);

    add_test_friend(&mut model, fid, pk);

    // Simulate Data transfer (kind = 0)
    let event = ToxEvent::FileRecv(
        fid,
        file_id,
        0, // Data
        1024,
        "doc.pdf".to_string(),
    );

    let cmds = update(&mut model, Msg::Tox(event));

    // Should verify:
    // 1. Transfer ADDED to model
    assert!(model.domain.file_transfers.contains_key(&file_id));

    // 2. NO cancel command (it waits for user to accept)
    let has_cancel = cmds.iter().any(|c| {
        matches!(c, Cmd::Tox(ToxAction::FileControl(_, _, control))
        if *control == toxcore::types::ToxFileControl::TOX_FILE_CONTROL_CANCEL)
    });

    assert!(!has_cancel, "Should NOT auto-cancel data transfer");
}
