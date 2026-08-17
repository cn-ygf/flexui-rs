//! VirtualList：面向大数据集的虚拟化表格列表。
//!
//! 数据源只在绘制可见区域时按需提供单元格文本，不会为每一行创建控件或排版对象。
//! 控件提供固定表头、双轴滚动、稳定行 ID 选择、排序、列宽拖动和键盘导航。

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use flexui_gfx::{Color, Point, Rect, Size};
use flexui_gfx::{Canvas, Font, TextAlign, TextLayout};

use crate::anim::AnimProp;
use crate::common_builders;
use crate::event::{keys, Event, EventFlow, MouseButton};
use crate::layout;
use crate::paint::elide_to_width;
use crate::scroll::{paint_scrollbars, ScrollAxes, ScrollBarStyle, ScrollState};
use crate::style::StyleSpec;
use crate::theme::WidgetKind;
use crate::widget::{Base, Widget, WidgetProperty, WidgetPropertyKey, WidgetRole};

const DEFAULT_ROW_HEIGHT: f32 = 36.0;
const DEFAULT_HEADER_HEIGHT: f32 = 40.0;
const CELL_PADDING: f32 = 10.0;
const RESIZE_HIT_WIDTH: f32 = 5.0;
const CELL_CACHE_CAPACITY: usize = 4096;
const MARQUEE_DRAG_THRESHOLD: f32 = 4.0;
const MARQUEE_EDGE_SCROLL_ZONE: f32 = 24.0;
const MARQUEE_EDGE_SCROLL_STEP: f32 = 18.0;
const MARQUEE_DASH_LENGTH: f32 = 4.0;
const MARQUEE_DASH_GAP: f32 = 3.0;

mod model;

pub use model::{
    VirtualColumn, VirtualListRow, VirtualListRows, VirtualListSource, VirtualListSourceRef,
    VirtualSelectionMode, VirtualSort, VirtualSortDirection,
};

#[derive(Debug, Clone, Copy)]
struct ColumnResize {
    index: usize,
    start_x: f32,
    start_width: f32,
}

#[derive(Debug, Clone)]
struct MarqueeSelection {
    start: Point,
    current: Point,
    origin_content: Point,
    base_selection: BTreeSet<u64>,
    extend: bool,
    toggle: bool,
    active: bool,
}

#[derive(Clone, Eq)]
struct CellCacheKey {
    row_id: u64,
    column: String,
    width_px: i32,
}

impl PartialEq for CellCacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.row_id == other.row_id
            && self.column == other.column
            && self.width_px == other.width_px
    }
}

impl Hash for CellCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.row_id.hash(state);
        self.column.hash(state);
        self.width_px.hash(state);
    }
}

#[derive(Default)]
struct CellLayoutCache {
    layouts: HashMap<CellCacheKey, TextLayout>,
}

impl CellLayoutCache {
    fn clear(&mut self) {
        self.layouts.clear();
    }

    fn get(&self, key: &CellCacheKey) -> Option<TextLayout> {
        self.layouts.get(key).cloned()
    }

    fn insert(&mut self, key: CellCacheKey, layout: TextLayout) {
        if self.layouts.len() >= CELL_CACHE_CAPACITY {
            self.layouts.clear();
        }
        self.layouts.insert(key, layout);
    }
}

/// 大数据虚拟表格列表。
pub struct VirtualList {
    base: Base,
    columns: Vec<VirtualColumn>,
    source: VirtualListSourceRef,
    row_height: f32,
    header_height: f32,
    show_header: bool,
    striped: bool,
    fill_last_column: bool,
    overscan: usize,
    selection_mode: VirtualSelectionMode,
    selected_rows: BTreeSet<u64>,
    active_index: Option<usize>,
    selection_anchor: Option<usize>,
    sort: Option<VirtualSort>,
    scroll: ScrollState,
    scrollbar: ScrollBarStyle,
    resize: Option<ColumnResize>,
    marquee: Option<MarqueeSelection>,
    source_revision: Cell<u64>,
    cache_font: Option<Font>,
    cell_cache: RefCell<CellLayoutCache>,
}

impl VirtualList {
    pub fn new() -> Self {
        let source: VirtualListSourceRef = Rc::new(VirtualListRows::new());
        let mut base = Base::new_kind(WidgetRole::ListView, WidgetKind::VirtualList);
        base.focusable = true;
        Self {
            base,
            columns: Vec::new(),
            source,
            row_height: DEFAULT_ROW_HEIGHT,
            header_height: DEFAULT_HEADER_HEIGHT,
            show_header: true,
            striped: true,
            fill_last_column: true,
            overscan: 3,
            selection_mode: VirtualSelectionMode::Single,
            selected_rows: BTreeSet::new(),
            active_index: None,
            selection_anchor: None,
            sort: None,
            scroll: ScrollState::new(ScrollAxes::both()),
            scrollbar: ScrollBarStyle::default(),
            resize: None,
            marquee: None,
            source_revision: Cell::new(0),
            cache_font: None,
            cell_cache: RefCell::new(CellLayoutCache::default()),
        }
    }

    pub fn columns(mut self, columns: Vec<VirtualColumn>) -> Self {
        self.columns = columns;
        self
    }

    pub fn source(mut self, source: VirtualListSourceRef) -> Self {
        self.source_revision.set(source.revision());
        self.source = source;
        self
    }

    pub fn row_height(mut self, value: f32) -> Self {
        self.row_height = value.max(16.0);
        self
    }

    pub fn header_height(mut self, value: f32) -> Self {
        self.header_height = value.max(16.0);
        self
    }

    pub fn show_header(mut self, value: bool) -> Self {
        self.show_header = value;
        self
    }

