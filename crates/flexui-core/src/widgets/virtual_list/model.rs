//! VirtualList 的公开列模型、行模型和惰性数据源协议。

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use flexui_gfx::TextAlign;

/// 虚拟列表的列定义。
#[derive(Debug, Clone, PartialEq)]
pub struct VirtualColumn {
    pub key: String,
    pub title: String,
    pub width: f32,
    pub min_width: f32,
    pub max_width: f32,
    /// 剩余宽度分配权重；0 表示固定宽度。
    pub flex: f32,
    pub align: TextAlign,
    pub sortable: bool,
    pub resizable: bool,
}

impl VirtualColumn {
    pub fn new(key: impl Into<String>, title: impl Into<String>, width: f32) -> Self {
        Self {
            key: key.into(),
            title: title.into(),
            width: width.max(1.0),
            min_width: 48.0,
            max_width: 1200.0,
            flex: 0.0,
            align: TextAlign::Left,
            sortable: true,
            resizable: true,
        }
    }

    pub fn min_width(mut self, value: f32) -> Self {
        self.min_width = value.max(1.0);
        self.width = self.width.max(self.min_width);
        self
    }

    pub fn max_width(mut self, value: f32) -> Self {
        self.max_width = value.max(self.min_width);
        self.width = self.width.min(self.max_width);
        self
    }

    pub fn flex(mut self, value: f32) -> Self {
        self.flex = value.max(0.0);
        self
    }

    pub fn align(mut self, value: TextAlign) -> Self {
        self.align = value;
        self
    }

    pub fn sortable(mut self, value: bool) -> Self {
        self.sortable = value;
        self
    }

    pub fn resizable(mut self, value: bool) -> Self {
        self.resizable = value;
        self
    }

    pub(super) fn clamped_width(&self) -> f32 {
        self.width
            .clamp(self.min_width, self.max_width.max(self.min_width))
    }
}

/// 行选择策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VirtualSelectionMode {
    None,
    #[default]
    Single,
    Multiple,
}

/// 排序方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtualSortDirection {
    Ascending,
    Descending,
}

/// 当前排序列。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualSort {
    pub column: String,
    pub direction: VirtualSortDirection,
}

/// 惰性数据源。实现者应保证 `row_id` 在排序和刷新之间稳定且唯一。
pub trait VirtualListSource {
    fn row_count(&self) -> usize;
    fn row_id(&self, row: usize) -> u64;
    fn cell_text(&self, row: usize, column_key: &str) -> String;

    /// 数据版本变化时递增；控件据此自动作废单元格排版缓存。
    fn revision(&self) -> u64 {
        0
    }

    /// 可选排序入口。返回 true 表示数据源已应用排序。
    fn set_sort(&self, _sort: Option<&VirtualSort>) -> bool {
        false
    }

    /// 可选反向索引，用于排序后恢复活动行位置。
    fn index_of_row_id(&self, row_id: u64) -> Option<usize> {
        (0..self.row_count()).find(|&row| self.row_id(row) == row_id)
    }
}

pub type VirtualListSourceRef = Rc<dyn VirtualListSource>;

/// 内置的键值行，适合中小规模静态数据和 XML 数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualListRow {
    pub id: u64,
    pub cells: HashMap<String, String>,
}

impl VirtualListRow {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            cells: HashMap::new(),
        }
    }

    pub fn cell(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.cells.insert(key.into(), value.into());
        self
    }
}

/// 可共享、可变更的内置行数据源。
#[derive(Default)]
pub struct VirtualListRows {
    rows: RefCell<Vec<VirtualListRow>>,
    revision: Cell<u64>,
}

impl VirtualListRows {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_rows(rows: Vec<VirtualListRow>) -> Self {
        Self {
            rows: RefCell::new(rows),
            revision: Cell::new(1),
        }
    }

    pub fn len(&self) -> usize {
        self.rows.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn push(&self, row: VirtualListRow) {
        self.rows.borrow_mut().push(row);
        self.bump_revision();
    }

    pub fn extend(&self, rows: impl IntoIterator<Item = VirtualListRow>) {
        self.rows.borrow_mut().extend(rows);
        self.bump_revision();
    }

    pub fn replace(&self, rows: Vec<VirtualListRow>) {
        *self.rows.borrow_mut() = rows;
        self.bump_revision();
    }

    pub fn remove_ids(&self, ids: &BTreeSet<u64>) -> usize {
        let mut rows = self.rows.borrow_mut();
        let before = rows.len();
        rows.retain(|row| !ids.contains(&row.id));
        let removed = before - rows.len();
        if removed > 0 {
            drop(rows);
            self.bump_revision();
        }
        removed
    }

    pub fn clear(&self) {
        if !self.rows.borrow().is_empty() {
            self.rows.borrow_mut().clear();
            self.bump_revision();
        }
    }

    fn bump_revision(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
    }
}

impl VirtualListSource for VirtualListRows {
    fn row_count(&self) -> usize {
        self.rows.borrow().len()
    }

    fn row_id(&self, row: usize) -> u64 {
        self.rows
            .borrow()
            .get(row)
            .map_or(row as u64, |item| item.id)
    }

    fn cell_text(&self, row: usize, column_key: &str) -> String {
        self.rows
            .borrow()
            .get(row)
            .and_then(|item| item.cells.get(column_key))
            .cloned()
            .unwrap_or_default()
    }

    fn revision(&self) -> u64 {
        self.revision.get()
    }

    fn set_sort(&self, sort: Option<&VirtualSort>) -> bool {
        let Some(sort) = sort else {
            return false;
        };
        let mut rows = self.rows.borrow_mut();
        let key = &sort.column;
        rows.sort_by(|a, b| {
            let left = a.cells.get(key).map(String::as_str).unwrap_or("");
            let right = b.cells.get(key).map(String::as_str).unwrap_or("");
            let order = natural_text_cmp(left, right).then_with(|| a.id.cmp(&b.id));
            match sort.direction {
                VirtualSortDirection::Ascending => order,
                VirtualSortDirection::Descending => order.reverse(),
            }
        });
        drop(rows);
        self.bump_revision();
        true
    }

    fn index_of_row_id(&self, row_id: u64) -> Option<usize> {
        self.rows.borrow().iter().position(|row| row.id == row_id)
    }
}

fn natural_text_cmp(left: &str, right: &str) -> Ordering {
    match (left.parse::<f64>(), right.parse::<f64>()) {
        (Ok(left), Ok(right)) => left.total_cmp(&right),
        _ => left.to_lowercase().cmp(&right.to_lowercase()),
    }
}
