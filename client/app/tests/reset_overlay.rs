//! Busy overlay component tests.

use std::cell::RefCell;
use std::rc::Rc;

use cynapse_client::views::busy_overlay::{BusyOverlay, BusyOverlayContent};
use gpui_kit::prelude::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    Context, Entity, IntoElement, ParentElement, Render, TestAppContext, Window, div, px, size,
};

struct OverlayHost {
    overlay: Entity<BusyOverlay>,
}

impl Render for OverlayHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.overlay.clone())
    }
}

fn sync_reset_overlay(
    overlay: &mut BusyOverlay,
    cx: &mut Context<BusyOverlay>,
    visible: bool,
    label: Option<String>,
) {
    overlay.sync(
        visible,
        BusyOverlayContent {
            label,
            ..Default::default()
        },
        cx,
    );
}

#[gpui_kit::test]
fn busy_overlay_stays_hidden_until_a_reset_starts(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(500.), px(300.)), |_, cx| {
        let overlay = cx.new(|_| BusyOverlay::new());
        OverlayHost { overlay }
    });

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("busy-overlay").is_none());
    })
    .expect("overlay host window");
}

#[gpui_kit::test]
fn fast_reset_before_the_first_frame_does_not_occlude(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(500.), px(300.)), |_, cx| {
        let overlay = cx.new(|_| BusyOverlay::new());
        overlay.update(cx, |overlay, cx| {
            sync_reset_overlay(overlay, cx, true, Some("正在重置课程".to_string()));
            sync_reset_overlay(overlay, cx, false, None);
        });
        OverlayHost { overlay }
    });

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("busy-overlay").is_none());
    })
    .expect("overlay host window");
}

#[gpui_kit::test]
fn busy_overlay_renders_startup_steps(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(|cx| cx.set_reduce_motion(true));
    let window = cx.open_window(size(px(500.), px(300.)), |_, cx| {
        let overlay = cx.new(|_| BusyOverlay::new());
        overlay.update(cx, |overlay, cx| {
            overlay.sync(
                true,
                BusyOverlayContent {
                    label: Some("正在启动学习".to_string()),
                    steps: vec![
                        "准备课程环境".to_string(),
                        "连接编辑环境".to_string(),
                        "准备工作区".to_string(),
                    ],
                    active_step: Some(1),
                    hint: Some("首次启动可能需要几分钟".to_string()),
                    ..Default::default()
                },
                cx,
            );
        });
        OverlayHost { overlay }
    });

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("busy-overlay").visible());
        assert!(window.find("busy-overlay-label").visible());
    })
    .expect("overlay host window");
}

#[gpui_kit::test]
fn busy_overlay_renders_while_reset_pending(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(|cx| cx.set_reduce_motion(true));
    let overlay_for_test = Rc::new(RefCell::new(None));
    let overlay_for_window = overlay_for_test.clone();
    let window = cx.open_window(size(px(500.), px(300.)), move |_, cx| {
        let overlay = cx.new(|_| BusyOverlay::new());
        overlay.update(cx, |overlay, cx| {
            sync_reset_overlay(overlay, cx, true, Some("正在重置课程".to_string()));
        });
        *overlay_for_window.borrow_mut() = Some(overlay.clone());
        OverlayHost { overlay }
    });

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("busy-overlay").visible());
        assert!(window.find("busy-overlay-label").visible());

        overlay_for_test
            .borrow()
            .as_ref()
            .expect("overlay entity")
            .update(cx, |overlay, cx| {
                sync_reset_overlay(overlay, cx, false, None);
            });
        window.render_frame(cx);
        assert!(window.try_find("busy-overlay").is_none());
    })
    .expect("overlay host window");
}
