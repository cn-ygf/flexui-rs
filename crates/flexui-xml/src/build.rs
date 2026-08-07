//! XML → 控件树的构建（L4/L5）。对应需求 C10（XML 布局）、C11（v-if）、C12（平台谓词）。

use std::collections::HashMap;

use flexui_core::{
    Base, BaseState, Button, CheckBox, Color, Corners, Edit, HBox, HitPolicy, Image, Insets, Label,
    Node, Panel, Radio, StyleSet, StyleSpec, TabBox, TextAlign, VBox, VisualState, WidgetId,
};

use crate::parser::{self, Element};

/// v-if 求值上下文：变量表 + 内置平台谓词。
pub struct Context {
    vars: HashMap<String, bool>,
}

impl Context {
    /// 新建上下文，按当前编译目标注入 `platform.macos` / `platform.windows`。
    pub fn new() -> Self {
        let mut vars = HashMap::new();
        vars.insert("platform.macos".to_string(), cfg!(target_os = "macos"));
        vars.insert("platform.windows".to_string(), cfg!(target_os = "windows"));
        Self { vars }
    }

    /// 设置/覆盖一个布尔变量（也可覆盖平台谓词用于测试）。
    pub fn set(&mut self, key: impl Into<String>, val: bool) -> &mut Self {
        self.vars.insert(key.into(), val);
        self
    }

