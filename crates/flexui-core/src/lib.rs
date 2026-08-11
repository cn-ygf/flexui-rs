//! flexui-core：平台无关的 UI 核心（L3/L4）。
//!
//! 提供事件模型、控件状态机与分状态样式、控件树、Flex 布局、统一绘制管线、
//! 事件分发。不含任何平台代码，可脱离窗口做单元测试。

pub mod anim;
pub mod dialog;
pub mod dispatch;
pub mod event;
pub mod layout;
pub mod paint;
pub mod sizing;
pub mod style;
pub mod widget;
pub mod widgets;
pub mod window;

// 常用类型再导出，方便上层与后端使用。
pub use dispatch::{hit_test, Dispatcher, EventCtx, MainProxy};
pub use anim::{AnimProp, Easing};
pub use dialog::{DialogKind, FileDialog, FileFilter};
pub use event::{Event, EventFlow, Mods, MouseButton};
pub use layout::{layout_node, Axis};
pub use paint::paint_tree;
pub use sizing::{Align, Justify, Sizing};
pub use style::{BaseState, StyleSet, StyleSpec, VisualState};
pub use widget::{
    find_by_name, find_mut_by_id, find_mut_by_name, visit_all_mut, Base, Clickable, Container,
    HitPolicy, Node, TextControl, Widget, WidgetId, WidgetRole,
};
pub use window::{
    AnimRequest, NoopDelegate, OverlayRequest, TitlebarMode, WindowConfig, WindowCtx,
    WindowDelegate, WindowHandle,
};
pub use widgets::{
    build_menu, build_menu_labels, build_tooltip, Button, CheckBox, ComboBox, Edit, HBox, Image,
    Label, ListView, MenuItem, Panel, Progress, Radio, ScrollView, Separator, Slider, TabBox, VBox,
};

// 几何/绘图类型透传，便于上层一次性引入。
pub use flexui_geometry::{Color, Corners, Insets, Point, Rect, Size};
pub use flexui_gfx::{Canvas, Font, ImageFit, ImageSource, TextAlign};
