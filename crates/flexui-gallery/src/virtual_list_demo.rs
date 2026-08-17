use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use flexui::{
    NativeMenu, NativeMenuAnchor, NativeMenuItem, Point, TextAlign, VirtualColumn,
    VirtualListSource, VirtualListSourceRef, VirtualSort, VirtualSortDirection, WidgetProperty,
    WidgetPropertyKey, WindowCtx,
};

pub(crate) const CONTROL_NAME: &str = "virtual_table";
const INITIAL_ROWS: usize = 100_000;

pub(crate) struct VirtualListDemo {
    source: Rc<GallerySource>,
    columns: Vec<VirtualColumn>,
    compact: bool,
    custom_column: usize,
    sort: Option<VirtualSort>,
}

impl Default for VirtualListDemo {
    fn default() -> Self {
        Self {
            source: Rc::new(GallerySource::new(INITIAL_ROWS)),
            columns: initial_columns(),
            compact: false,
            custom_column: 1,
            sort: None,
        }
    }
}

impl VirtualListDemo {
    pub(crate) fn init(&self, ctx: &mut WindowCtx) {
        let source: VirtualListSourceRef = self.source.clone();
        ctx.set_property(CONTROL_NAME, WidgetProperty::VirtualSource(source));
        ctx.set_property(
            CONTROL_NAME,
            WidgetProperty::VirtualColumns(self.columns.clone()),
        );
        self.update_status(ctx);
    }

    pub(crate) fn handle_click(&mut self, name: &str, ctx: &mut WindowCtx) -> bool {
        match name {
            "virtual_add_column" => self.add_column(ctx),
            "virtual_remove_column" => self.remove_column(ctx),
            "virtual_add_rows" => {
                self.source.append(1_000);
                ctx.refresh_data(CONTROL_NAME);
            }
            "virtual_delete_selected" => self.delete_selected(ctx),
            "virtual_reset" => {
                self.source.reset(INITIAL_ROWS);
                ctx.set_property(
                    CONTROL_NAME,
                    WidgetProperty::VirtualSelectedRows(Vec::new()),
                );
                ctx.refresh_data(CONTROL_NAME);
            }
            "virtual_density" => self.toggle_density(ctx),
            _ => return false,
        }
        self.update_status(ctx);
        true
    }

    pub(crate) fn show_context_menu(&mut self, x: f32, y: f32, ctx: &mut WindowCtx) {
        let menu = context_menu(!selected_ids(ctx).is_empty());
        if let Some(command) = ctx.popup_native_menu(
            &menu,
            NativeMenuAnchor::Window(Point::new(x, y)),
        ) {
            self.handle_click(&command, ctx);
        }
    }

    pub(crate) fn selection_changed(&self, ctx: &mut WindowCtx) {
        self.update_status(ctx);
    }

    pub(crate) fn sort_changed(&mut self, sort: Option<VirtualSort>, ctx: &mut WindowCtx) {
        self.sort = sort;
        self.update_status(ctx);
    }

    pub(crate) fn columns_changed(&mut self, columns: Vec<VirtualColumn>) {
        self.columns = columns;
    }

    fn add_column(&mut self, ctx: &mut WindowCtx) {
        let candidates = extra_columns();
        if let Some(column) = candidates
            .into_iter()
            .find(|candidate| !self.columns.iter().any(|item| item.key == candidate.key))
        {
            self.columns.push(column);
        } else {
            let number = self.custom_column;
            self.custom_column += 1;
            self.columns.push(
                VirtualColumn::new(
                    format!("custom_{number}"),
                    format!("Metric {number}"),
                    124.0,
                )
                .align(TextAlign::Right),
            );
        }
        self.apply_columns(ctx);
    }

    fn remove_column(&mut self, ctx: &mut WindowCtx) {
        if self.columns.len() <= 2 {
            return;
        }
        let removed = self.columns.pop();
        if removed
            .as_ref()
            .zip(self.sort.as_ref())
            .is_some_and(|(column, sort)| column.key == sort.column)
        {
            self.sort = None;
        }
        self.apply_columns(ctx);
    }

    fn apply_columns(&self, ctx: &mut WindowCtx) {
        ctx.set_property(
            CONTROL_NAME,
            WidgetProperty::VirtualColumns(self.columns.clone()),
        );
        ctx.set_enabled("virtual_remove_column", self.columns.len() > 2);
    }