    pub fn striped(mut self, value: bool) -> Self {
        self.striped = value;
        self
    }

    pub fn fill_last_column(mut self, value: bool) -> Self {
        self.fill_last_column = value;
        self
    }

    pub fn overscan(mut self, rows: usize) -> Self {
        self.overscan = rows.min(100);
        self
    }

    pub fn selection_mode(mut self, mode: VirtualSelectionMode) -> Self {
        self.selection_mode = mode;
        self
    }

    pub fn scrollbar(mut self, visibility: crate::scroll::ScrollBarVisibility) -> Self {
        self.scroll.set_visibility(visibility);
        self
    }

    pub fn column_definitions(&self) -> &[VirtualColumn] {
        &self.columns
    }

    pub fn selected_row_ids(&self) -> Vec<u64> {
        self.selected_rows.iter().copied().collect()
    }

    pub fn sort(&self) -> Option<&VirtualSort> {
        self.sort.as_ref()
    }

    fn content_rect(&self) -> Rect {
        layout::content_rect(&self.base)
    }

    fn body_rect(&self) -> Rect {
        let content = self.content_rect();
        let header = if self.show_header {
            self.header_height.min(content.size.height)
        } else {
            0.0
        };
        Rect::new(
            content.left(),
            content.top() + header,
            content.size.width,
            (content.size.height - header).max(0.0),
        )
    }

    fn scrollbar_viewport(&self) -> Rect {
        let body = self.body_rect();
        Rect::new(
            body.left(),
            body.top(),
            (self.base.rect.right() - body.left()).max(0.0),
            body.size.height,
        )
    }

    fn resolved_widths(&self, viewport_width: f32) -> Vec<f32> {
        let mut widths = self
            .columns
            .iter()
            .map(VirtualColumn::clamped_width)
            .collect::<Vec<_>>();
        let total = widths.iter().sum::<f32>();
        let remaining = (viewport_width - total).max(0.0);
        if remaining <= 0.0 || widths.is_empty() {
            return widths;
        }
        let flex_total = self.columns.iter().map(|column| column.flex).sum::<f32>();
        if flex_total > 0.0 {
            for (index, column) in self.columns.iter().enumerate() {
                if column.flex > 0.0 {
                    widths[index] = (widths[index] + remaining * column.flex / flex_total)
                        .min(column.max_width.max(column.min_width));
                }
            }
        } else if self.fill_last_column {
            let last = widths.len() - 1;
            widths[last] = (widths[last] + remaining).min(
                self.columns[last]
                    .max_width
                    .max(self.columns[last].min_width),
            );
        }
        widths
    }

    fn total_width(&self, viewport_width: f32) -> f32 {
        self.resolved_widths(viewport_width)
            .iter()
            .sum::<f32>()
            .max(viewport_width)
    }

    fn update_metrics(&mut self) {
        let body = self.body_rect();
        let total_height =
            ((self.source.row_count() as f64) * self.row_height as f64).min(f32::MAX as f64) as f32;
        self.scroll.set_metrics(
            Size::new(self.total_width(body.size.width), total_height),
            body.size,
        );
    }

    fn sync_source_revision(&self) {
        let revision = self.source.revision();
        if revision != self.source_revision.get() {
            self.source_revision.set(revision);
            self.cell_cache.borrow_mut().clear();
        }
    }

    fn visible_row_range(&self, body: Rect) -> std::ops::Range<usize> {
        let count = self.source.row_count();
        if count == 0 || body.size.height <= 0.0 {
            return 0..0;
        }
        let offset = self.scroll.offset().y;
        let first = (offset / self.row_height).floor().max(0.0) as usize;
        let first = first.saturating_sub(self.overscan);
        let visible = (body.size.height / self.row_height).ceil() as usize;
        let end = first
            .saturating_add(visible)
            .saturating_add(self.overscan * 2 + 1)
            .min(count);
        first..end
    }

    fn cell_layout(
        &self,
        cv: &dyn Canvas,
        row: usize,
        row_id: u64,
        column: &VirtualColumn,
        width: f32,
    ) -> TextLayout {
        let key = CellCacheKey {
            row_id,
            column: column.key.clone(),
            width_px: (width.max(0.0) * cv.scale()).round() as i32,
        };
        if let Some(layout) = self.cell_cache.borrow().get(&key) {
            return layout;
        }
        let text = self.source.cell_text(row, &column.key);
        let shown = elide_to_width(cv, &text, &self.base.font, width.max(0.0));
        let layout = cv.layout_text(&shown, &self.base.font);
        self.cell_cache.borrow_mut().insert(key, layout.clone());
        layout
    }

    fn draw_layout(
        cv: &mut dyn Canvas,
        layout: &TextLayout,
        rect: Rect,
        align: TextAlign,
        color: Color,
    ) {
        let x = match align {
            TextAlign::Left => rect.left(),
            TextAlign::Center => rect.left() + (rect.size.width - layout.width()) / 2.0,
            TextAlign::Right => rect.right() - layout.width(),
        };
        let y = rect.top() + (rect.size.height - layout.height()) / 2.0;
        cv.draw_text_layout(
            layout,
            Point::new(x.max(rect.left()), y.max(rect.top())),
            color,
        );
    }

    fn column_at(&self, x: f32, widths: &[f32]) -> Option<usize> {
        let local = x - self.content_rect().left() + self.scroll.offset().x;
        let mut cursor = 0.0;
        for (index, width) in widths.iter().enumerate() {
            if local >= cursor && local < cursor + width {
                return Some(index);
            }
            cursor += width;
        }
        None
    }

