//! 基础与扩展控件（L4）：Label / Button / Image / CheckBox / Radio / Edit。
//! 对应需求 C7（基础控件）、C8（Radio+group）。

use flexui_geometry::{Color, Corners, Rect, Size};
use flexui_gfx::{Canvas, ImageSource, TextAlign};

use crate::common_builders;
use crate::event::{Event, EventFlow};
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::style::StyleSpec;
use crate::widget::{Base, Clickable, TextControl, Widget, WidgetRole};

// ————————————————————————————————————————————————— Label
/// 文本标签（不可交互）。
pub struct Label {
    base: Base,
}

impl Label {
    pub fn new(text: impl Into<String>) -> Self {
        let mut base = Base::new(WidgetRole::Plain);
        base.text = text.into();
        Self { base }
    }
}

impl Widget for Label {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        let s = cv.measure_text(&self.base.text, &self.base.font);
        layout::size_from_content(&self.base, s.width, s.height)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let color = style.fg_color.unwrap_or(Color::BLACK);
        let align = style.text_align.unwrap_or(TextAlign::Left);
        draw_aligned_text(
            cv,
            &self.base.text,
            layout::content_rect(&self.base),
            &self.base.font,
            color,
            align,
        );
    }
}

common_builders!(Label);

// ————————————————————————————————————————————————— Button
/// 按钮：完整 4×2 状态，点击触发回调。
pub struct Button {
    base: Base,
}

impl Button {
    pub fn new(text: impl Into<String>) -> Self {
        let mut base = Base::new(WidgetRole::Button);
        base.text = text.into();
        Self { base }
    }
}

impl Widget for Button {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        let s = cv.measure_text(&self.base.text, &self.base.font);
        // 文字 + 默认左右各 12 的内边距（若已设 padding 则叠加）。
        layout::size_from_content(&self.base, s.width + 24.0, s.height + 12.0)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let color = style.fg_color.unwrap_or(Color::WHITE);
        let align = style.text_align.unwrap_or(TextAlign::Center);
        draw_aligned_text(
            cv,
            &self.base.text,
            layout::content_rect(&self.base),
            &self.base.font,
            color,
            align,
        );
    }
}

common_builders!(Button);

// ————————————————————————————————————————————————— Image
/// 图片控件。
pub struct Image {
    base: Base,
    source: ImageSource,
}

impl Image {
    pub fn new(source: ImageSource) -> Self {
        Self {
            base: Base::new(WidgetRole::Plain),
            source,
        }
    }
    pub fn path(p: impl Into<String>) -> Self {
        Self::new(ImageSource::path(p))
    }
}

impl Widget for Image {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, _cv: &dyn Canvas) -> Size {
        // 图片无内在尺寸信息时用显式尺寸，缺省给一个占位方块。
        layout::size_from_content(&self.base, 32.0, 32.0)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, _style: &StyleSpec) {
        cv.draw_image(&self.source, layout::content_rect(&self.base));
    }
}

common_builders!(Image);

// ————————————————————————————————————————————————— CheckBox
/// 勾选框：点击切换 selected。
pub struct CheckBox {
    base: Base,
}

impl CheckBox {
    pub fn new(text: impl Into<String>) -> Self {
        let mut base = Base::new(WidgetRole::CheckBox);
        base.text = text.into();
        Self { base }
    }
    pub fn checked(mut self, v: bool) -> Self {
        self.base.selected = v;
        self
    }
}

impl Widget for CheckBox {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        let s = cv.measure_text(&self.base.text, &self.base.font);
        layout::size_from_content(&self.base, s.width + 26.0, s.height.max(18.0))
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        paint_indicator_and_text(&self.base, cv, style, false);
    }
}

common_builders!(CheckBox);

// ————————————————————————————————————————————————— Radio
/// 单选：同 group 互斥（互斥逻辑在分发器实现），可绑定 tab_index 驱动 TabBox。
pub struct Radio {
    base: Base,
}

