//! 日期、时间与日历控件。
//!
//! 本模块不依赖操作系统日期 API，也不依赖第三方日期库：
//! - [`Calendar`] 提供固定六行的月份视图、跨月日期显示、月份切换与日期选择；
//! - [`DatePicker`]、[`TimePicker`]、[`DateTimePicker`] 共用一套可展开的自绘选择器；
//! - [`CalendarDate`]、[`TimeOfDay`]、[`DateTimeValue`] 可脱离 UI 单独使用。
//!
//! Picker 当前把展开面板作为控件自身内容参与布局。`open` 状态和面板命中逻辑均封装
//! 在控件内部，后续接入窗口浮层时可直接复用这些状态与日期计算函数。

use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use flexui_gfx::{Canvas, Color, Point, Rect, Size, TextAlign};

use crate::event::{keys, Event, EventFlow, MouseButton};
use crate::layout;
use crate::paint::draw_aligned_text;
use crate::sizing::Sizing;
use crate::style::{StyleSet, StyleSpec};
use crate::theme::WidgetKind;
use crate::widget::{Base, Widget, WidgetProperty, WidgetPropertyKey, WidgetRole};

const CALENDAR_WIDTH: f32 = 280.0;
const CALENDAR_HEADER_HEIGHT: f32 = 40.0;
const WEEK_HEADER_HEIGHT: f32 = 28.0;
const DAY_ROW_HEIGHT: f32 = 34.0;
const CALENDAR_HEIGHT: f32 = CALENDAR_HEADER_HEIGHT + WEEK_HEADER_HEIGHT + DAY_ROW_HEIGHT * 6.0;
const PICKER_FIELD_HEIGHT: f32 = 40.0;
const TIME_PANEL_HEIGHT: f32 = 100.0;

/// 公历日期，支持 1..=9999 年。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CalendarDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl CalendarDate {
    /// 创建合法日期；年月日越界时返回 `None`。
    pub fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        if !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
            return None;
        }
        if day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// 当前 UTC 日期。业务若需要严格的本地日期，应从平台层传入本地日期。
    pub fn today_utc() -> Self {
        let days = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| (duration.as_secs() / 86_400) as i64)
            .unwrap_or(0);
        civil_from_days(days).unwrap_or(Self {
            year: 1970,
            month: 1,
            day: 1,
        })
    }

    /// 是否为闰年。
    pub const fn is_leap_year(year: i32) -> bool {
        year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
    }

    /// 本日期所在月份的天数。
    pub const fn month_days(self) -> u8 {
        days_in_month(self.year, self.month)
    }

    /// 周一为 0、周日为 6。
    pub fn weekday_from_monday(self) -> u8 {
        (days_from_civil(self) + 3).rem_euclid(7) as u8
    }

    /// 增减天数。结果超出 1..=9999 年时保持原日期。
    pub fn add_days(self, days: i32) -> Self {
        civil_from_days(days_from_civil(self) + i64::from(days)).unwrap_or(self)
    }

    /// 增减月份；目标月份没有原来的日期时自动夹取到月末。
    pub fn add_months(self, months: i32) -> Self {
        let zero_based = i64::from(self.year) * 12 + i64::from(self.month) - 1 + i64::from(months);
        let year = zero_based.div_euclid(12) as i32;
        let month = zero_based.rem_euclid(12) as u8 + 1;
        if !(1..=9999).contains(&year) {
            return self;
        }
        Self {
            year,
            month,
            day: self.day.min(days_in_month(year, month)),
        }
    }
}

impl fmt::Display for CalendarDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for CalendarDate {
    type Err = DateTimeParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.trim().split('-');
        let year = parse_part(parts.next())?;
        let month = parse_part(parts.next())?;
        let day = parse_part(parts.next())?;
        if parts.next().is_some() {
            return Err(DateTimeParseError);
        }
        Self::new(year, month, day).ok_or(DateTimeParseError)
    }
}

/// 一天内的时间。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeOfDay {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl TimeOfDay {
    /// 创建合法时间。
    pub const fn new(hour: u8, minute: u8, second: u8) -> Option<Self> {
        if hour < 24 && minute < 60 && second < 60 {
            Some(Self {
                hour,
                minute,
                second,
            })
        } else {
            None
        }
    }

    fn seconds_since_midnight(self) -> i32 {
        i32::from(self.hour) * 3600 + i32::from(self.minute) * 60 + i32::from(self.second)
    }

    /// 增减秒数，跨日时在 24 小时内循环。
    pub fn add_seconds(self, seconds: i32) -> Self {
        let value = (i64::from(self.seconds_since_midnight()) + i64::from(seconds))
            .rem_euclid(86_400) as i32;
        Self {
            hour: (value / 3600) as u8,
            minute: ((value % 3600) / 60) as u8,
            second: (value % 60) as u8,
        }
    }
}

impl fmt::Display for TimeOfDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

impl FromStr for TimeOfDay {
    type Err = DateTimeParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.trim().split(':');
        let hour = parse_part(parts.next())?;
        let minute = parse_part(parts.next())?;
        let second = match parts.next() {
            Some(value) => value.parse().map_err(|_| DateTimeParseError)?,
            None => 0,
        };
        if parts.next().is_some() {
            return Err(DateTimeParseError);
        }
        Self::new(hour, minute, second).ok_or(DateTimeParseError)
    }
}