    fn get(&self, key: &str) -> bool {
        match key {
            "true" => true,
            "false" => false,
            _ => *self.vars.get(key).unwrap_or(&false),
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

/// 加载结果：根节点 + 需要注册到 Dispatcher 的 tabbar 绑定 (group, tabbox_id)。
pub struct LoadResult {
    pub root: Node,
    pub bindings: Vec<(u32, WidgetId)>,
}

/// 加载错误。
#[derive(Debug)]
pub struct LoadError(pub String);

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<parser::ParseError> for LoadError {
    fn from(e: parser::ParseError) -> Self {
        LoadError(e.to_string())
    }
}

/// 解析并构建控件树。
pub fn load_str(xml: &str, ctx: &Context) -> Result<LoadResult, LoadError> {
    let el = parser::parse(xml)?;
    let mut bindings = Vec::new();
    let root = build(&el, ctx, &mut bindings)?
        .ok_or_else(|| LoadError("根节点被 v-if 求值为 false".into()))?;
    Ok(LoadResult { root, bindings })
}

fn build(el: &Element, ctx: &Context, bindings: &mut Vec<(u32, WidgetId)>) -> Result<Option<Node>, LoadError> {
    // v-if：为假则整棵子树不生成（加载期静态求值）。
    if let Some(cond) = el.attr("v-if") {
        if !expr::eval(cond, ctx) {
            return Ok(None);
        }
    }

    let tag = el.tag.to_lowercase();
    let mut node = make_node(&tag, el)?;

    // 应用属性（含分状态样式）。
    apply_attrs(node.base_mut(), &tag, &el.attrs);

    // TabBox 的 tabbar 绑定。
    if tag == "tabbox" {
        if let Some(g) = el.attr("bindgroup").and_then(|s| s.parse::<u32>().ok()) {
            bindings.push((g, node.base().id));
        }
    }

    // 递归子节点。
    for child in &el.children {
        if let Some(c) = build(child, ctx, bindings)? {
            node.base_mut().children.push(c);
        }
    }

    Ok(Some(node))
}

/// 按标签名构造控件。
fn make_node(tag: &str, el: &Element) -> Result<Node, LoadError> {
    let node: Node = match tag {
        "vbox" => Box::new(VBox::new()),
        "hbox" => Box::new(HBox::new()),
        "box" | "panel" => Box::new(Panel::new()),
        "label" => Box::new(Label::new("")),
        "button" => Box::new(Button::new("")),
        "checkbox" => Box::new(CheckBox::new("")),
        "radio" => Box::new(Radio::new("")),
        "tabbox" => Box::new(TabBox::new()),
        "edit" => Box::new(Edit::new()),
        "image" => Box::new(Image::path(el.attr("src").unwrap_or(""))),
        other => return Err(LoadError(format!("未知标签 <{other}>"))),
    };
    Ok(node)
}

/// 把属性应用到 Base（通用属性 + 分状态样式）。
fn apply_attrs(base: &mut Base, tag: &str, attrs: &[(String, String)]) {
    // 分状态样式槽临时表。
    let mut slots: HashMap<(BaseState, bool), StyleSpec> = HashMap::new();

    for (k, v) in attrs {
        let key = k.to_lowercase();
        match key.as_str() {
            // 已在别处处理的属性。
            "v-if" | "src" | "bindgroup" => {}
            "name" => base.name = Some(v.clone()),
            "text" => base.text = v.clone(),
            "width" => base.width = v.parse().ok(),
            "height" => base.height = v.parse().ok(),
            "padding" => {
                if let Ok(p) = v.parse::<f32>() {
                    base.padding = Insets::all(p);
                }
            }
            "spacing" => base.spacing = v.parse().unwrap_or(0.0),
            "flex" => base.flex_grow = v.parse().unwrap_or(0.0),
            "enabled" => base.enabled = parse_bool(v),
            "mouse" => {
                base.hit = if v.eq_ignore_ascii_case("transparent") {
                    HitPolicy::Transparent
                } else {
                    HitPolicy::Solid
                };
            }
            "group" => base.group = v.parse().ok(),
            "tab-index" | "tabindex" => base.tab_index = v.parse().ok(),
            "checked" => base.selected = parse_bool(v),
            "selected" => {
                if tag == "tabbox" {
                    base.selected_index = v.parse().unwrap_or(0);
                } else {
                    base.selected = parse_bool(v);
                }
            }
            // 其余按分状态样式属性解析。
            _ => apply_style_attr(&mut slots, &key, v),
        }
    }

    // 组装 StyleSet。
    if !slots.is_empty() {
        let mut set = StyleSet::new();
        for ((st, foc), spec) in slots {
            set.set(VisualState::new(st, foc), spec);
        }
        base.style = set;
    }
}

/// 解析形如 `[state-][focus-]prop` 的样式属性并写入对应槽。
fn apply_style_attr(slots: &mut HashMap<(BaseState, bool), StyleSpec>, key: &str, val: &str) {
    let parts: Vec<&str> = key.split('-').collect();
    let mut idx = 0;
    let mut state = BaseState::Normal;
    if let Some(s) = parse_state(parts[0]) {
        state = s;
        idx = 1;
    }
    let mut focused = false;
    if parts.get(idx) == Some(&"focus") {
        focused = true;
        idx += 1;
    }
    // 剩余部分即属性名；拼接以兼容带连字符的写法（如 border-width → borderwidth）。
    if idx >= parts.len() {
        return;
    }
    let prop = parts[idx..].join("");

    let spec = slots.entry((state, focused)).or_default();
    match prop.as_str() {
        "bgcolor" => spec.bg_color = parse_color(val),
        "fgcolor" => spec.fg_color = parse_color(val),
        "bordercolor" => spec.border_color = parse_color(val),
        "borderwidth" => spec.border_width = val.parse().ok(),
        "cornerradius" => spec.corner_radius = val.parse().ok().map(Corners::all),
        "bgimage" => spec.bg_image = Some(flexui_core::ImageSource::path(val)),
        "fgimage" => spec.fg_image = Some(flexui_core::ImageSource::path(val)),
        "textalign" | "align" => spec.text_align = parse_align(val),
        _ => {} // 未知属性忽略
    }
}

fn parse_state(s: &str) -> Option<BaseState> {
    match s {
        "normal" => Some(BaseState::Normal),
        "hot" => Some(BaseState::Hot),
        "pushed" => Some(BaseState::Pushed),
        "disabled" => Some(BaseState::Disabled),
        _ => None,
    }
}

fn parse_bool(s: &str) -> bool {
    matches!(s.to_lowercase().as_str(), "true" | "1" | "yes" | "on")
}

fn parse_align(s: &str) -> Option<TextAlign> {
    match s.to_lowercase().as_str() {
        "left" => Some(TextAlign::Left),
        "center" => Some(TextAlign::Center),
        "right" => Some(TextAlign::Right),
        _ => None,
    }
}

/// 解析 `#RGB` / `#RRGGBB` / `#AARRGGBB` 颜色。
fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    let hex = s.strip_prefix('#')?;
    let n = |a: u8, b: u8| u8::from_str_radix(&format!("{}{}", a as char, b as char), 16).ok();
    let bytes = hex.as_bytes();
    match hex.len() {
        3 => {
            let r = n(bytes[0], bytes[0])?;
            let g = n(bytes[1], bytes[1])?;
            let b = n(bytes[2], bytes[2])?;
            Some(Color::from_u8(r, g, b, 255))
        }
        6 => {
            let r = n(bytes[0], bytes[1])?;
            let g = n(bytes[2], bytes[3])?;
            let b = n(bytes[4], bytes[5])?;
            Some(Color::from_u8(r, g, b, 255))
        }
        8 => {
            let a = n(bytes[0], bytes[1])?;
            let r = n(bytes[2], bytes[3])?;
            let g = n(bytes[4], bytes[5])?;
            let b = n(bytes[6], bytes[7])?;
            Some(Color::from_u8(r, g, b, a))
        }
        _ => None,
    }
}

/// v-if 布尔表达式求值：支持 `!`、`&&`、`||`、括号、标识符（可含点）、true/false。
mod expr {
    use super::Context;

    #[derive(Debug, PartialEq)]
    enum Tok {
        Ident(String),
        Not,
        And,
        Or,
        LParen,
        RParen,
    }

    fn tokenize(s: &str) -> Vec<Tok> {
        let b = s.as_bytes();
        let mut i = 0;
        let mut out = Vec::new();
        while i < b.len() {
            let c = b[i];
            if c.is_ascii_whitespace() {
                i += 1;
            } else if c == b'!' {
                out.push(Tok::Not);
                i += 1;
            } else if c == b'(' {
                out.push(Tok::LParen);
                i += 1;
            } else if c == b')' {
                out.push(Tok::RParen);
                i += 1;
            } else if b[i..].starts_with(b"&&") {
                out.push(Tok::And);
                i += 2;
            } else if b[i..].starts_with(b"||") {
                out.push(Tok::Or);
                i += 2;
            } else if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' {
                let start = i;
                while i < b.len()
                    && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.')
                {
                    i += 1;
                }
                out.push(Tok::Ident(String::from_utf8_lossy(&b[start..i]).into_owned()));
            } else {
                i += 1; // 跳过无法识别的字符
            }
        }
        out
    }

    struct P<'a> {
        toks: Vec<Tok>,
        pos: usize,
        ctx: &'a Context,
    }

