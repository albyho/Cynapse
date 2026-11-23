use super::{MAIN_WINDOW_EDGE_MARGIN, right_aligned_position};
use cynapse_client_core::MonitorBounds;

#[test]
fn first_launch_is_right_aligned_inside_the_work_area() {
    let monitor = MonitorBounds {
        x: 0,
        y: 0,
        width: 1707,
        height: 1019,
    };
    let height = monitor.height - MAIN_WINDOW_EDGE_MARGIN * 2;
    let position = right_aligned_position((420, height), monitor);
    assert_eq!(
        position.x + 420 + MAIN_WINDOW_EDGE_MARGIN,
        monitor.x + monitor.width
    );
    assert_eq!(position.y, monitor.y + MAIN_WINDOW_EDGE_MARGIN);
    assert!(position.y + height <= monitor.y + monitor.height);
}
