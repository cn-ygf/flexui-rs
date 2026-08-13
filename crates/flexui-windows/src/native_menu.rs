use flexui_core::{NativeMenu, NativeMenuPopupAnchor};
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;

pub(crate) fn popup(
    hwnd: HWND,
    menu: &NativeMenu,
    anchor: NativeMenuPopupAnchor,
) -> Option<String> {
    let position = anchor_position(hwnd, anchor);
    unsafe { flexui_native_menu::popup_for_hwnd(menu, hwnd as isize, position) }
}

fn anchor_position(hwnd: HWND, anchor: NativeMenuPopupAnchor) -> Option<(f64, f64)> {
    match anchor {
        NativeMenuPopupAnchor::Cursor => None,
        NativeMenuPopupAnchor::Window(point) => Some((point.x as f64, point.y as f64)),
        NativeMenuPopupAnchor::Screen(point) => {
            let mut client = POINT {
                x: point.x.round() as i32,
                y: point.y.round() as i32,
            };
            unsafe { ScreenToClient(hwnd, &mut client) };
            Some((client.x as f64, client.y as f64))
        }
    }
}