/// 日期时间值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTimeValue {
    pub date: CalendarDate,
    pub time: TimeOfDay,
}

impl DateTimeValue {
    pub const fn new(date: CalendarDate, time: TimeOfDay) -> Self {
        Self { date, time }
    }
}

impl fmt::Display for DateTimeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.date, self.time)
    }
}

impl FromStr for DateTimeValue {
    type Err = DateTimeParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let normalized = text.trim().replace('T', " ");
        let mut parts = normalized.split_whitespace();
        let date = parts.next().ok_or(DateTimeParseError)?.parse()?;
        let time = parts.next().ok_or(DateTimeParseError)?.parse()?;
        if parts.next().is_some() {
            return Err(DateTimeParseError);
        }
        Ok(Self { date, time })
    }
}

/// 日期/时间字符串解析失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTimeParseError;

impl fmt::Display for DateTimeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("无效的日期或时间")
    }
}

impl std::error::Error for DateTimeParseError {}

/// 日历标题和星期文本的语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalendarLocale {
    #[default]
    ZhCn,
    EnUs,
}

/// 自绘月份日历。
pub struct Calendar {
    base: Base,
    display_year: i32,
    display_month: u8,
    selected: Option<CalendarDate>,
    today: Option<CalendarDate>,
    min_date: Option<CalendarDate>,
    max_date: Option<CalendarDate>,
    locale: CalendarLocale,
}

impl Calendar {
    pub fn new() -> Self {
        let today = CalendarDate::today_utc();
        let mut base = Base::new_kind(WidgetRole::Plain, WidgetKind::Panel);
        base.width = Sizing::Fixed(CALENDAR_WIDTH);
        base.height = Sizing::Content;
        base.focusable = true;
        base.text = today.to_string();
        Self {
            base,
            display_year: today.year,
            display_month: today.month,
            selected: Some(today),
            today: Some(today),
            min_date: None,
            max_date: None,
            locale: CalendarLocale::default(),
        }
    }

    /// 设置当前显示月份。
    pub fn display_month(mut self, year: i32, month: u8) -> Self {
        self.set_display_month(year, month);
        self
    }

    pub fn set_display_month(&mut self, year: i32, month: u8) -> bool {
        if !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
            return false;
        }
        let changed = self.display_year != year || self.display_month != month;
        self.display_year = year;
        self.display_month = month;
        changed
    }

    /// 设置选中日期，并同步切换到该月。
    pub fn selected_date(mut self, date: CalendarDate) -> Self {
        self.set_selected_date(date);
        self
    }

    pub fn set_selected_date(&mut self, date: CalendarDate) -> bool {
        if !date_in_range(date, self.min_date, self.max_date) {
            return false;
        }
        let changed = self.selected != Some(date);
        self.selected = Some(date);
        self.display_year = date.year;
        self.display_month = date.month;
        self.base.text = date.to_string();
        changed
    }

    pub fn selection(&self) -> Option<CalendarDate> {
        self.selected
    }

    /// 设置“今天”标记；传 `None` 可关闭今天描边。
    pub fn today(mut self, date: Option<CalendarDate>) -> Self {
        self.today = date;
        self
    }

    /// 限制可选日期范围。起止任一端可省略。
    pub fn range(mut self, min: Option<CalendarDate>, max: Option<CalendarDate>) -> Self {
        self.set_range(min, max);
        self
    }

    pub fn set_range(&mut self, min: Option<CalendarDate>, max: Option<CalendarDate>) {
        self.min_date = min;
        self.max_date = max;
        if self
            .selected
            .is_some_and(|date| !date_in_range(date, min, max))
        {
            self.selected = None;
            self.base.text.clear();
        }
    }

    pub fn locale(mut self, locale: CalendarLocale) -> Self {
        self.locale = locale;
        self
    }

    pub fn shown_month(&self) -> (i32, u8) {
        (self.display_year, self.display_month)
    }

    pub fn previous_month(&mut self) -> bool {
        self.navigate_months(-1)
    }

    pub fn next_month(&mut self) -> bool {
        self.navigate_months(1)
    }

    pub fn navigate_months(&mut self, delta: i32) -> bool {
        let current = CalendarDate {
            year: self.display_year,
            month: self.display_month,
            day: 1,
        };
        let target = current.add_months(delta);
        self.set_display_month(target.year, target.month)
    }

    fn select_relative_days(&mut self, delta: i32) -> bool {
        let start = self.selected.unwrap_or(CalendarDate {
            year: self.display_year,
            month: self.display_month,
            day: 1,
        });
        self.set_selected_date(start.add_days(delta))
    }

    fn handle_pointer(&mut self, point: Point) -> bool {
        let content = layout::content_rect(&self.base);
        match calendar_hit(content, self.display_year, self.display_month, point) {
            CalendarHit::PreviousMonth => self.previous_month(),
            CalendarHit::NextMonth => self.next_month(),
            CalendarHit::Date(date) => self.set_selected_date(date),
            CalendarHit::None => false,
        }
    }
}