    impl<'a> P<'a> {
        fn peek(&self) -> Option<&Tok> {
            self.toks.get(self.pos)
        }
        fn or_expr(&mut self) -> bool {
            let mut v = self.and_expr();
            while matches!(self.peek(), Some(Tok::Or)) {
                self.pos += 1;
                let r = self.and_expr();
                v = v || r;
            }
            v
        }
        fn and_expr(&mut self) -> bool {
            let mut v = self.unary();
            while matches!(self.peek(), Some(Tok::And)) {
                self.pos += 1;
                let r = self.unary();
                v = v && r;
            }
            v
        }
        fn unary(&mut self) -> bool {
            if matches!(self.peek(), Some(Tok::Not)) {
                self.pos += 1;
                return !self.unary();
            }
            self.primary()
        }
        fn primary(&mut self) -> bool {
            match self.peek() {
                Some(Tok::LParen) => {
                    self.pos += 1;
                    let v = self.or_expr();
                    if matches!(self.peek(), Some(Tok::RParen)) {
                        self.pos += 1;
                    }
                    v
                }
                Some(Tok::Ident(name)) => {
                    let name = name.clone();
                    self.pos += 1;
                    self.ctx.get(&name)
                }
                _ => false,
            }
        }
    }

    /// 求值表达式。解析失败/为空时返回 false。
    pub fn eval(s: &str, ctx: &Context) -> bool {
        let toks = tokenize(s);
        if toks.is_empty() {
            return false;
        }
        let mut p = P {
            toks,
            pos: 0,
            ctx,
        };
        p.or_expr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexui_core::{layout_node, Canvas, Font, Rect, Size};

    struct FakeCanvas;
    impl Canvas for FakeCanvas {
        fn fill_rect(&mut self, _r: Rect, _c: Color) {}
        fn stroke_rect(&mut self, _r: Rect, _c: Color, _w: f32) {}
        fn fill_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color) {}
        fn stroke_round_rect(&mut self, _r: Rect, _rad: Corners, _c: Color, _w: f32) {}
        fn draw_text(&mut self, _t: &str, _o: flexui_core::Point, _f: &Font, _c: Color) {}
        fn measure_text(&self, t: &str, f: &Font) -> Size {
            Size::new(t.chars().count() as f32 * f.size * 0.6, f.size * 1.2)
        }
    }

