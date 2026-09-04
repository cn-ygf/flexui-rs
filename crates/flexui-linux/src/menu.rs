//! 系统原生弹出菜单（X11 override-redirect 窗口 + Cairo 自绘 + 指针/键盘抓取）。
//!
//! macOS/Windows 有 NSMenu/HMENU，Linux 无统一原生菜单 API（除 GTK），故用 X11 原语
//! 自绘一个同步弹出菜单：popup_native_menu 阻塞直到用户选中项(返回 id)或取消(None)。
//! 支持项/分隔线/子菜单(递归)/悬停高亮/键盘上下回车 Esc。

use cairo::{Format, ImageSurface};
use flexui_gfx::{Canvas, Color, Font, Rect, TextAlign};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask, GrabMode, ImageFormat, Window,
    WindowClass,
};
use x11rb::protocol::Event as XEvent;
use x11rb::rust_connection::RustConnection;
use x11rb::COPY_DEPTH_FROM_PARENT;

use flexui_core::{NativeMenu, NativeMenuEntry, NativeMenuPopupAnchor};

use crate::canvas::CairoCanvas;

const ITEM_H: f32 = 28.0;
const SEP_H: f32 = 9.0;
const PAD_X: f32 = 14.0;
const GAP: f32 = 28.0; // 文本与快捷键/箭头之间

// 配色（浅色菜单）。
const BG: Color = Color::rgba(1.0, 1.0, 1.0, 1.0);
const BORDER: Color = Color::rgba(0.80, 0.82, 0.86, 1.0);
const TEXT: Color = Color::rgba(0.12, 0.13, 0.15, 1.0);
const TEXT_DIM: Color = Color::rgba(0.55, 0.57, 0.60, 1.0);
const HOVER: Color = Color::rgba(0.20, 0.45, 0.95, 1.0);
const HOVER_TEXT: Color = Color::rgba(1.0, 1.0, 1.0, 1.0);

/// 一行的可见内容。
enum Row {
    Item {
        id: String,
        text: String,
        shortcut: Option<String>,
        enabled: bool,
        checked: bool,
    },
    Sep,
    Sub {
        text: String,
        enabled: bool,
        items: Vec<NativeMenuEntry>,
    },
}

/// 弹出原生菜单。anchor 为屏幕物理像素位置。返回选中项 id。
pub fn popup(
    conn: &RustConnection,
    parent: Window,
    anchor: NativeMenuPopupAnchor,
    menu: &NativeMenu,
    scale: f32,
) -> Option<String> {
    let (ax, ay) = anchor_screen(conn, parent, anchor, scale)?;
    popup_at(conn, ax, ay, &menu.items, scale)
}

/// 计算 anchor 的屏幕物理坐标。
fn anchor_screen(
    conn: &RustConnection,
    parent: Window,
    anchor: NativeMenuPopupAnchor,
    scale: f32,
) -> Option<(i16, i16)> {
    let root = conn.setup().roots.first()?.root;
    match anchor {
        NativeMenuPopupAnchor::Cursor => {
            let p = conn.query_pointer(root).ok()?.reply().ok()?;
            Some((p.root_x, p.root_y))
        }
        NativeMenuPopupAnchor::Window(pt) => {
            // 窗口逻辑坐标 → 相对根窗口。
            let t = conn
                .translate_coordinates(parent, root, (pt.x * scale) as i16, (pt.y * scale) as i16)
                .ok()?
                .reply()
                .ok()?;
            Some((t.dst_x, t.dst_y))
        }
        NativeMenuPopupAnchor::Screen(pt) => Some(((pt.x * scale) as i16, (pt.y * scale) as i16)),
    }
}