impl Default for Calendar {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Calendar {
    fn base(&self) -> &Base {
        &self.base
    }

    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }

    fn measure(&mut self, _avail: Size, _cv: &dyn Canvas) -> Size {
        layout::size_from_content(&self.base, CALENDAR_WIDTH, CALENDAR_HEIGHT)
    }

    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        paint_calendar(
            cv,
            layout::content_rect(&self.base),
            style,
            &self.base,
            CalendarPaintState {
                display_year: self.display_year,
                display_month: self.display_month,
                selected: self.selected,
                today: self.today,
                min_date: self.min_date,
                max_date: self.max_date,
                locale: self.locale,
            },
        );
    }

    fn on_event(&mut self, event: &Event) -> EventFlow {
        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                self.handle_pointer(*pos);
                EventFlow::Consumed
            }
            Event::KeyDown { key, .. } => match *key {
                keys::LEFT => consumed(self.select_relative_days(-1)),
                keys::RIGHT => consumed(self.select_relative_days(1)),
                keys::UP => consumed(self.select_relative_days(-7)),
                keys::DOWN => consumed(self.select_relative_days(7)),
                _ => EventFlow::Ignored,
            },
            _ => EventFlow::Ignored,
        }
    }

    fn wants_pointer_events(&self) -> bool {
        true
    }

    fn set_text_value(&mut self, text: String) {
        if let Ok(date) = text.parse() {
            self.set_selected_date(date);
        }
    }

    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        match property {
            WidgetProperty::Text(text) => {
                self.set_text_value(text);
                true
            }
            _ => false,
        }
    }

    fn property(&self, key: WidgetPropertyKey) -> Option<WidgetProperty> {
        (key == WidgetPropertyKey::Text).then(|| {
            WidgetProperty::Text(
                self.selected
                    .map(|date| date.to_string())
                    .unwrap_or_default(),
            )
        })
    }
}

crate::common_builders!(Calendar);

/// 仅日期选择器的类型标记。
pub struct DateOnly;
/// 仅时间选择器的类型标记。
pub struct TimeOnly;
/// 日期时间选择器的类型标记。
pub struct DateAndTime;

/// Picker 类型标记的公共约束。实现类型由本模块提供。
pub trait PickerMode: private::Sealed {
    const HAS_DATE: bool;
    const HAS_TIME: bool;
}

impl PickerMode for DateOnly {
    const HAS_DATE: bool = true;
    const HAS_TIME: bool = false;
}

impl PickerMode for TimeOnly {
    const HAS_DATE: bool = false;
    const HAS_TIME: bool = true;
}

impl PickerMode for DateAndTime {
    const HAS_DATE: bool = true;
    const HAS_TIME: bool = true;
}

mod private {
    pub trait Sealed {}
    impl Sealed for super::DateOnly {}
    impl Sealed for super::TimeOnly {}
    impl Sealed for super::DateAndTime {}
}

/// 日期/时间选择器的共享实现。
pub struct Picker<M: PickerMode> {
    base: Base,
    value: DateTimeValue,
    open: bool,
    display_year: i32,
    display_month: u8,
    today: Option<CalendarDate>,
    min_date: Option<CalendarDate>,
    max_date: Option<CalendarDate>,
    minute_step: u8,
    second_step: u8,
    locale: CalendarLocale,
    marker: PhantomData<M>,
}

/// 日期选择器。
pub type DatePicker = Picker<DateOnly>;
/// 时间选择器。
pub type TimePicker = Picker<TimeOnly>;
/// 日期时间选择器。
pub type DateTimePicker = Picker<DateAndTime>;

impl Picker<DateOnly> {
    pub fn new() -> Self {
        Self::new_mode()
    }

    pub fn value(mut self, date: CalendarDate) -> Self {
        self.set_date(date);
        self
    }

    pub fn selected_date(&self) -> CalendarDate {
        self.value.date
    }
}

impl Default for Picker<DateOnly> {
    fn default() -> Self {
        Self::new()
    }
}

impl Picker<TimeOnly> {
    pub fn new() -> Self {
        Self::new_mode()
    }

    pub fn value(mut self, time: TimeOfDay) -> Self {
        self.set_time(time);
        self
    }

    pub fn selected_time(&self) -> TimeOfDay {
        self.value.time
    }
}

impl Default for Picker<TimeOnly> {
    fn default() -> Self {
        Self::new()
    }
}

impl Picker<DateAndTime> {
    pub fn new() -> Self {
        Self::new_mode()
    }

    pub fn value(mut self, value: DateTimeValue) -> Self {
        self.set_value(value);
        self
    }

    pub fn selected_value(&self) -> DateTimeValue {
        self.value
    }
}

impl Default for Picker<DateAndTime> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: PickerMode> Picker<M> {
    fn new_mode() -> Self {
        let today = CalendarDate::today_utc();
        let value = DateTimeValue {
            date: today,
            time: TimeOfDay {
                hour: 12,
                minute: 0,
                second: 0,
            },
        };
        let mut base = Base::new_kind(WidgetRole::Plain, WidgetKind::ComboBox);
        base.width = Sizing::Fixed(CALENDAR_WIDTH);
        base.height = Sizing::Content;
        base.focusable = true;
        base.text = format_picker_value::<M>(value);
        Self {
            base,
            value,
            open: false,
            display_year: today.year,
            display_month: today.month,
            today: Some(today),
            min_date: None,
            max_date: None,
            minute_step: 1,
            second_step: 1,
            locale: CalendarLocale::default(),
            marker: PhantomData,
        }
    }

