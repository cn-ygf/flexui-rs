//! 平台无关的统一事件模型（L3）。
//!
//! 各平台后端把原生事件（NSEvent / WndProc 消息）翻译成这里的 `Event`，
//! 再交给 `Dispatcher` 做命中测试与分发。坐标一律为「逻辑像素、左上原点」。

use flexui_geometry::Point;

/// 鼠标按键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// 统一事件枚举（一期覆盖鼠标 + 键盘 + 窗口基础事件）。
#[derive(Debug, Clone)]
pub enum Event {
    /// 鼠标移动到某点（用于 hover / Hot 状态）。
    MouseMove { pos: Point },
    /// 鼠标按下。
    MouseDown { pos: Point, button: MouseButton },
    /// 鼠标抬起。
    MouseUp { pos: Point, button: MouseButton },
    /// 滚轮滚动（dx/dy 为逻辑像素增量）。
    MouseWheel { pos: Point, dx: f32, dy: f32 },
    /// 键按下（key 为平台无关键码，一期用 u32 透传）。
    KeyDown { key: u32 },
    /// 键抬起。
    KeyUp { key: u32 },
    /// 字符输入（已由输入法/键盘布局翻译，用于文本框）。
    Char { ch: char },
    /// 窗口尺寸改变（逻辑像素）。
    WindowResized { width: f32, height: f32 },
    /// 缩放因子改变（HiDPI）。
    ScaleChanged { scale: f32 },
}

/// 平台无关的按键码常量（后端把各自的原始键码映射到这些值再发出）。
pub mod keys {
    pub const BACKSPACE: u32 = 8;
    pub const TAB: u32 = 9;
    pub const ENTER: u32 = 13;
    pub const ESCAPE: u32 = 27;
    pub const DELETE: u32 = 127;
    pub const LEFT: u32 = 0x1000;
    pub const RIGHT: u32 = 0x1001;
    pub const HOME: u32 = 0x1002;
    pub const END: u32 = 0x1003;
    pub const UP: u32 = 0x1004;
    pub const DOWN: u32 = 0x1005;
}

/// 事件处理后的传播控制。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventFlow {
    /// 事件已被消费，停止继续传播。
    Consumed,
    /// 未消费，继续向下/兄弟传播。
    Ignored,
}