/// 在屏幕位置弹出一级菜单（子菜单递归）。
fn popup_at(
    conn: &RustConnection,
    sx: i16,
    sy: i16,
    entries: &[NativeMenuEntry],
    scale: f32,
) -> Option<String> {
    let rows: Vec<Row> = entries.iter().map(to_row).collect();
    if rows.is_empty() {
        return None;
    }

    // 量尺寸。
    let font = Font::system(14.0);
    let measure_surface = ImageSurface::create(Format::ARgb32, 4, 4).ok()?;
    let mcv = CairoCanvas::new(&measure_surface, 1.0);
    let mut content_w = 60.0f32;
    let mut extra_w = 0.0f32;
    for r in &rows {
        if let Row::Item { text, shortcut, .. } = r {
            content_w = content_w.max(mcv.measure_text(text, &font).width);
            if let Some(sc) = shortcut {
                extra_w = extra_w.max(mcv.measure_text(sc, &font).width);
            }
        } else if let Row::Sub { text, .. } = r {
            content_w = content_w.max(mcv.measure_text(text, &font).width);
            extra_w = extra_w.max(12.0); // ▶
        }
    }
    drop(mcv);
    let width = PAD_X * 2.0 + content_w + if extra_w > 0.0 { GAP + extra_w } else { 0.0 };
    let height: f32 = rows
        .iter()
        .map(|r| if matches!(r, Row::Sep) { SEP_H } else { ITEM_H })
        .sum::<f32>()
        + 2.0;

    let pw = (width * scale).ceil().max(1.0) as u16;
    let ph = (height * scale).ceil().max(1.0) as u16;

    // 建 override-redirect 弹窗。
    let root = conn.setup().roots.first()?.root;
    let win = conn.generate_id().ok()?;
    let gc = conn.generate_id().ok()?;
    let aux = CreateWindowAux::new().override_redirect(1).event_mask(
        EventMask::EXPOSURE
            | EventMask::BUTTON_PRESS
            | EventMask::BUTTON_RELEASE
            | EventMask::POINTER_MOTION
            | EventMask::KEY_PRESS
            | EventMask::LEAVE_WINDOW,
    );
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        root,
        sx,
        sy,
        pw,
        ph,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &aux,
    )
    .ok()?;
    conn.create_gc(gc, win, &CreateGCAux::new()).ok()?;
    let depth = conn.setup().roots.first()?.root_depth;
    conn.map_window(win).ok()?;
    let _ = conn.flush();

    // 抓取指针 + 键盘，独占交互。
    let _ = conn.grab_pointer(
        true,
        win,
        EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
        GrabMode::ASYNC,
        GrabMode::ASYNC,
        0u32,
        0u32,
        0u32,
    );
    let _ = conn.grab_keyboard(true, win, 0u32, GrabMode::ASYNC, GrabMode::ASYNC);
    let _ = conn.flush();

    let mut hovered: Option<usize> = None;
    render_menu(
        conn, win, gc, depth, &rows, &font, width, height, scale, hovered,
    );

    let result;
    loop {
        let Ok(ev) = conn.wait_for_event() else {
            result = None;
            break;
        };
        match ev {
            XEvent::Expose(_) => {
                render_menu(
                    conn, win, gc, depth, &rows, &font, width, height, scale, hovered,
                );
            }
            XEvent::MotionNotify(e) => {
                let idx = row_at(&rows, e.event_y as f32 / scale);
                if idx != hovered {
                    hovered = idx;
                    render_menu(
                        conn, win, gc, depth, &rows, &font, width, height, scale, hovered,
                    );
                }
            }
            XEvent::ButtonRelease(e) => {
                // 落在弹窗外 → 取消。
                if e.event_x < 0
                    || e.event_y < 0
                    || e.event_x as u16 >= pw
                    || e.event_y as u16 >= ph
                {
                    result = None;
                    break;
                }
                if let Some(i) = row_at(&rows, e.event_y as f32 / scale) {
                    match &rows[i] {
                        Row::Item { id, enabled, .. } if *enabled => {
                            result = Some(id.clone());
                            break;
                        }
                        Row::Sub { items, enabled, .. } if *enabled => {
                            // 子菜单在右侧展开。
                            let (cx, cy) = (sx + pw as i16, sy + row_top(&rows, i, scale) as i16);
                            close(conn, win, gc);
                            return popup_at(conn, cx, cy, items, scale);
                        }
                        _ => {}
                    }
                }
            }
            XEvent::ButtonPress(_) => {}
            XEvent::KeyPress(e) => {
                // keycode 直接判断常见键（避免再查映射）：Esc=9,Up=111,Down=116,Enter=36。
                match e.detail {
                    9 => {
                        result = None;
                        break;
                    }
                    36 => {
                        if let Some(i) = hovered {
                            if let Row::Item { id, enabled, .. } = &rows[i] {
                                if *enabled {
                                    result = Some(id.clone());
                                    break;
                                }
                            }
                        }
                    }
                    111 => {
                        hovered = step(&rows, hovered, -1);
                        render_menu(
                            conn, win, gc, depth, &rows, &font, width, height, scale, hovered,
                        );
                    }
                    116 => {
                        hovered = step(&rows, hovered, 1);
                        render_menu(
                            conn, win, gc, depth, &rows, &font, width, height, scale, hovered,
                        );
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    close(conn, win, gc);
    result
}

fn close(conn: &RustConnection, win: Window, gc: u32) {
    let _ = conn.ungrab_pointer(0u32);
    let _ = conn.ungrab_keyboard(0u32);
    let _ = conn.destroy_window(win);
    let _ = conn.free_gc(gc);
    let _ = conn.flush();
}

fn to_row(entry: &NativeMenuEntry) -> Row {
    match entry {
        NativeMenuEntry::Item(it) => Row::Item {
            id: it.id.clone(),
            text: it.text.clone(),
            shortcut: it.shortcut.clone(),
            enabled: it.enabled,
            checked: it.checked,
        },
        NativeMenuEntry::Separator => Row::Sep,
        NativeMenuEntry::Submenu(s) => Row::Sub {
            text: s.text.clone(),
            enabled: s.enabled,
            items: s.items.clone(),
        },
    }
}

/// y(逻辑) → 行索引（分隔线不可命中）。
fn row_at(rows: &[Row], y: f32) -> Option<usize> {
    let mut top = 1.0;
    for (i, r) in rows.iter().enumerate() {
        let h = if matches!(r, Row::Sep) { SEP_H } else { ITEM_H };
        if y >= top && y < top + h && !matches!(r, Row::Sep) {
            return Some(i);
        }
        top += h;
    }
    None
}

fn row_top(rows: &[Row], idx: usize, _scale: f32) -> f32 {
    let mut top = 1.0;
    for r in rows.iter().take(idx) {
        top += if matches!(r, Row::Sep) { SEP_H } else { ITEM_H };
    }
    top
}

/// 键盘上下移动到下一个可命中项。
fn step(rows: &[Row], cur: Option<usize>, dir: i32) -> Option<usize> {
    let n = rows.len() as i32;
    let mut i = cur
        .map(|c| c as i32)
        .unwrap_or(if dir > 0 { -1 } else { n });
    for _ in 0..n {
        i = (i + dir).rem_euclid(n);
        if matches!(
            rows[i as usize],
            Row::Item { enabled: true, .. } | Row::Sub { enabled: true, .. }
        ) {
            return Some(i as usize);
        }
    }
    cur
}

#[allow(clippy::too_many_arguments)]
fn render_menu(
    conn: &RustConnection,
    win: Window,
    gc: u32,
    depth: u8,
    rows: &[Row],
    font: &Font,
    width: f32,
    height: f32,
    scale: f32,
    hovered: Option<usize>,
) {
    let pw = (width * scale).ceil().max(1.0) as i32;
    let ph = (height * scale).ceil().max(1.0) as i32;
    let Ok(mut surface) = ImageSurface::create(Format::ARgb32, pw, ph) else {
        return;
    };
    {
        let mut cv = CairoCanvas::new(&surface, scale);
        cv.fill_rect(Rect::new(0.0, 0.0, width, height), BG);
        cv.stroke_rect(Rect::new(0.5, 0.5, width - 1.0, height - 1.0), BORDER, 1.0);
        let mut y = 1.0f32;
        for (i, r) in rows.iter().enumerate() {
            match r {
                Row::Sep => {
                    cv.fill_rect(
                        Rect::new(PAD_X, y + SEP_H / 2.0, width - PAD_X * 2.0, 1.0),
                        BORDER,
                    );
                    y += SEP_H;
                }
                Row::Item {
                    text,
                    shortcut,
                    enabled,
                    checked,
                    ..
                } => {
                    let hot = hovered == Some(i);
                    if hot {
                        cv.fill_rect(Rect::new(1.0, y, width - 2.0, ITEM_H), HOVER);
                    }
                    let col = if hot {
                        HOVER_TEXT
                    } else if *enabled {
                        TEXT
                    } else {
                        TEXT_DIM
                    };
                    let tr = Rect::new(PAD_X, y, width - PAD_X * 2.0, ITEM_H);
                    let label = if *checked {
                        format!("✓ {text}")
                    } else {
                        text.clone()
                    };
                    draw_line(&mut cv, &label, tr, font, col, TextAlign::Left);
                    if let Some(sc) = shortcut {
                        draw_line(
                            &mut cv,
                            sc,
                            tr,
                            font,
                            if hot { HOVER_TEXT } else { TEXT_DIM },
                            TextAlign::Right,
                        );
                    }
                    y += ITEM_H;
                }
                Row::Sub { text, enabled, .. } => {
                    let hot = hovered == Some(i);
                    if hot {
                        cv.fill_rect(Rect::new(1.0, y, width - 2.0, ITEM_H), HOVER);
                    }
                    let col = if hot {
                        HOVER_TEXT
                    } else if *enabled {
                        TEXT
                    } else {
                        TEXT_DIM
                    };
                    let tr = Rect::new(PAD_X, y, width - PAD_X * 2.0, ITEM_H);
                    draw_line(&mut cv, text, tr, font, col, TextAlign::Left);
                    draw_line(&mut cv, "▶", tr, font, col, TextAlign::Right);
                    y += ITEM_H;
                }
            }
        }
    }
    surface.flush();
    let stride = surface.stride();
    if stride != pw * 4 {
        return;
    }
    let bytes = match surface.data() {
        Ok(data) => data.to_vec(),
        Err(_) => return,
    };
    let _ = conn.put_image(
        ImageFormat::Z_PIXMAP,
        win,
        gc,
        pw as u16,
        ph as u16,
        0,
        0,
        0,
        depth,
        &bytes,
    );
    let _ = conn.flush();
}

/// 在 rect 内按对齐画一行文字（垂直居中）。
fn draw_line(
    cv: &mut CairoCanvas,
    text: &str,
    rect: Rect,
    font: &Font,
    color: Color,
    align: TextAlign,
) {
    let sz = cv.measure_text(text, font);
    let x = match align {
        TextAlign::Left => rect.left(),
        TextAlign::Right => rect.right() - sz.width,
        TextAlign::Center => rect.left() + (rect.size.width - sz.width) / 2.0,
    };
    let y = rect.top() + (rect.size.height - sz.height) / 2.0;
    cv.draw_text(text, flexui_gfx::Point::new(x, y), font, color);
}