    fn resize_boundary_at(&self, x: f32, widths: &[f32]) -> Option<usize> {
        let local = x - self.content_rect().left() + self.scroll.offset().x;
        let mut boundary = 0.0;
        for (index, width) in widths.iter().enumerate() {
            boundary += width;
            if self.columns[index].resizable && (local - boundary).abs() <= RESIZE_HIT_WIDTH {
                return Some(index);
            }
        }
        None
    }

    fn row_at(&self, y: f32) -> Option<usize> {
        let body = self.body_rect();
        if !body.contains(Point::new(body.left(), y)) {
            return None;
        }
        let local = y - body.top() + self.scroll.offset().y;
        let row = (local / self.row_height).floor().max(0.0) as usize;
        (row < self.source.row_count()).then_some(row)
    }

    fn begin_marquee(&mut self, pos: Point, extend: bool, toggle: bool) {
        let body = self.body_rect();
        let offset = self.scroll.offset();
        self.marquee = Some(MarqueeSelection {
            start: pos,
            current: pos,
            origin_content: Point::new(
                pos.x - body.left() + offset.x,
                pos.y - body.top() + offset.y,
            ),
            base_selection: self.selected_rows.clone(),
            extend,
            toggle,
            active: false,
        });
    }

    fn update_marquee(&mut self, pos: Point) {
        let Some(mut marquee) = self.marquee.take() else {
            return;
        };
        marquee.current = pos;
        if !marquee.active {
            let dx = pos.x - marquee.start.x;
            let dy = pos.y - marquee.start.y;
            marquee.active = dx * dx + dy * dy >= MARQUEE_DRAG_THRESHOLD.powi(2);
        }
        if !marquee.active {
            self.marquee = Some(marquee);
            return;
        }

        let body = self.body_rect();
        let offset = self.scroll.offset();
        let scroll_delta = if pos.y < body.top() + MARQUEE_EDGE_SCROLL_ZONE {
            -((body.top() + MARQUEE_EDGE_SCROLL_ZONE - pos.y) / MARQUEE_EDGE_SCROLL_ZONE)
                .clamp(0.0, 2.0)
                * MARQUEE_EDGE_SCROLL_STEP
        } else if pos.y > body.bottom() - MARQUEE_EDGE_SCROLL_ZONE {
            ((pos.y - (body.bottom() - MARQUEE_EDGE_SCROLL_ZONE)) / MARQUEE_EDGE_SCROLL_ZONE)
                .clamp(0.0, 2.0)
                * MARQUEE_EDGE_SCROLL_STEP
        } else {
            0.0
        };
        if scroll_delta != 0.0 {
            self.scroll.set_offset(offset.x, offset.y + scroll_delta);
        }

        marquee.current = Point::new(
            pos.x.clamp(body.left(), body.right()),
            pos.y.clamp(body.top(), body.bottom()),
        );
        let current_content_y = marquee.current.y - body.top() + self.scroll.offset().y;
        let top = marquee.origin_content.y.min(current_content_y).max(0.0);
        let bottom = marquee
            .origin_content
            .y
            .max(current_content_y)
            .max(top + 0.001);
        let count = self.source.row_count();
        let first = ((top / self.row_height).floor() as usize).min(count);
        let end = (((bottom - 0.001) / self.row_height).floor() as usize)
            .saturating_add(1)
            .min(count);
        let hits = (first..end)
            .map(|row| self.source.row_id(row))
            .collect::<BTreeSet<_>>();

        self.selected_rows = if marquee.toggle {
            let mut selected = marquee.base_selection.clone();
            for id in hits {
                if !selected.remove(&id) {
                    selected.insert(id);
                }
            }
            selected
        } else if marquee.extend {
            marquee.base_selection.union(&hits).copied().collect()
        } else {
            hits
        };
        if first < end {
            let current_row = ((current_content_y.max(0.0) / self.row_height).floor() as usize)
                .clamp(first, end - 1);
            self.active_index = Some(current_row);
            self.selection_anchor = Some(first);
        }
        self.marquee = Some(marquee);
    }

    fn marquee_rect(&self) -> Option<Rect> {
        let marquee = self.marquee.as_ref().filter(|item| item.active)?;
        let body = self.body_rect();
        let offset = self.scroll.offset();
        let origin = Point::new(
            body.left() + marquee.origin_content.x - offset.x,
            body.top() + marquee.origin_content.y - offset.y,
        );
        let left = origin
            .x
            .min(marquee.current.x)
            .clamp(body.left(), body.right());
        let top = origin
            .y
            .min(marquee.current.y)
            .clamp(body.top(), body.bottom());
        let mut right = origin
            .x
            .max(marquee.current.x)
            .clamp(body.left(), body.right());
        let mut bottom = origin
            .y
            .max(marquee.current.y)
            .clamp(body.top(), body.bottom());
        right = right.max((left + 1.0).min(body.right()));
        bottom = bottom.max((top + 1.0).min(body.bottom()));
        Some(Rect::new(left, top, right - left, bottom - top))
    }

    fn ensure_row_visible(&mut self, row: usize) {
        self.scroll.ensure_visible(
            Rect::new(
                0.0,
                row as f32 * self.row_height,
                self.scroll.viewport().width,
                self.row_height,
            ),
            0.0,
        );
    }

