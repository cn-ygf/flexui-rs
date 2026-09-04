use super::*;
use crate::layout::layout_node;
use flexui_gfx::Font;
use flexui_gfx::{Color, Corners, Point, Rect, Size};
use std::cell::RefCell;

/// 记录最后一次 draw_text 文本，用于验证 IME 组合串已内联绘制。
struct RecCanvas {
    last_text: RefCell<String>,
    last_font: RefCell<Option<Font>>,
    last_color: RefCell<Option<Color>>,
    fills: Vec<(Rect, Color)>,
    advance_draws: usize,
}
impl Canvas for RecCanvas {
    fn fill_rect(&mut self, r: Rect, c: Color) {
        self.fills.push((r, c));
    }
    fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
    fn fill_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color) {}
    fn stroke_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color, _w: f32) {}
    fn draw_text(&mut self, t: &str, _o: Point, f: &Font, c: Color) {
        *self.last_text.borrow_mut() = t.to_string();
        *self.last_font.borrow_mut() = Some(f.clone());
        *self.last_color.borrow_mut() = Some(c);
    }
    fn draw_text_advance(&mut self, t: &str, o: Point, f: &Font, c: Color) {
        self.advance_draws += 1;
        self.draw_text(t, o, f, c);
    }
    fn measure_text(&self, t: &str, f: &Font) -> Size {
        Size::new(t.chars().count() as f32 * f.size * 0.6, f.size * 1.2)
    }
}