    /// 设置日期部分。
    pub fn date(mut self, date: CalendarDate) -> Self {
        self.set_date(date);
        self
    }

    /// 设置时间部分。
    pub fn time(mut self, time: TimeOfDay) -> Self {
        self.set_time(time);
        self
    }

    pub fn set_date(&mut self, date: CalendarDate) -> bool {
        if !date_in_range(date, self.min_date, self.max_date) {
            return false;
        }
        let changed = self.value.date != date;
        self.value.date = date;
        self.display_year = date.year;
        self.display_month = date.month;
        self.sync_text();
        changed
    }

    pub fn set_time(&mut self, time: TimeOfDay) -> bool {
        let changed = self.value.time != time;
        self.value.time = time;
        self.sync_text();
        changed
    }

    pub fn set_value(&mut self, value: DateTimeValue) -> bool {
        if !date_in_range(value.date, self.min_date, self.max_date) {
            return false;
        }
        let changed = self.value != value;
        self.value = value;
        self.display_year = value.date.year;
        self.display_month = value.date.month;
        self.sync_text();
        changed
    }

    pub fn date_value(&self) -> CalendarDate {
        self.value.date
    }

    pub fn time_value(&self) -> TimeOfDay {
        self.value.time
    }

    pub fn date_time_value(&self) -> DateTimeValue {
        self.value
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn set_open(&mut self, open: bool) -> bool {
        let changed = self.open != open;
        self.open = open;
        changed
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle(&mut self) -> bool {
        self.open = !self.open;
        self.open
    }

    pub fn minute_step(mut self, step: u8) -> Self {
        self.minute_step = step.clamp(1, 59);
        self
    }

    pub fn second_step(mut self, step: u8) -> Self {
        self.second_step = step.clamp(1, 59);
        self
    }

    pub fn range(mut self, min: Option<CalendarDate>, max: Option<CalendarDate>) -> Self {
        self.set_range(min, max);
        self
    }

    pub fn set_range(&mut self, min: Option<CalendarDate>, max: Option<CalendarDate>) {
        self.min_date = min;
        self.max_date = max;
        if !date_in_range(self.value.date, min, max) {
            if let Some(date) = min.or(max) {
                self.value.date = date;
                self.display_year = date.year;
                self.display_month = date.month;
                self.sync_text();
            }
        }
    }

    pub fn locale(mut self, locale: CalendarLocale) -> Self {
        self.locale = locale;
        self
    }

    // 常用布局/样式 Builder；XML 构建器仍可通过 `base_mut` 应用全部通用属性。
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.base.name = Some(name.into());
        self
    }

    pub fn style(mut self, style: StyleSet) -> Self {
        self.base.style = style;
        self
    }

    pub fn variant(mut self, variant: impl Into<String>) -> Self {
        self.base.variant = variant.into();
        self
    }

    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.base.classes.push(class.into());
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.base.width = Sizing::Fixed(width);
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.base.height = Sizing::Fixed(height);
        self
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.base.padding = flexui_gfx::Insets::all(padding);
        self
    }

    fn desired_height(&self) -> f32 {
        if !self.open {
            PICKER_FIELD_HEIGHT
        } else if M::HAS_DATE && M::HAS_TIME {
            PICKER_FIELD_HEIGHT + CALENDAR_HEIGHT + TIME_PANEL_HEIGHT
        } else if M::HAS_DATE {
            PICKER_FIELD_HEIGHT + CALENDAR_HEIGHT
        } else {
            PICKER_FIELD_HEIGHT + TIME_PANEL_HEIGHT
        }
    }

    fn sync_text(&mut self) {
        self.base.text = format_picker_value::<M>(self.value);
    }

    fn handle_pointer(&mut self, point: Point) {
        let content = layout::content_rect(&self.base);
        if point.y < content.top() + PICKER_FIELD_HEIGHT {
            self.toggle();
            return;
        }
        if !self.open {
            return;
        }

        let mut top = content.top() + PICKER_FIELD_HEIGHT;
        if M::HAS_DATE {
            let calendar_rect = Rect::new(content.left(), top, content.size.width, CALENDAR_HEIGHT);
            match calendar_hit(calendar_rect, self.display_year, self.display_month, point) {
                CalendarHit::PreviousMonth => self.navigate_months(-1),
                CalendarHit::NextMonth => self.navigate_months(1),
                CalendarHit::Date(date) => {
                    if self.set_date(date) && !M::HAS_TIME {
                        self.open = false;
                    }
                }
                CalendarHit::None => {}
            }
            top += CALENDAR_HEIGHT;
        }
        if M::HAS_TIME {
            let time_rect = Rect::new(content.left(), top, content.size.width, TIME_PANEL_HEIGHT);
            if time_rect.contains(point) {
                self.adjust_time_from_point(time_rect, point);
            }
        }
    }