    fn delete_selected(&self, ctx: &mut WindowCtx) {
        let selected = selected_ids(ctx);
        if selected.is_empty() {
            return;
        }
        self.source.remove(&selected.into_iter().collect());
        ctx.set_property(
            CONTROL_NAME,
            WidgetProperty::VirtualSelectedRows(Vec::new()),
        );
        ctx.refresh_data(CONTROL_NAME);
    }

    fn toggle_density(&mut self, ctx: &mut WindowCtx) {
        self.compact = !self.compact;
        let (row_height, header_height, label) = if self.compact {
            (28.0, 34.0, "Comfortable")
        } else {
            (36.0, 40.0, "Compact")
        };
        ctx.set_property(CONTROL_NAME, WidgetProperty::RowHeight(row_height));
        ctx.set_property(CONTROL_NAME, WidgetProperty::HeaderHeight(header_height));
        ctx.set_text("virtual_density", label);
    }

    fn update_status(&self, ctx: &mut WindowCtx) {
        let selected = selected_ids(ctx).len();
        let sort = self.sort.as_ref().map_or("Unsorted".to_owned(), |sort| {
            let direction = match sort.direction {
                VirtualSortDirection::Ascending => "ascending",
                VirtualSortDirection::Descending => "descending",
            };
            format!("{} {direction}", sort.column)
        });
        ctx.set_text(
            "virtual_status",
            format!(
                "{} rows  |  {} columns  |  {} selected  |  {sort}",
                format_count(self.source.row_count()),
                self.columns.len(),
                selected
            ),
        );
        ctx.set_enabled("virtual_delete_selected", selected > 0);
    }
}

fn context_menu(has_selection: bool) -> NativeMenu {
    NativeMenu::new()
        .item(NativeMenuItem::new("virtual_reset", "Reset"))
        .item(
            NativeMenuItem::new("virtual_delete_selected", "Delete")
                .enabled(has_selection),
        )
}

fn selected_ids(ctx: &mut WindowCtx) -> Vec<u64> {
    match ctx.property(CONTROL_NAME, WidgetPropertyKey::VirtualSelectedRows) {
        Some(WidgetProperty::VirtualSelectedRows(rows)) => rows,
        _ => Vec::new(),
    }
}

fn initial_columns() -> Vec<VirtualColumn> {
    vec![
        VirtualColumn::new("id", "ID", 84.0).align(TextAlign::Right),
        VirtualColumn::new("service", "Service", 190.0)
            .min_width(120.0)
            .max_width(420.0)
            .flex(1.0),
        VirtualColumn::new("region", "Region", 112.0),
        VirtualColumn::new("status", "Status", 104.0),
        VirtualColumn::new("requests", "Requests", 124.0).align(TextAlign::Right),
    ]
}

fn extra_columns() -> Vec<VirtualColumn> {
    vec![
        VirtualColumn::new("latency", "Latency", 104.0).align(TextAlign::Right),
        VirtualColumn::new("owner", "Owner", 132.0),
        VirtualColumn::new("version", "Version", 104.0),
        VirtualColumn::new("updated", "Updated", 146.0),
    ]
}

fn format_count(value: usize) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            output.push(',');
        }
        output.push(ch);
    }
    output
}

struct GallerySource {
    ids: RefCell<Vec<u64>>,
    next_id: Cell<u64>,
    revision: Cell<u64>,
}

impl GallerySource {
    fn new(rows: usize) -> Self {
        Self {
            ids: RefCell::new((1..=rows as u64).collect()),
            next_id: Cell::new(rows as u64 + 1),
            revision: Cell::new(1),
        }
    }

    fn append(&self, count: usize) {
        let start = self.next_id.get();
        self.ids
            .borrow_mut()
            .extend(start..start.saturating_add(count as u64));
        self.next_id.set(start.saturating_add(count as u64));
        self.bump_revision();
    }

    fn remove(&self, ids: &BTreeSet<u64>) {
        let mut rows = self.ids.borrow_mut();
        let before = rows.len();
        rows.retain(|id| !ids.contains(id));
        if rows.len() != before {
            drop(rows);
            self.bump_revision();
        }
    }

    fn reset(&self, rows: usize) {
        *self.ids.borrow_mut() = (1..=rows as u64).collect();
        self.next_id.set(rows as u64 + 1);
        self.bump_revision();
    }

    fn bump_revision(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
    }
}