    fn select_row(&mut self, row: usize, extend: bool, toggle: bool) -> bool {
        if row >= self.source.row_count() {
            return false;
        }
        let before = self.selected_rows.clone();
        self.active_index = Some(row);
        match self.selection_mode {
            VirtualSelectionMode::None => self.selected_rows.clear(),
            VirtualSelectionMode::Single => {
                self.selected_rows.clear();
                self.selected_rows.insert(self.source.row_id(row));
                self.selection_anchor = Some(row);
            }
            VirtualSelectionMode::Multiple if extend => {
                let anchor = self.selection_anchor.unwrap_or(row);
                self.selected_rows.clear();
                for index in anchor.min(row)..=anchor.max(row) {
                    self.selected_rows.insert(self.source.row_id(index));
                }
            }
            VirtualSelectionMode::Multiple if toggle => {
                let id = self.source.row_id(row);
                if !self.selected_rows.remove(&id) {
                    self.selected_rows.insert(id);
                }
                self.selection_anchor = Some(row);
            }
            VirtualSelectionMode::Multiple => {
                self.selected_rows.clear();
                self.selected_rows.insert(self.source.row_id(row));
                self.selection_anchor = Some(row);
            }
        }
        self.ensure_row_visible(row);
        before != self.selected_rows
    }

    fn move_active(&mut self, target: usize, extend: bool) {
        if self.source.row_count() == 0 {
            return;
        }
        self.select_row(target.min(self.source.row_count() - 1), extend, false);
    }

    fn toggle_sort(&mut self, column: usize) {
        if !self.columns.get(column).is_some_and(|item| item.sortable) {
            return;
        }
        let key = self.columns[column].key.clone();
        let direction = match self.sort.as_ref() {
            Some(sort)
                if sort.column == key && sort.direction == VirtualSortDirection::Ascending =>
            {
                VirtualSortDirection::Descending
            }
            _ => VirtualSortDirection::Ascending,
        };
        let active_id = self.active_index.map(|row| self.source.row_id(row));
        self.sort = Some(VirtualSort {
            column: key,
            direction,
        });
        if self.source.set_sort(self.sort.as_ref()) {
            self.source_revision.set(self.source.revision());
            self.cell_cache.borrow_mut().clear();
            if let Some(id) = active_id {
                self.active_index = self.source.index_of_row_id(id);
            }
        }
    }

    fn set_columns(&mut self, columns: Vec<VirtualColumn>) {
        self.columns = columns;
        if self
            .sort
            .as_ref()
            .is_some_and(|sort| !self.columns.iter().any(|column| column.key == sort.column))
        {
            self.sort = None;
        }
        self.cell_cache.borrow_mut().clear();
        self.update_metrics();
    }
}

impl Default for VirtualList {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for VirtualList {
    fn base(&self) -> &Base {
        &self.base
    }

    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }

    fn measure(&mut self, _avail: Size, _cv: &dyn Canvas) -> Size {
        let rows = self.source.row_count().min(8) as f32;
        let header = if self.show_header {
            self.header_height
        } else {
            0.0
        };
        layout::size_from_content(&self.base, 480.0, header + rows.max(3.0) * self.row_height)
    }

    fn arrange(&mut self, _content: Rect, _cv: &dyn Canvas) {
        self.sync_source_revision();
        if self.cache_font.as_ref() != Some(&self.base.font) {
            self.cache_font = Some(self.base.font.clone());
            self.cell_cache.borrow_mut().clear();
        }
        self.update_metrics();
        if let Some(index) = self.active_index {
            if index >= self.source.row_count() {
                self.active_index = self.source.row_count().checked_sub(1);
            }
        }
    }

    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        self.sync_source_revision();
        let content = self.content_rect();
        let body = self.body_rect();
        if content.size.width <= 0.0 || content.size.height <= 0.0 {
            return;
        }
        let widths = self.resolved_widths(body.size.width);
        let offset = self.scroll.offset();
        let foreground = style.fg_color.unwrap_or(Color::from_u8(45, 50, 60, 255));
        let background = style.bg_color.unwrap_or(Color::WHITE);
        let border = style
            .border_color
            .unwrap_or(Color::from_u8(220, 224, 232, 255));
        let header_background = mix(background, foreground, 0.055);
        let stripe_background = mix(background, foreground, 0.025);
        let selection = style
            .selection_color
            .unwrap_or(Color::from_u8(72, 120, 235, 42));
        let accent = style
            .accent_color
            .unwrap_or(Color::from_u8(72, 120, 235, 255));

        cv.save();
        cv.clip_rect(content);
        if self.show_header {
            let header = Rect::new(
                content.left(),
                content.top(),
                content.size.width,
                self.header_height.min(content.size.height),
            );
            cv.fill_rect(header, header_background);
            let mut x = content.left() - offset.x;
            for (index, (column, width)) in self.columns.iter().zip(widths.iter()).enumerate() {
                let rect = Rect::new(x, header.top(), *width, header.size.height);
                if rect.right() >= content.left() && rect.left() <= content.right() {
                    let marker = self.sort.as_ref().and_then(|sort| {
                        (sort.column == column.key).then_some(match sort.direction {
                            VirtualSortDirection::Ascending => " ^",
                            VirtualSortDirection::Descending => " v",
                        })
                    });
                    let title = marker
                        .map_or_else(|| column.title.clone(), |m| format!("{}{m}", column.title));
                    let shown = elide_to_width(
                        cv,
                        &title,
                        &self.base.font,
                        (rect.size.width - CELL_PADDING * 2.0).max(0.0),
                    );
                    let layout = cv.layout_text(&shown, &self.base.font);
                    Self::draw_layout(
                        cv,
                        &layout,
                        rect.inset_all(CELL_PADDING),
                        column.align,
                        foreground,
                    );
                    if index + 1 < self.columns.len() {
                        cv.fill_rect(
                            Rect::new(rect.right() - 0.5, rect.top(), 1.0, rect.size.height),
                            border,
                        );
                    }
                }
                x += width;
            }
            cv.fill_rect(
                Rect::new(header.left(), header.bottom() - 1.0, header.size.width, 1.0),
                border,
            );
        }