impl Radio {
    pub fn new(text: impl Into<String>) -> Self {
        let mut base = Base::new(WidgetRole::Radio);
        base.text = text.into();
        Self { base }
    }
    /// 所属分组。
    pub fn group(mut self, g: u32) -> Self {
        self.base.group = Some(g);
        self
    }
    /// 关联的 TabBox 页索引（组成 tabbar）。
    pub fn tab_index(mut self, i: usize) -> Self {
        self.base.tab_index = Some(i);
        self
    }
    pub fn selected(mut self, v: bool) -> Self {
        self.base.selected = v;
        self
    }
}

impl Widget for Radio {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        let s = cv.measure_text(&self.base.text, &self.base.font);
        layout::size_from_content(&self.base, s.width + 26.0, s.height.max(18.0))
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        paint_indicator_and_text(&self.base, cv, style, true);
    }
}

common_builders!(Radio);

// ————————————————————————————————————————————————— Edit
/// 单行文本输入（基础版：接收 Char 追加、Backspace 删除；focus 时显示光标）。
pub struct Edit {
    base: Base,
}

impl Edit {
    pub fn new() -> Self {
        Self {
            base: Base::new(WidgetRole::Edit),
        }
    }
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.base.text = t.into();
        self
    }
}

impl Default for Edit {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Edit {
    fn base(&self) -> &Base {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }
    fn measure(&mut self, _avail: Size, cv: &dyn Canvas) -> Size {
        let s = cv.measure_text("Ag", &self.base.font);
        layout::size_from_content(&self.base, 120.0, s.height + 8.0)
    }
    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = layout::content_rect(&self.base);
        let color = style.fg_color.unwrap_or(Color::BLACK);
        draw_aligned_text(cv, &self.base.text, content, &self.base.font, color, TextAlign::Left);
        // focus 时在文字末尾画一根光标。
        if self.base.focused {
            let tw = cv.measure_text(&self.base.text, &self.base.font).width;
            let cx = content.left() + tw + 1.0;
            cv.fill_rect(
                Rect::new(cx, content.top() + 2.0, 1.5, content.size.height - 4.0),
                color,
            );
        }
    }
    fn on_event(&mut self, ev: &Event) -> EventFlow {
        match ev {
            Event::Char { ch } if !ch.is_control() => {
                self.base.text.push(*ch);
                EventFlow::Consumed
            }
            // 退格键（macOS/Win 退格常见键码 8）。
            Event::KeyDown { key } if *key == 8 => {
                self.base.text.pop();
                EventFlow::Consumed
            }
            _ => EventFlow::Ignored,
        }
    }
}

common_builders!(Edit);

// —— 能力 trait 归类（继承视图）——
impl TextControl for Label {}
impl TextControl for Button {}
impl Clickable for Button {}
impl TextControl for Edit {}
impl TextControl for CheckBox {}
impl Clickable for CheckBox {}
impl TextControl for Radio {}
impl Clickable for Radio {}

/// 公共：绘制左侧指示器（方框/圆点）+ 右侧文字。`circular=true` 画单选圆点。
fn paint_indicator_and_text(base: &Base, cv: &mut dyn Canvas, style: &StyleSpec, circular: bool) {
    let content = layout::content_rect(base);
    let box_size = 16.0;
    let bx = content.left();
    let by = content.top() + (content.size.height - box_size) / 2.0;
    let ind = Rect::new(bx, by, box_size, box_size);
    let border = style.border_color.unwrap_or(Color::from_u8(140, 150, 170, 255));
    let radius = if circular {
        Corners::all(box_size / 2.0)
    } else {
        Corners::all(3.0)
    };
    // 指示器外框
    cv.stroke_round_rect(ind, radius, border, 1.5);
    // 选中：填充内部
    if base.selected {
        let fill = style.fg_color.unwrap_or(Color::from_u8(52, 120, 246, 255));
        let inner = ind.inset_all(4.0);
        let inner_radius = if circular {
            Corners::all((box_size - 8.0) / 2.0)
        } else {
            Corners::all(2.0)
        };
        cv.fill_round_rect(inner, inner_radius, fill);
    }
    // 文字
    let text_rect = Rect::new(
        content.left() + box_size + 8.0,
        content.top(),
        (content.size.width - box_size - 8.0).max(0.0),
        content.size.height,
    );
    let color = style.fg_color.unwrap_or(Color::from_u8(230, 235, 245, 255));
    draw_aligned_text(cv, &base.text, text_rect, &base.font, color, TextAlign::Left);
}