impl VirtualListSource for GallerySource {
    fn row_count(&self) -> usize {
        self.ids.borrow().len()
    }

    fn row_id(&self, row: usize) -> u64 {
        self.ids.borrow().get(row).copied().unwrap_or_default()
    }

    fn cell_text(&self, row: usize, column_key: &str) -> String {
        let id = self.row_id(row);
        match column_key {
            "id" => format!("{id:06}"),
            "service" => format!("edge-service-{:04}", id % 4096),
            "region" => REGIONS[id as usize % REGIONS.len()].to_owned(),
            "status" => STATUSES[id as usize % STATUSES.len()].to_owned(),
            "requests" => format_count((id as usize * 7919) % 8_000_000),
            "latency" => format!("{} ms", 8 + id * 37 % 480),
            "owner" => OWNERS[id as usize % OWNERS.len()].to_owned(),
            "version" => format!("v{}.{}.{}", 1 + id % 4, id % 18, id % 31),
            "updated" => format!("2026-08-{:02} {:02}:{:02}", 1 + id % 28, id % 24, id % 60),
            key if key.starts_with("custom_") => format!("{}", id * 97 % 100_000),
            _ => String::new(),
        }
    }

    fn revision(&self) -> u64 {
        self.revision.get()
    }

    fn set_sort(&self, sort: Option<&VirtualSort>) -> bool {
        let Some(sort) = sort else {
            return false;
        };
        let mut ids = self.ids.borrow_mut();
        ids.sort_unstable_by(|left, right| {
            let order = sort_rank(*left, &sort.column)
                .cmp(&sort_rank(*right, &sort.column))
                .then_with(|| left.cmp(right));
            match sort.direction {
                VirtualSortDirection::Ascending => order,
                VirtualSortDirection::Descending => order.reverse(),
            }
        });
        drop(ids);
        self.bump_revision();
        true
    }

    fn index_of_row_id(&self, row_id: u64) -> Option<usize> {
        self.ids.borrow().iter().position(|id| *id == row_id)
    }
}

fn sort_rank(id: u64, key: &str) -> u64 {
    match key {
        "id" => id,
        "service" => id % 4096,
        "region" => id % REGIONS.len() as u64,
        "status" => id % STATUSES.len() as u64,
        "requests" => id * 7919 % 8_000_000,
        "latency" => 8 + id * 37 % 480,
        "owner" => id % OWNERS.len() as u64,
        "version" => (1 + id % 4) * 10_000 + id % 18 * 100 + id % 31,
        "updated" => id % (28 * 24 * 60),
        key if key.starts_with("custom_") => id * 97 % 100_000,
        _ => id,
    }
}

const REGIONS: &[&str] = &["Hong Kong", "Singapore", "Tokyo", "Frankfurt", "Virginia"];
const STATUSES: &[&str] = &["Healthy", "Healthy", "Healthy", "Degraded", "Deploying"];
const OWNERS: &[&str] = &["Edge", "Payments", "Media", "Search", "Platform", "Storage"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 数据源按需生成并支持批量变更() {
        let source = GallerySource::new(100_000);
        assert_eq!(source.row_count(), 100_000);
        assert_eq!(source.cell_text(0, "service"), "edge-service-0001");
        let revision = source.revision();
        source.append(1_000);
        assert_eq!(source.row_count(), 101_000);
        assert!(source.revision() > revision);
        source.remove(&[1, 2, 3].into_iter().collect());
        assert_eq!(source.row_count(), 100_997);
    }

    #[test]
    fn 数量格式化() {
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(100_000), "100,000");
        assert_eq!(format_count(8_123_456), "8,123,456");
    }

    #[test]
    fn 右键菜单仅包含重置和删除且删除状态跟随选区() {
        let empty = context_menu(false);
        assert_eq!(empty.items.len(), 2);
        let flexui::NativeMenuEntry::Item(reset) = &empty.items[0] else {
            panic!("第一项应为 Reset");
        };
        let flexui::NativeMenuEntry::Item(delete) = &empty.items[1] else {
            panic!("第二项应为 Delete");
        };
        assert_eq!(reset.id, "virtual_reset");
        assert_eq!(delete.id, "virtual_delete_selected");
        assert!(!delete.enabled);

        let selected = context_menu(true);
        let flexui::NativeMenuEntry::Item(delete) = &selected.items[1] else {
            panic!("第二项应为 Delete");
        };
        assert!(delete.enabled);
    }
}