    fn navigate_months(&mut self, delta: i32) {
        let current = CalendarDate {
            year: self.display_year,
            month: self.display_month,
            day: 1,
        };
        let target = current.add_months(delta);
        self.display_year = target.year;
        self.display_month = target.month;
    }

    fn adjust_time_from_point(&mut self, rect: Rect, point: Point) {
        let column_width = rect.size.width / 3.0;
        let column = ((point.x - rect.left()) / column_width)
            .floor()
            .clamp(0.0, 2.0) as usize;
        let direction = if point.y < rect.top() + rect.size.height / 2.0 {
            1
        } else {
            -1
        };
        let seconds = match column {
            0 => direction * 3600,
            1 => direction * i32::from(self.minute_step) * 60,
            _ => direction * i32::from(self.second_step),
        };
        self.value.time = self.value.time.add_seconds(seconds);
        self.sync_text();
    }

    fn handle_keyboard(&mut self, key: u32) -> bool {
        if M::HAS_DATE {
            let delta = match key {
                keys::LEFT => -1,
                keys::RIGHT => 1,
                keys::UP => -7,
                keys::DOWN => 7,
                _ => 0,
            };
            if delta != 0 {
                return self.set_date(self.value.date.add_days(delta));
            }
        }
        false
    }
}

impl<M: PickerMode> Widget for Picker<M> {
    fn base(&self) -> &Base {
        &self.base
    }

    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }

    fn measure(&mut self, _avail: Size, _cv: &dyn Canvas) -> Size {
        layout::size_from_content(&self.base, CALENDAR_WIDTH, self.desired_height())
    }

    fn paint_content(&self, cv: &mut dyn Canvas, style: &StyleSpec) {
        let content = layout::content_rect(&self.base);
        paint_picker_field(cv, content, style, &self.base, self.open);
        if !self.open {
            return;
        }

        let mut top = content.top() + PICKER_FIELD_HEIGHT;
        if M::HAS_DATE {
            let calendar_rect = Rect::new(content.left(), top, content.size.width, CALENDAR_HEIGHT);
            paint_calendar(
                cv,
                calendar_rect,
                style,
                &self.base,
                CalendarPaintState {
                    display_year: self.display_year,
                    display_month: self.display_month,
                    selected: Some(self.value.date),
                    today: self.today,
                    min_date: self.min_date,
                    max_date: self.max_date,
                    locale: self.locale,
                },
            );
            top += CALENDAR_HEIGHT;
        }
        if M::HAS_TIME {
            paint_time_panel(
                cv,
                Rect::new(content.left(), top, content.size.width, TIME_PANEL_HEIGHT),
                style,
                &self.base,
                self.value.time,
            );
        }
    }

    fn on_event(&mut self, event: &Event) -> EventFlow {
        match event {
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                self.handle_pointer(*pos);
                EventFlow::Consumed
            }
            Event::KeyDown { key, .. } if *key == keys::ENTER || *key == keys::ESCAPE => {
                if *key == keys::ENTER {
                    self.toggle();
                } else {
                    self.set_open(false);
                }
                EventFlow::Consumed
            }
            Event::KeyDown { key, .. } => consumed(self.handle_keyboard(*key)),
            _ => EventFlow::Ignored,
        }
    }

    fn wants_pointer_events(&self) -> bool {
        true
    }

    fn layout_state(&self) -> u64 {
        u64::from(self.open)
    }

    fn set_text_value(&mut self, text: String) {
        if M::HAS_DATE && M::HAS_TIME {
            if let Ok(value) = text.parse() {
                self.set_value(value);
            }
        } else if M::HAS_DATE {
            if let Ok(date) = text.parse() {
                self.set_date(date);
            }
        } else if let Ok(time) = text.parse() {
            self.set_time(time);
        }
    }

    fn apply_property(&mut self, property: WidgetProperty) -> bool {
        match property {
            WidgetProperty::Text(text) => {
                self.set_text_value(text);
                true
            }
            _ => false,
        }
    }

    fn property(&self, key: WidgetPropertyKey) -> Option<WidgetProperty> {
        (key == WidgetPropertyKey::Text).then(|| WidgetProperty::Text(self.base.text.clone()))
    }
}