#[test]
fn ime_组合串内联绘制在光标处() {
    // 文本 "ac"，光标在中间（1），组合串 "b" → 显示 "abc"，text 不变。
    let mut e = Edit::new().text("ac");
    e.state.cursor = 1;
    e.set_marked_text("b".to_string());
    e.base_mut().focused = true;
    let cv = RecCanvas {
        last_text: RefCell::new(String::new()),
        last_font: RefCell::new(None),
        last_color: RefCell::new(None),
        fills: Vec::new(),
        advance_draws: 0,
    };
    let mut cv = cv;
    layout_node(&mut e, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
    let style = StyleSpec::default();
    e.paint_content(&mut cv, &style);
    assert_eq!(*cv.last_text.borrow(), "abc");
    assert_eq!(cv.advance_draws, 1);
    assert_eq!(e.base().text, "ac", "组合串不改动已提交文本");
}

fn rec_canvas() -> RecCanvas {
    RecCanvas {
        last_text: RefCell::new(String::new()),
        last_font: RefCell::new(None),
        last_color: RefCell::new(None),
        fills: Vec::new(),
        advance_draws: 0,
    }
}

use crate::event::{keys, Event, Mods};

fn kd(key: u32, shift: bool) -> Event {
    Event::KeyDown {
        key,
        mods: Mods {
            shift,
            ..Default::default()
        },
    }
}

#[test]
fn placeholder_only_draws_for_empty_text_and_returns_after_delete() {
    let mut edit = Edit::new().placeholder("请输入");
    let mut cv = rec_canvas();
    layout_node(&mut edit, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
    edit.paint_content(&mut cv, &StyleSpec::default());
    assert_eq!(*cv.last_text.borrow(), "请输入");
    assert_eq!(cv.advance_draws, 1, "占位文本必须使用输入文本排版路径");
    assert!(edit.base().text.is_empty(), "占位文本不能成为输入内容");
    edit.on_event(&Event::Char { ch: 'A' });
    edit.paint_content(&mut cv, &StyleSpec::default());
    assert_eq!(*cv.last_text.borrow(), "A");
    edit.on_event(&kd(keys::BACKSPACE, false));
    edit.paint_content(&mut cv, &StyleSpec::default());
    assert_eq!(*cv.last_text.borrow(), "请输入");
}

#[test]
fn 单行选区和光标覆盖实际排版行高() {
    let mut edit = Edit::new().text("fjord你好");
    edit.select_all();
    edit.base_mut().focused = true;
    let mut cv = rec_canvas();
    layout_node(&mut edit, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
    edit.paint_content(&mut cv, &StyleSpec::default());

    let layout = edit.display_layout.as_ref().unwrap();
    let selection = cv
        .fills
        .iter()
        .find_map(|(rect, color)| (*color == SEL_COLOR).then_some(*rect))
        .expect("必须绘制选区背景");
    assert!((selection.size.height - layout.height()).abs() < 0.01);
    assert!(
        (selection.top() - Edit::layout_y(layout, layout::content_rect(edit.base()))).abs() < 0.01
    );
    assert!((edit.text_input_rect().unwrap().size.height - layout.height()).abs() < 0.01);
    assert_eq!(edit.text_input_rect().unwrap().size.width, CARET_WIDTH);
}

#[test]
fn placeholder_draws_complete_font_for_current_state() {
    use crate::style::{BaseState, PlaceholderStyleSpec, VisualState};
    let mut styles = PlaceholderStyleSet::new().with_normal(PlaceholderStyleSpec {
        font_family: Some("Microsoft YaHei".to_string()),
        font_size: Some(13.0),
        fg_color: Some(Color::WHITE),
        bold: Some(true),
        italic: Some(false),
        underline: Some(true),
    });
    styles.set(
        VisualState::new(BaseState::Hot, false),
        PlaceholderStyleSpec {
            font_size: Some(15.0),
            fg_color: Some(Color::BLACK),
            italic: Some(true),
            ..Default::default()
        },
    );
    let mut edit = Edit::new().placeholder("搜索").placeholder_style(styles);
    edit.base_mut().hover = true;
    let mut cv = rec_canvas();
    layout_node(&mut edit, Rect::new(0.0, 0.0, 200.0, 40.0), &cv);
    edit.paint_content(&mut cv, &StyleSpec::default());
    let font = cv.last_font.borrow().clone().unwrap();
    assert_eq!(font.family.as_deref(), Some("Microsoft YaHei"));
    assert_eq!(font.size, 15.0);
    assert!(font.bold && font.italic && font.underline);
    assert_eq!(*cv.last_color.borrow(), Some(Color::BLACK));
}

#[test]
fn 选区_shift方向扩展与收起() {
    let mut e = Edit::new().text("hello"); // cursor=5
                                           // Shift+Left ×2 选中 "lo"
    e.on_event(&kd(keys::LEFT, true));
    e.on_event(&kd(keys::LEFT, true));
    assert_eq!(e.selection(), Some((3, 5)));
    assert_eq!(e.selected_text().as_deref(), Some("lo"));
    // 平移 Left：收起到左端 3，无选区
    e.on_event(&kd(keys::LEFT, false));
    assert_eq!(e.cursor(), 3);
    assert_eq!(e.selection(), None);
}

#[test]
fn 选区_typing替换选区() {
    let mut e = Edit::new().text("hello");
    e.on_event(&kd(keys::HOME, false)); // cursor=0
    e.on_event(&kd(keys::RIGHT, true)); // 选中 "h"
    e.on_event(&kd(keys::RIGHT, true)); // 选中 "he"
    assert_eq!(e.selected_text().as_deref(), Some("he"));
    e.on_event(&Event::Char { ch: 'X' }); // 替换为 X
    assert_eq!(e.base().text, "Xllo");
    assert_eq!(e.cursor(), 1);
    assert_eq!(e.selection(), None);
}

#[test]
fn 选区_全选与钩子() {
    let mut e = Edit::new().text("abcd");
    e.select_all();
    assert_eq!(e.selected_text().as_deref(), Some("abcd"));
    // 粘贴替换
    assert!(e.replace_selection("Z"));
    assert_eq!(e.base().text, "Z");
    // 无选区+空串 → 不改变
    assert!(!e.replace_selection(""));
    // 删选区
    e.select_all();
    assert!(e.delete_selection());
    assert_eq!(e.base().text, "");
    assert!(!e.delete_selection());
}

#[test]
fn 多行_enter插换行与上下移行() {
    let cv = FakeCanvas;
    let mut e = Edit::new().multiline(true).text("ab");
    // 末尾回车 → "ab\n"，光标在第2行行首。
    e.on_event(&kd(keys::ENTER, false));
    assert_eq!(e.base().text, "ab\n");
    e.on_event(&Event::Char { ch: 'c' });
    e.on_event(&Event::Char { ch: 'd' }); // "ab\ncd"
    assert_eq!(e.base().text, "ab\ncd");
    // 布局以建立行缓存。
    layout_node(&mut e, Rect::new(0.0, 0.0, 200.0, 80.0), &cv);
    // 光标此时在末尾(第2行 col2)。上移到第1行同列(col2 → "ab"末尾, 索引2)。
    e.on_event(&kd(keys::UP, false));
    assert_eq!(e.cursor(), 2);
    // 下移回第2行同列(col2 → 索引5)。
    e.on_event(&kd(keys::DOWN, false));
    assert_eq!(e.cursor(), 5);
}

#[test]
fn 多行_measure高度随行数增长() {
    let cv = FakeCanvas;
    let mut one = Edit::new().multiline(true).text("a");
    let mut three = Edit::new().multiline(true).text("a\nb\nc");
    let h1 = one.measure(Size::new(200.0, 200.0), &cv).height;
    let h3 = three.measure(Size::new(200.0, 200.0), &cv).height;
    assert!(h3 > h1, "三行应比一行高: {h3} vs {h1}");
}

#[test]
fn 多行_autoscroll追加后跟随底部() {
    let cv = FakeCanvas;
    let text = (0..20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    // 开启 auto_scroll：设文本后布局，偏移应跟随到底部。
    let mut e = Edit::new().multiline(true).auto_scroll(true);
    e.set_text_value(text.clone());
    layout_node(&mut e, Rect::new(0.0, 0.0, 200.0, 60.0), &cv);
    let s = e.scroll.get();
    assert!(s.max().y > 0.0, "内容应溢出视口");
    assert!(
        (s.offset().y - s.max().y).abs() < 0.5,
        "auto_scroll 应跟随到底部"
    );
    // 关闭 auto_scroll：偏移保持顶部。
    let mut e2 = Edit::new().multiline(true);
    e2.set_text_value(text);
    layout_node(&mut e2, Rect::new(0.0, 0.0, 200.0, 60.0), &cv);
    assert_eq!(e2.scroll.get().offset().y, 0.0, "未开启则不跟随");
}

#[test]
fn 多行大文本_重复布局复用整形缓存() {
    struct Counting {
        calls: Cell<usize>,
    }
    impl Canvas for Counting {
        fn fill_rect(&mut self, _r: Rect, _c: Color) {}
        fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
        fn fill_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color) {}
        fn stroke_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color, _w: f32) {}
        fn draw_text(&mut self, _t: &str, _o: Point, _f: &Font, _c: Color) {}
        fn measure_text(&self, t: &str, f: &Font) -> Size {
            self.calls.set(self.calls.get() + 1);
            Size::new(t.chars().count() as f32 * f.size * 0.6, f.size * 1.2)
        }
    }
    let cv = Counting {
        calls: Cell::new(0),
    };
    let text = (0..50)
        .map(|i| format!("line {i} with some content"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = Edit::new().multiline(true).text(text);
    layout_node(&mut e, Rect::new(0.0, 0.0, 200.0, 100.0), &cv);
    let first = cv.calls.get();
    assert!(first > 0);
    cv.calls.set(0);
    // 文本/字体不变，再次布局不应重新整形每一行。
    layout_node(&mut e, Rect::new(0.0, 0.0, 200.0, 100.0), &cv);
    let second = cv.calls.get();
    assert!(
        second * 4 < first,
        "重复布局应复用整形缓存: first={first} second={second}"
    );
}

#[test]
fn 多行静态_滚动复用离屏带() {
    struct BandCanvas {
        captures: Cell<usize>,
        blits: Cell<usize>,
    }
    impl Canvas for BandCanvas {
        fn fill_rect(&mut self, _r: Rect, _c: Color) {}
        fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
        fn fill_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color) {}
        fn stroke_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color, _w: f32) {}
        fn draw_text(&mut self, _t: &str, _o: Point, _f: &Font, _c: Color) {}
        fn measure_text(&self, t: &str, f: &Font) -> Size {
            Size::new(t.chars().count() as f32 * f.size * 0.6, f.size * 1.2)
        }
        fn scale(&self) -> f32 {
            2.0
        }
        fn capture_layer(
            &mut self,
            size: Size,
            draw: &mut dyn FnMut(&mut dyn Canvas),
        ) -> Option<LayerHandle> {
            self.captures.set(self.captures.get() + 1);
            draw(self);
            Some(LayerHandle::new(size, 2.0, std::rc::Rc::new(())))
        }
        fn draw_layer(&mut self, _layer: &LayerHandle, _origin: Point) {
            self.blits.set(self.blits.get() + 1);
        }
    }
    let mut cv = BandCanvas {
        captures: Cell::new(0),
        blits: Cell::new(0),
    };
    let text = (0..200)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = Edit::new().multiline(true).text(text); // 未聚焦、无选区 → 静态
    layout_node(&mut e, Rect::new(0.0, 0.0, 200.0, 100.0), &cv);
    // 首帧：建带一次 + blit 一次。
    e.paint_content(&mut cv, &StyleSpec::default());
    assert_eq!(cv.captures.get(), 1);
    assert_eq!(cv.blits.get(), 1);
    // 带内小滚动：只 blit，不重建。
    assert!(e.scroll_by(0.0, -20.0));
    e.paint_content(&mut cv, &StyleSpec::default());
    assert_eq!(cv.captures.get(), 1, "带内滚动应复用离屏带");
    assert_eq!(cv.blits.get(), 2);
    // 猛滚越出带：重建一次。
    assert!(e.scroll_by(0.0, -5000.0));
    e.paint_content(&mut cv, &StyleSpec::default());
    assert_eq!(cv.captures.get(), 2, "越界滚动应重建带");
}

#[test]
fn 多行大文本_只整形可见行() {
    let mut cv = rec_canvas();
    let text = (0..200)
        .map(|i| format!("line number {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut e = Edit::new().multiline(true).text(text);
    // 视口只容纳少量行。
    layout_node(&mut e, Rect::new(0.0, 0.0, 200.0, 60.0), &cv);
    e.paint_content(&mut cv, &StyleSpec::default());
    let shaped = e
        .lines
        .iter()
        .filter(|l| l.layout.borrow().is_some())
        .count();
    assert!(e.lines.len() >= 200);
    assert!(
        shaped < 20,
        "只应整形可见行(+光标行)，实际整形 {shaped} / 共 {} 行",
        e.lines.len()
    );
}

#[test]
fn 多行不创建整段单行排版缓存() {
    let cv = FakeCanvas;
    let mut edit = Edit::new().multiline(true).text("first\nsecond");

    layout_node(&mut edit, Rect::new(0.0, 0.0, 200.0, 80.0), &cv);

    assert!(edit.display_layout.is_none());
    assert_eq!(edit.lines.len(), 2);
}

struct FakeCanvas;
impl Canvas for FakeCanvas {
    fn fill_rect(&mut self, _r: Rect, _c: Color) {}
    fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
    fn fill_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color) {}
    fn stroke_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color, _w: f32) {}
    fn draw_text(&mut self, _t: &str, _o: Point, _f: &Font, _c: Color) {}
    fn measure_text(&self, t: &str, f: &Font) -> Size {
        Size::new(t.chars().count() as f32 * f.size * 0.6, f.size * 1.2)
    }
}

#[test]
fn 折叠锚点不算选区() {
    let mut e = Edit::new().text("ab");
    e.on_event(&kd(keys::LEFT, true)); // anchor=2 cursor=1
    e.on_event(&kd(keys::RIGHT, true)); // cursor 回到 2 == anchor
    assert_eq!(e.selection(), None, "锚点与光标重合视为无选区");
    assert_eq!(e.selected_text(), None);
}

#[test]
fn 输入约束统一作用于键盘和粘贴() {
    let mut e = Edit::new().number_only(true).max_chars(4);
    assert!(e.replace_selection("1a23中45"));
    assert_eq!(e.base().text, "1234");
    assert_eq!(e.on_event(&Event::Char { ch: '6' }), EventFlow::Ignored);
    e.select_all();
    assert!(!e.replace_selection("abc"));
    assert_eq!(e.base().text, "1234", "被过滤的输入不应删除当前选区");
}

#[test]
fn 只读与密码保护剪贴板内容() {
    let mut readonly = Edit::new().text("hello").read_only(true);
    readonly.select_all();
    assert_eq!(readonly.selected_text().as_deref(), Some("hello"));
    assert!(!readonly.replace_selection("x"));
    let mut password = Edit::new().text("secret").password(true);
    password.select_all();
    assert_eq!(password.selected_text(), None);
    assert!(!password.delete_selection());
}

#[test]
fn 长文本滚动后光标保持可见且命中考虑滚动() {
    let cv = FakeCanvas;
    let mut e = Edit::new().text("abcdefghij");
    layout_node(&mut e, Rect::new(0.0, 0.0, 40.0, 30.0), &cv);
    assert!(e.scroll.get().offset().x > 0.0);
    let content = layout::content_rect(e.base());
    let caret = e.text_input_rect().unwrap();
    assert!(caret.left() >= content.left() && caret.right() <= content.right() + 2.0);
    assert!(e.hit_index(Point::new(content.right() - 1.0, content.top() + 2.0)) >= 8);
}

#[test]
fn 光标移动和删除遵守grapheme边界() {
    let mut combining = Edit::new().text("a\u{301}b");
    combining.on_event(&kd(keys::HOME, false));
    combining.on_event(&kd(keys::RIGHT, false));
    assert_eq!(combining.cursor(), 2, "组合音标必须与基础字符一起移动");
    combining.on_event(&kd(keys::BACKSPACE, false));
    assert_eq!(combining.base().text, "b");

    let family = "👨‍👩‍👧‍👦";
    let mut emoji = Edit::new().text(format!("{family}x"));
    emoji.on_event(&kd(keys::HOME, false));
    emoji.on_event(&kd(keys::RIGHT, false));
    assert_eq!(
        emoji.cursor(),
        family.chars().count(),
        "ZWJ emoji 必须整体移动"
    );
    emoji.on_event(&kd(keys::DELETE, false));
    assert_eq!(emoji.base().text, family, "Delete 只删除下一个 grapheme");
}

#[test]
fn 鼠标命中不会停在grapheme内部() {
    let boundaries = Edit::grapheme_char_boundaries("a\u{301}b");
    assert_eq!(boundaries, vec![0, 2, 3]);
    assert_eq!(
        Edit::snap_to_grapheme_boundary(1, 16.0, &boundaries, |index| index as f32 * 10.0),
        2
    );

    let family = "👨‍👩‍👧‍👦";
    let boundaries = Edit::grapheme_char_boundaries(&format!("{family}x"));
    let family_end = family.chars().count();
    assert_eq!(boundaries, vec![0, family_end, family_end + 1]);
    assert_eq!(
        Edit::snap_to_grapheme_boundary(3, 65.0, &boundaries, |index| index as f32 * 10.0),
        family_end
    );
}
