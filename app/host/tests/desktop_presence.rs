#[path = "../src/desktop_presence.rs"]
mod desktop_presence;
#[path = "../src/lan_playback.rs"]
mod lan_playback;

#[test]
#[ignore = "requires the desktop app to be open"]
fn running_desktop_is_detected() {
    assert!(desktop_presence::is_running());
}

#[test]
fn mailbox_presence_roundtrips_without_changing_queue() {
    let key = [0x11; 32];
    for running in [false, true] {
        let status = lan_playback::MailboxStatus {
            unread_slots: 5,
            running_tasks: 4,
            desktop_running: running,
            coverage_by_slot: [7, 0, 2, 0],
        };
        let packet = lan_playback::encode_mailbox_status(status, 42, &key).unwrap();
        assert_eq!(
            lan_playback::decode_mailbox_status(&packet, &key).unwrap(),
            (status, 42)
        );
    }
}