#[derive(Clone, Copy)]
struct CalendarPaintState {
    display_year: i32,
    display_month: u8,
    selected: Option<CalendarDate>,
    today: Option<CalendarDate>,
    min_date: Option<CalendarDate>,
    max_date: Option<CalendarDate>,
    locale: CalendarLocale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CalendarHit {
    None,
    PreviousMonth,
    NextMonth,
    Date(CalendarDate),
}

fn paint_calendar(
    cv: &mut dyn Canvas,
    rect: Rect,
    style: &StyleSpec,
    base: &Base,
    state: CalendarPaintState,
) {
    if rect.size.width <= 0.0 || rect.size.height <= 0.0 {
        return;
    }
    let foreground = style.fg_color.unwrap_or(Color::from_u8(40, 44, 52, 255));
    let secondary = Color::rgba(
        foreground.r,
        foreground.g,
        foreground.b,
        foreground.a * 0.55,
    );
    let border = style
        .border_color
        .unwrap_or(Color::from_u8(205, 210, 220, 255));
    let accent = style
        .accent_color
        .or(style.selection_color)
        .unwrap_or(Color::from_u8(64, 119, 255, 255));
    let selection_text = Color::from_u8(255, 255, 255, 255);

    cv.stroke_rect(rect, border, 1.0);
    let header = Rect::new(
        rect.left(),
        rect.top(),
        rect.size.width,
        CALENDAR_HEADER_HEIGHT,
    );
    draw_aligned_text(
        cv,
        "‹",
        Rect::new(
            header.left(),
            header.top(),
            CALENDAR_HEADER_HEIGHT,
            header.size.height,
        ),
        &base.font,
        foreground,
        TextAlign::Center,
        false,
    );
    draw_aligned_text(
        cv,
        "›",
        Rect::new(
            header.right() - CALENDAR_HEADER_HEIGHT,
            header.top(),
            CALENDAR_HEADER_HEIGHT,
            header.size.height,
        ),
        &base.font,
        foreground,
        TextAlign::Center,
        false,
    );
    let title = match state.locale {
        CalendarLocale::ZhCn => format!("{}年 {}月", state.display_year, state.display_month),
        CalendarLocale::EnUs => format!("{}-{:02}", state.display_year, state.display_month),
    };
    draw_aligned_text(
        cv,
        &title,
        Rect::new(
            header.left() + CALENDAR_HEADER_HEIGHT,
            header.top(),
            (header.size.width - CALENDAR_HEADER_HEIGHT * 2.0).max(0.0),
            header.size.height,
        ),
        &base.font,
        foreground,
        TextAlign::Center,
        true,
    );

    let labels: [&str; 7] = match state.locale {
        CalendarLocale::ZhCn => ["一", "二", "三", "四", "五", "六", "日"],
        CalendarLocale::EnUs => ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
    };
    let column_width = rect.size.width / 7.0;
    let week_top = rect.top() + CALENDAR_HEADER_HEIGHT;
    for (column, label) in labels.iter().enumerate() {
        draw_aligned_text(
            cv,
            label,
            Rect::new(
                rect.left() + column as f32 * column_width,
                week_top,
                column_width,
                WEEK_HEADER_HEIGHT,
            ),
            &base.font,
            secondary,
            TextAlign::Center,
            true,
        );
    }

    let first = CalendarDate {
        year: state.display_year,
        month: state.display_month,
        day: 1,
    };
    let leading = i32::from(first.weekday_from_monday());
    let grid_top = week_top + WEEK_HEADER_HEIGHT;
    for index in 0..42 {
        let date = first.add_days(index - leading);
        let column = index % 7;
        let row = index / 7;
        let cell = Rect::new(
            rect.left() + column as f32 * column_width,
            grid_top + row as f32 * DAY_ROW_HEIGHT,
            column_width,
            DAY_ROW_HEIGHT,
        );
        let enabled = date_in_range(date, state.min_date, state.max_date);
        let is_selected = state.selected == Some(date);
        let in_month = date.year == state.display_year && date.month == state.display_month;
        if is_selected {
            let side = cell.size.height.min(cell.size.width) - 6.0;
            cv.fill_round_rect(
                Rect::new(
                    cell.left() + (cell.size.width - side) / 2.0,
                    cell.top() + (cell.size.height - side) / 2.0,
                    side,
                    side,
                ),
                flexui_gfx::Corners::all(side / 2.0),
                accent,
            );
        } else if state.today == Some(date) {
            let side = cell.size.height.min(cell.size.width) - 7.0;
            cv.stroke_round_rect(
                Rect::new(
                    cell.left() + (cell.size.width - side) / 2.0,
                    cell.top() + (cell.size.height - side) / 2.0,
                    side,
                    side,
                ),
                flexui_gfx::Corners::all(side / 2.0),
                accent,
                1.0,
            );
        }
        let color = if is_selected {
            selection_text
        } else if !enabled {
            Color::rgba(secondary.r, secondary.g, secondary.b, secondary.a * 0.45)
        } else if in_month {
            foreground
        } else {
            secondary
        };
        draw_aligned_text(
            cv,
            &date.day.to_string(),
            cell,
            &base.font,
            color,
            TextAlign::Center,
            false,
        );
    }
}

fn paint_picker_field(
    cv: &mut dyn Canvas,
    content: Rect,
    style: &StyleSpec,
    base: &Base,
    open: bool,
) {
    let field = Rect::new(
        content.left(),
        content.top(),
        content.size.width,
        PICKER_FIELD_HEIGHT,
    );
    let foreground = style.fg_color.unwrap_or(Color::from_u8(40, 44, 52, 255));
    let border = style
        .border_color
        .unwrap_or(Color::from_u8(205, 210, 220, 255));
    cv.stroke_rect(field, border, 1.0);
    draw_aligned_text(
        cv,
        &base.text,
        Rect::new(
            field.left() + 10.0,
            field.top(),
            (field.size.width - 38.0).max(0.0),
            field.size.height,
        ),
        &base.font,
        foreground,
        TextAlign::Left,
        true,
    );
    draw_aligned_text(
        cv,
        if open { "▴" } else { "▾" },
        Rect::new(field.right() - 32.0, field.top(), 32.0, field.size.height),
        &base.font,
        foreground,
        TextAlign::Center,
        false,
    );
}

fn paint_time_panel(
    cv: &mut dyn Canvas,
    rect: Rect,
    style: &StyleSpec,
    base: &Base,
    time: TimeOfDay,
) {
    let foreground = style.fg_color.unwrap_or(Color::from_u8(40, 44, 52, 255));
    let secondary = Color::rgba(
        foreground.r,
        foreground.g,
        foreground.b,
        foreground.a * 0.62,
    );
    let border = style
        .border_color
        .unwrap_or(Color::from_u8(205, 210, 220, 255));
    cv.stroke_rect(rect, border, 1.0);
    let values = [time.hour, time.minute, time.second];
    let column_width = rect.size.width / 3.0;
    for (column, value) in values.iter().enumerate() {
        let cell = Rect::new(
            rect.left() + column as f32 * column_width,
            rect.top(),
            column_width,
            rect.size.height,
        );
        if column > 0 {
            cv.fill_rect(
                Rect::new(cell.left(), cell.top() + 8.0, 1.0, cell.size.height - 16.0),
                border,
            );
        }
        draw_aligned_text(
            cv,
            "+",
            Rect::new(
                cell.left(),
                cell.top(),
                cell.size.width,
                cell.size.height * 0.28,
            ),
            &base.font,
            secondary,
            TextAlign::Center,
            false,
        );
        draw_aligned_text(
            cv,
            &format!("{value:02}"),
            Rect::new(
                cell.left(),
                cell.top() + cell.size.height * 0.28,
                cell.size.width,
                cell.size.height * 0.44,
            ),
            &base.font,
            foreground,
            TextAlign::Center,
            false,
        );
        draw_aligned_text(
            cv,
            "−",
            Rect::new(
                cell.left(),
                cell.top() + cell.size.height * 0.72,
                cell.size.width,
                cell.size.height * 0.28,
            ),
            &base.font,
            secondary,
            TextAlign::Center,
            false,
        );
    }
}

fn calendar_hit(rect: Rect, year: i32, month: u8, point: Point) -> CalendarHit {
    if !rect.contains(point) {
        return CalendarHit::None;
    }
    if point.y < rect.top() + CALENDAR_HEADER_HEIGHT {
        if point.x < rect.left() + CALENDAR_HEADER_HEIGHT {
            return CalendarHit::PreviousMonth;
        }
        if point.x >= rect.right() - CALENDAR_HEADER_HEIGHT {
            return CalendarHit::NextMonth;
        }
        return CalendarHit::None;
    }
    let grid_top = rect.top() + CALENDAR_HEADER_HEIGHT + WEEK_HEADER_HEIGHT;
    if point.y < grid_top {
        return CalendarHit::None;
    }
    let column_width = rect.size.width / 7.0;
    if column_width <= 0.0 {
        return CalendarHit::None;
    }
    let column = ((point.x - rect.left()) / column_width).floor() as i32;
    let row = ((point.y - grid_top) / DAY_ROW_HEIGHT).floor() as i32;
    if !(0..7).contains(&column) || !(0..6).contains(&row) {
        return CalendarHit::None;
    }
    let first = CalendarDate {
        year,
        month,
        day: 1,
    };
    let leading = i32::from(first.weekday_from_monday());
    CalendarHit::Date(first.add_days(row * 7 + column - leading))
}

fn format_picker_value<M: PickerMode>(value: DateTimeValue) -> String {
    match (M::HAS_DATE, M::HAS_TIME) {
        (true, true) => value.to_string(),
        (true, false) => value.date.to_string(),
        (false, true) => value.time.to_string(),
        (false, false) => String::new(),
    }
}

fn date_in_range(
    date: CalendarDate,
    min_date: Option<CalendarDate>,
    max_date: Option<CalendarDate>,
) -> bool {
    min_date.is_none_or(|min| date >= min) && max_date.is_none_or(|max| date <= max)
}

const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if CalendarDate::is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn days_from_civil(date: CalendarDate) -> i64 {
    let mut year = i64::from(date.year);
    let month = i64::from(date.month);
    let day = i64::from(date.day);
    year -= i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> Option<CalendarDate> {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    CalendarDate::new(
        year.try_into().ok()?,
        month.try_into().ok()?,
        day.try_into().ok()?,
    )
}

fn parse_part<T: FromStr>(part: Option<&str>) -> Result<T, DateTimeParseError> {
    part.ok_or(DateTimeParseError)?
        .parse()
        .map_err(|_| DateTimeParseError)
}

fn consumed(changed: bool) -> EventFlow {
    if changed {
        EventFlow::Consumed
    } else {
        EventFlow::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Mods;
    use flexui_gfx::{Corners, Font};

    struct FakeCanvas;

    impl Canvas for FakeCanvas {
        fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
        fn stroke_rect(&mut self, _rect: Rect, _color: Color, _line_width: f32) {}
        fn fill_round_rect(&mut self, _rect: Rect, _radius: Corners, _color: Color) {}
        fn stroke_round_rect(
            &mut self,
            _rect: Rect,
            _radius: Corners,
            _color: Color,
            _line_width: f32,
        ) {
        }
        fn draw_text(&mut self, _text: &str, _origin: Point, _font: &Font, _color: Color) {}
        fn measure_text(&self, text: &str, font: &Font) -> Size {
            Size::new(
                text.chars().count() as f32 * font.size * 0.6,
                font.size * 1.2,
            )
        }
    }

    #[test]
    fn 日期计算覆盖闰年跨月和星期() {
        let leap = CalendarDate::new(2024, 2, 29).unwrap();
        assert_eq!(leap.month_days(), 29);
        assert_eq!(leap.add_days(1), CalendarDate::new(2024, 3, 1).unwrap());
        assert_eq!(leap.add_months(12), CalendarDate::new(2025, 2, 28).unwrap());
        assert_eq!(
            CalendarDate::new(2026, 9, 7).unwrap().weekday_from_monday(),
            0
        );
        assert!(CalendarDate::new(2025, 2, 29).is_none());
    }

    #[test]
    fn 日期时间格式可往返() {
        let date: CalendarDate = "2026-09-08".parse().unwrap();
        let time: TimeOfDay = "09:05".parse().unwrap();
        let value: DateTimeValue = "2026-09-08T09:05:00".parse().unwrap();
        assert_eq!(date.to_string(), "2026-09-08");
        assert_eq!(time.to_string(), "09:05:00");
        assert_eq!(value.to_string(), "2026-09-08 09:05:00");
        assert!("2026-13-01".parse::<CalendarDate>().is_err());
        assert!("25:00".parse::<TimeOfDay>().is_err());
    }

    #[test]
    fn 日历支持翻月和点击跨月日期() {
        let mut calendar = Calendar::new()
            .display_month(2026, 9)
            .selected_date(CalendarDate::new(2026, 9, 8).unwrap());
        assert!(calendar.previous_month());
        assert_eq!(calendar.shown_month(), (2026, 8));
        assert!(calendar.next_month());
        calendar.base_mut().rect = Rect::new(0.0, 0.0, CALENDAR_WIDTH, CALENDAR_HEIGHT);

        // 2026-09-01 为周二，第一格是上月 31 日。
        calendar.on_event(&Event::MouseDown {
            pos: Point::new(10.0, CALENDAR_HEADER_HEIGHT + WEEK_HEADER_HEIGHT + 10.0),
            button: MouseButton::Left,
            mods: Mods::default(),
        });
        assert_eq!(calendar.selection(), CalendarDate::new(2026, 8, 31));
        assert_eq!(calendar.shown_month(), (2026, 8));
    }

    #[test]
    fn 日历日期范围阻止越界选择() {
        let min = CalendarDate::new(2026, 9, 5).unwrap();
        let max = CalendarDate::new(2026, 9, 20).unwrap();
        let mut calendar = Calendar::new()
            .display_month(2026, 9)
            .range(Some(min), Some(max));
        assert!(!calendar.set_selected_date(CalendarDate::new(2026, 9, 4).unwrap()));
        assert!(calendar.set_selected_date(CalendarDate::new(2026, 9, 12).unwrap()));
    }

    #[test]
    fn 三种_picker_格式和展开高度独立() {
        let date = CalendarDate::new(2026, 9, 8).unwrap();
        let time = TimeOfDay::new(18, 30, 15).unwrap();
        let mut date_picker = DatePicker::new().value(date).open(true);
        let mut time_picker = TimePicker::new().value(time).open(true);
        let mut date_time_picker = DateTimePicker::new()
            .value(DateTimeValue::new(date, time))
            .open(true);
        assert_eq!(date_picker.base().text, "2026-09-08");
        assert_eq!(time_picker.base().text, "18:30:15");
        assert_eq!(date_time_picker.base().text, "2026-09-08 18:30:15");
        assert_eq!(
            date_picker.measure(Size::default(), &FakeCanvas).height,
            PICKER_FIELD_HEIGHT + CALENDAR_HEIGHT
        );
        assert_eq!(
            time_picker.measure(Size::default(), &FakeCanvas).height,
            PICKER_FIELD_HEIGHT + TIME_PANEL_HEIGHT
        );
        assert_eq!(
            date_time_picker
                .measure(Size::default(), &FakeCanvas)
                .height,
            PICKER_FIELD_HEIGHT + CALENDAR_HEIGHT + TIME_PANEL_HEIGHT
        );
    }

    #[test]
    fn 时间面板按步长增减() {
        let mut picker = TimePicker::new()
            .value(TimeOfDay::new(23, 55, 0).unwrap())
            .minute_step(5)
            .open(true);
        picker.base_mut().rect = Rect::new(
            0.0,
            0.0,
            CALENDAR_WIDTH,
            PICKER_FIELD_HEIGHT + TIME_PANEL_HEIGHT,
        );
        // 中间列上半区：分钟 +5，并跨到下一小时。
        picker.on_event(&Event::MouseDown {
            pos: Point::new(CALENDAR_WIDTH / 2.0, PICKER_FIELD_HEIGHT + 10.0),
            button: MouseButton::Left,
            mods: Mods::default(),
        });
        assert_eq!(picker.selected_time(), TimeOfDay::new(0, 0, 0).unwrap());
    }
}
