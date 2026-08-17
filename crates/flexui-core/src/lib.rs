//! flexui-core：平台无关的 UI 核心（L3/L4）。
//!
//! 提供事件模型、控件状态机与分状态样式、控件树、Flex 布局、统一绘制管线、
//! 事件分发。不含任何平台代码，可脱离窗口做单元测试。

pub mod anim;
pub mod dialog;
pub mod dispatch;
pub mod event;
pub mod frame_animation;
pub mod layout;
pub mod localization;
pub mod native_menu;
pub mod paint;
pub mod scroll;
pub mod sizing;
pub mod style;
pub mod theme;
pub mod widget;
pub mod widgets;
pub mod window;

// 常用类型再导出，方便上层与后端使用。
pub use anim::{AnimProp, Easing};
pub use dialog::{DialogKind, FileDialog, FileFilter};
pub use dispatch::{
    hit_test, point_wants_text_cursor, Dispatcher, EventCtx, Invalidation, MainProxy, UiTask,
};
pub use event::{ControlEvent, Event, EventFlow, Mods, MouseButton, WindowEvent};
pub use frame_animation::{FrameAnimation, FrameFinish, FrameLayer, FramePlayback};
pub use layout::{layout_node, Axis};
pub use localization::{apply_localizations, LocalizationBinding};
pub use native_menu::{
    NativeMenu, NativeMenuAnchor, NativeMenuEntry, NativeMenuItem, NativeMenuPopupAnchor,
    NativeSubmenu,
};
pub use paint::{paint_tree, paint_tree_in_rect};
pub use scroll::{
    apply_thumb_drag, paint_scrollbars, scrollbar_region_contains, thumb_grab, thumb_h_rect,
    thumb_v, ScrollAxes, ScrollAxis, ScrollBarStyle, ScrollBarVisibility, ScrollGrab, ScrollState,
};
pub use sizing::{Align, Justify, Sizing};
pub use style::{
    BaseState, Gradient, PlaceholderStyleSet, PlaceholderStyleSpec, Shadow, StyleSet, StyleSpec,
    VisualState,
};
pub use theme::{
    apply_theme, refresh_theme, Theme, ThemeColorBinding, ThemeColorProperty, ThemeMode,
    ThemePalette, WidgetKind,
};
pub use widget::{
    find_by_id, find_by_name, find_mut_by_id, find_mut_by_name, visit_all_mut, Base, Clickable,
    Container, HitPolicy, Node, TextControl, TextInputState, Widget, WidgetId, WidgetProperty,
    WidgetPropertyKey, WidgetRole,
};
pub use widgets::{
    build_menu, build_menu_entries, build_menu_labels, build_menu_styled, build_tooltip, Button,
    CheckBox, ComboBox, Edit, EditConfig, HBox, Image, Label, ListView, MenuAlignment, MenuEntry,
    MenuItem, MenuStyle, Panel, Progress, Radio, ScrollView, Separator, Slider, Switch, TabBox,
    VBox, VirtualColumn, VirtualList, VirtualListRow, VirtualListRows, VirtualListSource,
    VirtualListSourceRef, VirtualSelectionMode, VirtualSort, VirtualSortDirection,
};
pub use window::{
    AnimRequest, NewWindow, NoopDelegate, OverlayRequest, TitlebarMode, WindowConfig, WindowCtx,
    WindowDelegate, WindowDragRegion, WindowHandle, WindowPresentation,
};

// 几何/绘图类型透传，便于上层一次性引入。
pub use flexui_gfx::{Color, Corners, Insets, Point, Rect, Size};
pub use flexui_gfx::{image_density_from_path, Canvas, Font, ImageFit, ImageSource, TextAlign};
pub use flexui_i18n::{
    I18nError, LocalizationValue, LocalizedStringKey, LocalizedStringResource, Localizer,
};
