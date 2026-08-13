use flexui_core::{NativeMenu, NativeMenuPopupAnchor};
use objc2::rc::Retained;
use objc2_app_kit::{NSScreen, NSView, NSWindow};
use objc2_foundation::{MainThreadMarker, NSPoint};

pub(crate) fn popup(
    window: &NSWindow,
    menu: &NativeMenu,
    anchor: NativeMenuPopupAnchor,
) -> Option<String> {
    let view = window.contentView()?;
    let position = anchor_position(window, &view, anchor).map(|point| (point.x, point.y));
    unsafe { flexui_native_menu::popup_for_nsview(menu, Retained::as_ptr(&view).cast(), position) }
}

fn anchor_position(
    window: &NSWindow,
    view: &NSView,
    anchor: NativeMenuPopupAnchor,
) -> Option<NSPoint> {
    match anchor {
        NativeMenuPopupAnchor::Cursor => None,
        NativeMenuPopupAnchor::Window(point) => Some(NSPoint::new(point.x as f64, point.y as f64)),
        NativeMenuPopupAnchor::Screen(point) => {
            let mtm = MainThreadMarker::new()?;
            let desktop_top = NSScreen::screens(mtm)
                .iter()
                .map(|screen| screen.frame().origin.y + screen.frame().size.height)
                .fold(0.0_f64, f64::max);
            let window_point = window
                .convertPointFromScreen(NSPoint::new(point.x as f64, desktop_top - point.y as f64));
            Some(view.convertPoint_fromView(window_point, None))
        }
    }
}
