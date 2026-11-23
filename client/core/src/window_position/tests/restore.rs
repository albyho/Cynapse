use super::*;

fn monitor(x: i32, y: i32, width: i32, height: i32) -> MonitorBounds {
    MonitorBounds {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn a_fully_visible_position_is_preserved() {
    let monitors = [monitor(0, 0, 1920, 1080)];

    assert_eq!(
        restore_position(
            Some(WindowPosition::new(100, 100)),
            (420, 640),
            &monitors,
            Some(monitors[0])
        ),
        Some(WindowPosition::new(100, 100))
    );
}

#[test]
fn a_window_hanging_partly_off_screen_is_preserved() {
    let monitors = [monitor(0, 0, 1920, 1080)];

    assert_eq!(
        restore_position(
            Some(WindowPosition::new(-200, 100)),
            (420, 640),
            &monitors,
            Some(monitors[0])
        ),
        Some(WindowPosition::new(-200, 100))
    );
}

#[test]
fn a_window_spanning_two_monitors_is_preserved() {
    let monitors = [monitor(0, 0, 1920, 1080), monitor(1920, 0, 2560, 1440)];

    assert_eq!(
        restore_position(
            Some(WindowPosition::new(1800, 100)),
            (420, 640),
            &monitors,
            Some(monitors[0])
        ),
        Some(WindowPosition::new(1800, 100))
    );
}

#[test]
fn a_position_on_a_disconnected_monitor_is_recentered() {
    let monitors = [monitor(0, 0, 1920, 1080)];

    assert_eq!(
        restore_position(
            Some(WindowPosition::new(5000, 100)),
            (420, 640),
            &monitors,
            Some(monitors[0])
        ),
        Some(centered_position((420, 640), monitors[0]))
    );
}

#[test]
fn a_minimized_sentinel_is_recentered() {
    let monitors = [monitor(0, 0, 1920, 1080)];

    assert_eq!(
        restore_position(
            Some(WindowPosition::new(-32000, -32000)),
            (420, 640),
            &monitors,
            Some(monitors[0])
        ),
        Some(centered_position((420, 640), monitors[0]))
    );
}

#[test]
fn no_saved_position_stays_none() {
    assert_eq!(
        restore_position(None, (420, 640), &[monitor(0, 0, 1920, 1080)], None),
        None
    );
}

#[test]
fn an_empty_monitor_list_preserves_the_saved_position() {
    assert_eq!(
        restore_position(Some(WindowPosition::new(100, 100)), (420, 640), &[], None),
        Some(WindowPosition::new(100, 100))
    );
}

#[test]
fn centered_position_uses_the_monitor_origin() {
    assert_eq!(
        centered_position((420, 640), monitor(-1920, 0, 1920, 1080)),
        WindowPosition::new(-1920 + (1920 - 420) / 2, (1080 - 640) / 2)
    );
}

#[test]
fn finds_monitor_containing_position() {
    let monitors = [monitor(0, 0, 1707, 1067), monitor(-1920, 0, 1920, 1080)];

    assert_eq!(
        monitor_containing(WindowPosition::new(-500, 100), &monitors),
        Some(monitors[1]),
    );
    assert_eq!(monitor_containing(WindowPosition::new(100, 100), &[]), None);
}