        cv.save();
        cv.clip_rect(body);
        let rows = self.visible_row_range(body);
        for row in rows {
            let row_id = self.source.row_id(row);
            let y = body.top() + row as f32 * self.row_height - offset.y;
            let row_rect = Rect::new(body.left(), y, body.size.width, self.row_height);
            if self.selected_rows.contains(&row_id) {
                cv.fill_rect(row_rect, selection);
            } else if self.striped && row % 2 == 1 {
                cv.fill_rect(row_rect, stripe_background);
            }
            if self.base.focused && self.active_index == Some(row) {
                cv.fill_rect(
                    Rect::new(row_rect.left(), row_rect.top(), 3.0, row_rect.size.height),
                    accent,
                );
            }

            let mut x = body.left() - offset.x;
            for (index, (column, width)) in self.columns.iter().zip(widths.iter()).enumerate() {
                let cell = Rect::new(x, y, *width, self.row_height);
                if cell.right() >= body.left() && cell.left() <= body.right() {
                    let text_rect = Rect::new(
                        cell.left() + CELL_PADDING,
                        cell.top(),
                        (cell.size.width - CELL_PADDING * 2.0).max(0.0),
                        cell.size.height,
                    );
                    let layout = self.cell_layout(cv, row, row_id, column, text_rect.size.width);
                    Self::draw_layout(cv, &layout, text_rect, column.align, foreground);
                    if index + 1 < self.columns.len() {
                        cv.fill_rect(
                            Rect::new(cell.right() - 0.5, cell.top(), 1.0, cell.size.height),
                            border,
                        );
                    }
                }
                x += width;
            }
            cv.fill_rect(
                Rect::new(
                    row_rect.left(),
                    row_rect.bottom() - 0.5,
                    row_rect.size.width,
                    0.5,
                ),
                border,
            );
        }
        if self.source.row_count() == 0 {
            let text = cv.layout_text("No data", &self.base.font);
            Self::draw_layout(
                cv,
                &text,
                body,
                TextAlign::Center,
                mix(background, foreground, 0.6),
            );
        }
        if let Some(rect) = self.marquee_rect() {
            cv.fill_rect(rect, Color::rgba(accent.r, accent.g, accent.b, 0.08));
            paint_dashed_rect(cv, rect, accent);
        }
        cv.restore();
        cv.restore();

