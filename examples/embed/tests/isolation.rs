//! Host pings stay in the host. Guest clicks stay in the Grapheme store.
//! The host never passes an action id into the guest.

use embed::{boot, click_guest, click_host, guest_label, guest_revision, host_pings};

#[test]
fn host_and_guest_actions_stay_isolated() {
    let mut app = boot();
    app.update();
    assert_eq!(host_pings(&app), 0);
    assert_eq!(guest_revision(&mut app), Some(0));
    assert_eq!(guest_label(&mut app), "Count: 0 (doubled: 0)");

    click_host(&mut app);
    assert_eq!(host_pings(&app), 1);
    assert_eq!(guest_revision(&mut app), Some(0));
    assert_eq!(guest_label(&mut app), "Count: 0 (doubled: 0)");

    click_guest(&mut app, "Increment");
    assert_eq!(host_pings(&app), 1);
    assert_eq!(guest_revision(&mut app), Some(1));
    assert_eq!(guest_label(&mut app), "Count: 1 (doubled: 2)");

    click_guest(&mut app, "Add 5");
    assert_eq!(host_pings(&app), 1);
    assert_eq!(guest_revision(&mut app), Some(2));
    assert_eq!(guest_label(&mut app), "Count: 6 (doubled: 12)");

    click_host(&mut app);
    assert_eq!(host_pings(&app), 2);
    assert_eq!(guest_revision(&mut app), Some(2));
    assert_eq!(guest_label(&mut app), "Count: 6 (doubled: 12)");
}