    #[test]
    fn 颜色解析() {
        assert_eq!(parse_color("#FFFFFF"), Some(Color::from_u8(255, 255, 255, 255)));
        assert_eq!(parse_color("#F00"), Some(Color::from_u8(255, 0, 0, 255)));
        assert_eq!(parse_color("#80000000"), Some(Color::from_u8(0, 0, 0, 128)));
    }

    #[test]
    fn v_if_表达式() {
        let mut ctx = Context::new();
        ctx.set("a", true).set("b", false);
        assert!(expr::eval("a", &ctx));
        assert!(!expr::eval("b", &ctx));
        assert!(expr::eval("a && !b", &ctx));
        assert!(expr::eval("b || a", &ctx));
        assert!(expr::eval("!(b)", &ctx));
    }

    #[test]
    fn v_if_裁剪节点() {
        let xml = r#"<VBox>
            <Button v-if="show" text="A"/>
            <Button v-if="hide" text="B"/>
        </VBox>"#;
        let mut ctx = Context::new();
        ctx.set("show", true).set("hide", false);
        let res = load_str(xml, &ctx).unwrap();
        // 只有 show=true 的按钮生成
        assert_eq!(res.root.base().children.len(), 1);
    }

    #[test]
    fn 平台谓词裁剪系统按钮() {
        let xml = r#"<VBox>
            <HBox v-if="!platform.macos"><Button text="min"/><Button text="close"/></HBox>
        </VBox>"#;
        // 强制 macos=true → 该 HBox 不应生成
        let mut ctx = Context::new();
        ctx.set("platform.macos", true);
        let res = load_str(xml, &ctx).unwrap();
        assert_eq!(res.root.base().children.len(), 0, "macOS 上不渲染系统按钮组");

        // 强制 macos=false → 应生成
        let mut ctx2 = Context::new();
        ctx2.set("platform.macos", false);
        let res2 = load_str(xml, &ctx2).unwrap();
        assert_eq!(res2.root.base().children.len(), 1);
    }

    #[test]
    fn 分状态样式与_tabbar_绑定() {
        let xml = r##"<VBox spacing="10" padding="8">
            <Button text="ok" width="120" height="40"
                    normal-bgcolor="#3478F6" hot-bgcolor="#4A8CFF"
                    pushed-bgcolor="#2A5FD0" border-width="1" corner-radius="6"/>
            <Radio group="1" tab-index="0" text="t0"/>
            <TabBox bindgroup="1">
                <Panel/>
                <Panel/>
            </TabBox>
        </VBox>"##;
        let ctx = Context::new();
        let res = load_str(xml, &ctx).unwrap();
        // 布局一下确保结构可用
        let cv = FakeCanvas;
        let mut root = res.root;
        layout_node(root.as_mut(), Rect::new(0.0, 0.0, 300.0, 300.0), &cv);
        assert_eq!(root.base().children.len(), 3);
        // 绑定被记录
        assert_eq!(res.bindings.len(), 1);
        assert_eq!(res.bindings[0].0, 1);
    }
}