        paint_scrollbars(
            cv,
            self.scrollbar_viewport(),
            &self.scroll,
            &self.scrollbar,
            style,
        );
    }

    fn on_event(&mut self, event: &Event) -> EventFlow {
        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                mods,
            } => {
                let content = self.content_rect();
                let widths = self.resolved_widths(self.body_rect().size.width);
                if self.show_header && pos.y >= content.top() && pos.y < self.body_rect().top() {
                    if let Some(index) = self.resize_boundary_at(pos.x, &widths) {
                        self.resize = Some(ColumnResize {
                            index,
                            start_x: pos.x,
                            start_width: self.columns[index].clamped_width(),
                        });
                    } else if let Some(index) = self.column_at(pos.x, &widths) {
                        self.toggle_sort(index);
                    }
                    return EventFlow::Consumed;
                }
                let body = self.body_rect();
                if body.contains(*pos) {
                    if self.selection_mode == VirtualSelectionMode::Multiple {
                        self.begin_marquee(*pos, mods.shift, mods.command_or_control());
                        if let Some(row) = self.row_at(pos.y) {
                            self.select_row(row, mods.shift, mods.command_or_control());
                        } else if !mods.shift && !mods.command_or_control() {
                            self.selected_rows.clear();
                            self.active_index = None;
                            self.selection_anchor = None;
                        }
                    } else if let Some(row) = self.row_at(pos.y) {
                        self.select_row(row, false, false);
                    }
                    return EventFlow::Consumed;
                }
                EventFlow::Ignored
            }
            Event::MouseMove { pos } => {
                if let Some(resize) = self.resize {
                    let column = &mut self.columns[resize.index];
                    column.width = (resize.start_width + pos.x - resize.start_x)
                        .clamp(column.min_width, column.max_width.max(column.min_width));
                    column.flex = 0.0;
                    self.cell_cache.borrow_mut().clear();
                    self.update_metrics();
                    EventFlow::Consumed
                } else if self.marquee.is_some() {
                    self.update_marquee(*pos);
                    EventFlow::Consumed
                } else {
                    EventFlow::Ignored
                }
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                if self.resize.take().is_some() {
                    return EventFlow::Consumed;
                }
                if self.marquee.take().is_none() {
                    return EventFlow::Ignored;
                }
                EventFlow::Consumed
            }
            Event::KeyDown { key, mods } => {
                let count = self.source.row_count();
                if count == 0 {
                    return EventFlow::Ignored;
                }
                let current = self.active_index.unwrap_or(0).min(count - 1);
                let target = match *key {
                    keys::UP => current.saturating_sub(1),
                    keys::DOWN => (current + 1).min(count - 1),
                    keys::HOME => 0,
                    keys::END => count - 1,
                    _ => return EventFlow::Ignored,
                };
                self.move_active(target, mods.shift);
                EventFlow::Consumed
            }
            _ => EventFlow::Ignored,
        }
    }

    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        match property {
            WidgetProperty::VirtualColumns(columns) => {
                self.set_columns(columns);
                true
            }
            WidgetProperty::VirtualSource(source) => {
                self.source = source;
                self.source_revision.set(self.source.revision());
                self.selected_rows.clear();
                self.active_index = None;
                self.cell_cache.borrow_mut().clear();
                self.update_metrics();
                true
            }
            WidgetProperty::VirtualSelectionMode(mode) => {
                self.selection_mode = mode;
                if mode == VirtualSelectionMode::None {
                    self.selected_rows.clear();
                } else if mode == VirtualSelectionMode::Single && self.selected_rows.len() > 1 {
                    let first = self.selected_rows.iter().next().copied();
                    self.selected_rows.clear();
                    self.selected_rows.extend(first);
                }
                true
            }
            WidgetProperty::VirtualSelectedRows(rows) => {
                self.selected_rows = match self.selection_mode {
                    VirtualSelectionMode::None => BTreeSet::new(),
                    VirtualSelectionMode::Single => rows.into_iter().take(1).collect(),
                    VirtualSelectionMode::Multiple => rows.into_iter().collect(),
                };
                true
            }
            WidgetProperty::HeaderHeight(value) => {
                self.header_height = value.max(16.0);
                true
            }
            WidgetProperty::ShowHeader(value) => {
                self.show_header = value;
                true
            }
            WidgetProperty::Striped(value) => {
                self.striped = value;
                true
            }
            WidgetProperty::FillLastColumn(value) => {
                self.fill_last_column = value;
                true
            }
            WidgetProperty::Overscan(value) => {
                self.overscan = value.min(100);
                true
            }
            WidgetProperty::RowHeight(value) => {
                self.row_height = value.max(16.0);
                true
            }
            WidgetProperty::SelectedIndex(index) => self.set_selected_index(index),
            WidgetProperty::ScrollBar(value) => {
                self.scroll.set_visibility(value);
                true
            }
            _ => false,
        }
    }

    fn property(&self, key: WidgetPropertyKey) -> Option<WidgetProperty> {
        match key {
            WidgetPropertyKey::VirtualColumns => {
                Some(WidgetProperty::VirtualColumns(self.columns.clone()))
            }
            WidgetPropertyKey::VirtualSource => {
                Some(WidgetProperty::VirtualSource(self.source.clone()))
            }
            WidgetPropertyKey::VirtualSelectionMode => {
                Some(WidgetProperty::VirtualSelectionMode(self.selection_mode))
            }
            WidgetPropertyKey::VirtualSelectedRows => {
                Some(WidgetProperty::VirtualSelectedRows(self.selected_row_ids()))
            }
            WidgetPropertyKey::HeaderHeight => {
                Some(WidgetProperty::HeaderHeight(self.header_height))
            }
            WidgetPropertyKey::ShowHeader => Some(WidgetProperty::ShowHeader(self.show_header)),
            WidgetPropertyKey::Striped => Some(WidgetProperty::Striped(self.striped)),
            WidgetPropertyKey::FillLastColumn => {
                Some(WidgetProperty::FillLastColumn(self.fill_last_column))
            }
            WidgetPropertyKey::Overscan => Some(WidgetProperty::Overscan(self.overscan)),
            WidgetPropertyKey::RowHeight => Some(WidgetProperty::RowHeight(self.row_height)),
            WidgetPropertyKey::SelectedIndex => {
                self.active_index.map(WidgetProperty::SelectedIndex)
            }
            WidgetPropertyKey::ScrollBar => {
                Some(WidgetProperty::ScrollBar(self.scroll.visibility()))
            }
            _ => None,
        }
    }

    fn selected_index(&self) -> Option<usize> {
        self.active_index
    }

    fn set_selected_index(&mut self, index: usize) -> bool {
        self.select_row(index, false, false)
    }

    fn selected_rows(&self) -> Option<Vec<u64>> {
        Some(self.selected_row_ids())
    }

    fn sort_state(&self) -> Option<VirtualSort> {
        self.sort.clone()
    }

    fn virtual_columns(&self) -> Option<Vec<VirtualColumn>> {
        Some(self.columns.clone())
    }

    fn refresh_data(&mut self) -> bool {
        self.source_revision.set(self.source.revision());
        self.cell_cache.borrow_mut().clear();
        self.update_metrics();
        true
    }

    fn is_scrollable(&self) -> bool {
        true
    }

    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        self.scroll.scroll_by(dx, dy)
    }

    fn scroll_offset(&self) -> Option<Point> {
        Some(self.scroll.offset())
    }

    fn scrollbar_grab(&self, pos: Point) -> Option<crate::scroll::ScrollGrab> {
        crate::scroll::thumb_grab(
            &self.scroll,
            self.scrollbar_viewport(),
            &self.scrollbar,
            pos,
        )
    }

    fn scrollbar_drag(&mut self, pos: Point, grab: &crate::scroll::ScrollGrab) -> bool {
        let viewport = self.scrollbar_viewport();
        crate::scroll::apply_thumb_drag(&mut self.scroll, viewport, &self.scrollbar, pos, grab)
    }

    fn scrollbar_contains(&self, pos: Point) -> bool {
        crate::scroll::scrollbar_region_contains(
            &self.scroll,
            self.scrollbar_viewport(),
            &self.scrollbar,
            pos,
        )
    }

    fn animation_value(&self, prop: AnimProp) -> Option<f32> {
        self.scroll.axis_value(prop)
    }

    fn set_animation_value(&mut self, prop: AnimProp, value: f32) -> bool {
        self.scroll.set_axis_value(prop, value)
    }
}

common_builders!(VirtualList);

fn mix(background: Color, foreground: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    Color::rgba(
        background.r + (foreground.r - background.r) * amount,
        background.g + (foreground.g - background.g) * amount,
        background.b + (foreground.b - background.b) * amount,
        background.a,
    )
}

fn paint_dashed_rect(cv: &mut dyn Canvas, rect: Rect, color: Color) {
    let stroke = 1.0;
    let step = MARQUEE_DASH_LENGTH + MARQUEE_DASH_GAP;
    let mut x = rect.left();
    while x < rect.right() {
        let width = MARQUEE_DASH_LENGTH.min(rect.right() - x);
        cv.fill_rect(Rect::new(x, rect.top(), width, stroke), color);
        cv.fill_rect(
            Rect::new(x, (rect.bottom() - stroke).max(rect.top()), width, stroke),
            color,
        );
        x += step;
    }
    let mut y = rect.top();
    while y < rect.bottom() {
        let height = MARQUEE_DASH_LENGTH.min(rect.bottom() - y);
        cv.fill_rect(Rect::new(rect.left(), y, stroke, height), color);
        cv.fill_rect(
            Rect::new((rect.right() - stroke).max(rect.left()), y, stroke, height),
            color,
        );
        y += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::Dispatcher;
    use flexui_gfx::Corners;

    struct CountingSource {
        rows: usize,
        reads: Cell<usize>,
    }

    impl VirtualListSource for CountingSource {
        fn row_count(&self) -> usize {
            self.rows
        }

        fn row_id(&self, row: usize) -> u64 {
            row as u64 + 100
        }

        fn cell_text(&self, row: usize, column_key: &str) -> String {
            self.reads.set(self.reads.get() + 1);
            format!("{column_key}-{row}")
        }
    }

    #[derive(Default)]
    struct FakeCanvas {
        text_draws: Cell<usize>,
        fills: RefCell<Vec<Rect>>,
    }

    impl Canvas for FakeCanvas {
        fn fill_rect(&mut self, rect: Rect, _color: Color) {
            self.fills.borrow_mut().push(rect);
        }
        fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
        fn fill_round_rect(&mut self, _rect: Rect, _radius: Corners, _color: Color) {}
        fn stroke_round_rect(&mut self, _rect: Rect, _radius: Corners, _color: Color, _width: f32) {
        }
        fn draw_text(&mut self, _text: &str, _origin: Point, _font: &Font, _color: Color) {
            self.text_draws.set(self.text_draws.get() + 1);
        }
        fn measure_text(&self, text: &str, font: &Font) -> Size {
            Size::new(
                text.chars().count() as f32 * font.size * 0.55,
                font.size * 1.2,
            )
        }
    }

    fn layout_list(list: &mut VirtualList, canvas: &FakeCanvas) {
        list.base_mut().rect = Rect::new(0.0, 0.0, 400.0, 220.0);
        list.arrange(Rect::new(0.0, 0.0, 400.0, 220.0), canvas);
    }

    fn selectable_list() -> VirtualList {
        VirtualList::new()
            .source(Rc::new(CountingSource {
                rows: 20,
                reads: Cell::new(0),
            }))
            .selection_mode(VirtualSelectionMode::Multiple)
    }

    fn row_center(row: usize) -> Point {
        Point::new(40.0, DEFAULT_HEADER_HEIGHT + row as f32 * DEFAULT_ROW_HEIGHT + 18.0)
    }

    fn click(list: &mut VirtualList, row: usize, mods: crate::event::Mods) {
        let pos = row_center(row);
        assert_eq!(
            list.on_event(&Event::MouseDown {
                pos,
                button: MouseButton::Left,
                mods,
            }),
            EventFlow::Consumed
        );
        assert_eq!(
            list.on_event(&Event::MouseUp {
                pos,
                button: MouseButton::Left,
            }),
            EventFlow::Consumed
        );
    }

    fn toggle_mods() -> crate::event::Mods {
        #[cfg(target_os = "macos")]
        {
            crate::event::Mods {
                meta: true,
                ..Default::default()
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            crate::event::Mods {
                ctrl: true,
                ..Default::default()
            }
        }
    }

    #[test]
    fn 百万行只读取可见单元格() {
        let source = Rc::new(CountingSource {
            rows: 1_000_000,
            reads: Cell::new(0),
        });
        let mut list = VirtualList::new()
            .columns(vec![
                VirtualColumn::new("id", "ID", 100.0),
                VirtualColumn::new("name", "Name", 180.0),
            ])
            .source(source.clone());
        let mut canvas = FakeCanvas::default();
        layout_list(&mut list, &canvas);
        list.paint_content(&mut canvas, &StyleSpec::default());
        assert!(
            source.reads.get() < 40,
            "实际读取 {} 个单元格",
            source.reads.get()
        );
        assert_eq!(list.scroll.content().height, 36_000_000.0);
    }

    #[test]
    fn 重绘复用有界排版缓存() {
        let source = Rc::new(CountingSource {
            rows: 1000,
            reads: Cell::new(0),
        });
        let mut list = VirtualList::new()
            .columns(vec![VirtualColumn::new("name", "Name", 180.0)])
            .source(source.clone());
        let mut canvas = FakeCanvas::default();
        layout_list(&mut list, &canvas);
        list.paint_content(&mut canvas, &StyleSpec::default());
        let first = source.reads.get();
        list.paint_content(&mut canvas, &StyleSpec::default());
        assert_eq!(source.reads.get(), first);
        assert!(list.cell_cache.borrow().layouts.len() <= CELL_CACHE_CAPACITY);
    }

    #[test]
    fn 多选使用稳定行_id() {
        let rows = Rc::new(VirtualListRows::from_rows(vec![
            VirtualListRow::new(42).cell("name", "B"),
            VirtualListRow::new(7).cell("name", "A"),
        ]));
        let mut list = VirtualList::new()
            .columns(vec![VirtualColumn::new("name", "Name", 120.0)])
            .source(rows)
            .selection_mode(VirtualSelectionMode::Multiple);
        let canvas = FakeCanvas::default();
        layout_list(&mut list, &canvas);
        assert!(list.select_row(0, false, true));
        assert!(list.select_row(1, false, true));
        assert_eq!(list.selected_row_ids(), vec![7, 42]);
        list.toggle_sort(0);
        assert_eq!(list.selected_row_ids(), vec![7, 42]);
        assert_eq!(list.active_index, Some(0));
    }

    #[test]
    fn 多选模式普通点击替换已有选区() {
        let mut list = selectable_list();
        let canvas = FakeCanvas::default();
        layout_list(&mut list, &canvas);

        click(&mut list, 0, crate::event::Mods::default());
        click(&mut list, 2, toggle_mods());
        assert_eq!(list.selected_row_ids(), vec![100, 102]);

        click(&mut list, 1, crate::event::Mods::default());
        assert_eq!(list.selected_row_ids(), vec![101]);
    }

    #[test]
    fn 主修饰键点击切换单行且_shift_点击连续选择() {
        let mut list = selectable_list();
        let canvas = FakeCanvas::default();
        layout_list(&mut list, &canvas);

        click(&mut list, 1, crate::event::Mods::default());
        click(&mut list, 3, toggle_mods());
        assert_eq!(list.selected_row_ids(), vec![101, 103]);
        click(&mut list, 1, toggle_mods());
        assert_eq!(list.selected_row_ids(), vec![103]);

        click(&mut list, 1, crate::event::Mods::default());
        click(
            &mut list,
            3,
            crate::event::Mods {
                shift: true,
                ..Default::default()
            },
        );
        assert_eq!(list.selected_row_ids(), vec![101, 102, 103]);
    }

    #[test]
    fn 鼠标正反向框选并在抬起后停止更新() {
        let mut list = selectable_list();
        let canvas = FakeCanvas::default();
        layout_list(&mut list, &canvas);

        let start = Point::new(30.0, 50.0);
        list.on_event(&Event::MouseDown {
            pos: start,
            button: MouseButton::Left,
            mods: crate::event::Mods::default(),
        });
        list.on_event(&Event::MouseMove {
            pos: Point::new(180.0, 130.0),
        });
        assert_eq!(list.selected_row_ids(), vec![100, 101, 102]);
        assert!(list.marquee_rect().is_some());
        list.on_event(&Event::MouseUp {
            pos: Point::new(180.0, 130.0),
            button: MouseButton::Left,
        });
        assert!(list.marquee.is_none());
        list.on_event(&Event::MouseMove {
            pos: Point::new(180.0, 200.0),
        });
        assert_eq!(list.selected_row_ids(), vec![100, 101, 102]);

        list.on_event(&Event::MouseDown {
            pos: row_center(3),
            button: MouseButton::Left,
            mods: crate::event::Mods::default(),
        });
        list.on_event(&Event::MouseMove {
            pos: Point::new(20.0, 65.0),
        });
        assert_eq!(list.selected_row_ids(), vec![100, 101, 102, 103]);
    }

    #[test]
    fn 框选边框使用短线段绘制() {
        let mut canvas = FakeCanvas::default();
        let rect = Rect::new(10.0, 20.0, 80.0, 40.0);
        paint_dashed_rect(&mut canvas, rect, Color::BLACK);
        let fills = canvas.fills.borrow();
        assert!(fills.len() > 8);
        assert!(fills.iter().all(|item| {
            item.left() >= rect.left()
                && item.top() >= rect.top()
                && item.right() <= rect.right()
                && item.bottom() <= rect.bottom()
        }));
    }

    #[test]
    fn 动态列改变水平滚动范围() {
        let mut list = VirtualList::new().columns(vec![
            VirtualColumn::new("a", "A", 300.0),
            VirtualColumn::new("b", "B", 300.0),
        ]);
        let canvas = FakeCanvas::default();
        layout_list(&mut list, &canvas);
        assert!(list.scroll.max().x >= 200.0);
        list.set_columns(vec![VirtualColumn::new("a", "A", 100.0)]);
        assert_eq!(list.scroll.max().x, 0.0);
    }

    #[test]
    fn 拖动列宽上报列定义并在抬起后停止() {
        let mut list = VirtualList::new().name("table").columns(vec![
            VirtualColumn::new("a", "A", 100.0),
            VirtualColumn::new("b", "B", 300.0),
        ]);
        let canvas = FakeCanvas::default();
        layout_list(&mut list, &canvas);
        let mut dispatcher = Dispatcher::new();
        dispatcher.handle(
            &mut list,
            &Event::MouseDown {
                pos: Point::new(100.0, 20.0),
                button: MouseButton::Left,
                mods: crate::event::Mods::default(),
            },
        );
        dispatcher.take_control_events();
        dispatcher.handle(
            &mut list,
            &Event::MouseMove {
                pos: Point::new(140.0, 20.0),
            },
        );
        assert!(dispatcher.take_control_events().iter().any(|(_, event)| {
            matches!(event, crate::ControlEvent::ColumnsChanged(columns) if columns[0].width == 140.0)
        }));
        dispatcher.handle(
            &mut list,
            &Event::MouseUp {
                pos: Point::new(140.0, 20.0),
                button: MouseButton::Left,
            },
        );
        dispatcher.handle(
            &mut list,
            &Event::MouseMove {
                pos: Point::new(180.0, 20.0),
            },
        );
        assert_eq!(list.columns[0].width, 140.0);
    }
}
