//! XML → 控件树的构建（L4/L5）。对应需求 C10（XML 布局）、C11（v-if）、C12（平台谓词）。

use std::collections::HashMap;

use flexui_core::{
    Align, Base, BaseState, Button, CheckBox, Color, Corners, Edit, HBox, HitPolicy, Image, ImageFit,
    ImageSource, Insets, Justify, Label, Node, Panel, Radio, Sizing, StyleSet, StyleSpec, TabBox,
    TextAlign, TitlebarMode, VBox, VisualState, WidgetId, WindowConfig,
};
use flexui_resource::ResourceManager;

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

/// 构建环境：贯穿递归的上下文（求值上下文、资源、tabbar 绑定、Include 栈）。
struct Env<'a> {
    ctx: &'a Context,
    res: Option<&'a ResourceManager>,
    bindings: Vec<(u32, WidgetId)>,
    includes: Vec<String>,
}

/// 解析并构建控件树（图片按文件路径处理）。
pub fn load_str(xml: &str, ctx: &Context) -> Result<LoadResult, LoadError> {
    load_root(xml, ctx, None)
}

/// 经资源系统加载 skin：XML 文本与图片（src/bgimage/fgimage）+ Include 都走 ResourceManager（RM5/W7）。
pub fn load_res(res: &ResourceManager, path: &str, ctx: &Context) -> Result<LoadResult, LoadError> {
    let xml = res
        .read_string(path)
        .map_err(|e| LoadError(format!("读取 skin 失败: {e}")))?;
    load_root(&xml, ctx, Some(res))
}

/// 动态从 XML 字符串构建一个布局片段（根为普通容器，非 `<Window>`）。供代码动态 build（W8）。
pub fn build_fragment_str(xml: &str, ctx: &Context) -> Result<Node, LoadError> {
    Ok(load_str(xml, ctx)?.root)
}

/// 动态从资源路径构建布局片段（W8）。
pub fn build_fragment_res(res: &ResourceManager, path: &str, ctx: &Context) -> Result<Node, LoadError> {
    Ok(load_res(res, path, ctx)?.root)
}

/// 窗口文档：`<Window>` 根解析出的配置 + 内容树。
pub struct WindowDoc {
    /// 若 XML 根是 `<Window>`，则为其属性解析出的配置，否则 None。
    pub config: Option<WindowConfig>,
    pub root: Node,
    pub bindings: Vec<(u32, WidgetId)>,
}

/// 加载以 `<Window>` 为根的窗口 XML（W6）；根非 Window 时 config 为 None、按普通片段处理。
pub fn load_window_res(res: &ResourceManager, path: &str, ctx: &Context) -> Result<WindowDoc, LoadError> {
    let xml = res
        .read_string(path)
        .map_err(|e| LoadError(format!("读取 skin 失败: {e}")))?;
    load_window(&xml, ctx, Some(res))
}

/// 加载以 `<Window>` 为根的窗口 XML 字符串（W6）。
pub fn load_window_str(xml: &str, ctx: &Context) -> Result<WindowDoc, LoadError> {
    load_window(xml, ctx, None)
}

fn load_root(xml: &str, ctx: &Context, res: Option<&ResourceManager>) -> Result<LoadResult, LoadError> {
    let el = parser::parse(xml)?;
    let mut env = Env {
        ctx,
        res,
        bindings: Vec::new(),
        includes: Vec::new(),
    };
    let root = build(&el, &mut env)?.ok_or_else(|| LoadError("根节点被 v-if 求值为 false".into()))?;
    Ok(LoadResult {
        root,
        bindings: env.bindings,
    })
}

fn load_window(xml: &str, ctx: &Context, res: Option<&ResourceManager>) -> Result<WindowDoc, LoadError> {
    let el = parser::parse(xml)?;
    let mut env = Env {
        ctx,
        res,
        bindings: Vec::new(),
        includes: Vec::new(),
    };
    if el.tag.to_lowercase() == "window" {
        let config = parse_window_config(&el);
        // Window 的子节点即内容；多个则包进 VBox。
        let mut kids: Vec<Node> = Vec::new();
        for child in &el.children {
            if let Some(c) = build(child, &mut env)? {
                kids.push(c);
            }
        }
        let root: Node = match kids.len() {
            1 => kids.pop().unwrap(),
            _ => {
                let mut v = VBox::new();
                for k in kids {
                    v = v.push_node(k);
                }
                Box::new(v)
            }
        };
        Ok(WindowDoc {
            config: Some(config),
            root,
            bindings: env.bindings,
        })
    } else {
        let root = build(&el, &mut env)?.ok_or_else(|| LoadError("根节点被 v-if 求值为 false".into()))?;
        Ok(WindowDoc {
            config: None,
            root,
            bindings: env.bindings,
        })
    }
}

/// 从 `<Window>` 属性解析窗口配置。
fn parse_window_config(el: &Element) -> WindowConfig {
    let mut cfg = WindowConfig::new(
        el.attr("title").unwrap_or("flexui-rs"),
        el.attr("width").and_then(|s| s.parse().ok()).unwrap_or(640.0),
        el.attr("height").and_then(|s| s.parse().ok()).unwrap_or(440.0),
    );
    if let Some(r) = el.attr("resizable") {
        cfg.resizable = parse_bool(r);
    }
    if let Some(t) = el.attr("titlebar") {
        cfg.titlebar = match t.to_lowercase().as_str() {
            "hidden" | "hiddenkeepcontrols" => TitlebarMode::HiddenKeepControls,
            "none" | "borderless" => TitlebarMode::None,
            _ => TitlebarMode::System,
        };
    }
    cfg
}

/// 解析图片来源：有资源管理器则读成字节（支持 zip/内嵌），否则按文件路径。
/// `.svg` 走矢量光栅化路径。
fn resolve_image(res: Option<&ResourceManager>, path: &str) -> ImageSource {
    let is_svg = path.to_lowercase().ends_with(".svg");
    if let Some(rm) = res {
        if let Ok(bytes) = rm.read(path) {
            return if is_svg {
                ImageSource::svg(bytes)
            } else {
                ImageSource::bytes(bytes)
            };
        }
    }
    if is_svg {
        if let Ok(bytes) = std::fs::read(path) {
            return ImageSource::svg(bytes);
        }
    }
    ImageSource::path(path)
}

/// Include 最大嵌套深度（防环兜底）。
const MAX_INCLUDE_DEPTH: usize = 32;

fn build(el: &Element, env: &mut Env) -> Result<Option<Node>, LoadError> {
    // v-if：为假则整棵子树不生成（加载期静态求值）。
    if let Some(cond) = el.attr("v-if") {
        if !expr::eval(cond, env.ctx) {
            return Ok(None);
        }
    }

    let tag = el.tag.to_lowercase();

    // <Include src="逻辑路径"/>：读取子 XML 就地展开（W7），带防环。
    if tag == "include" {
        return build_include(el, env);
    }

    let mut node = make_node(&tag, el, env.res)?;
    apply_attrs(node.base_mut(), &tag, &el.attrs, env.res);

    // TabBox 的 tabbar 绑定。
    if tag == "tabbox" {
        if let Some(g) = el.attr("bindgroup").and_then(|s| s.parse::<u32>().ok()) {
            env.bindings.push((g, node.base().id));
        }
    }

    // 递归子节点。
    for child in &el.children {
        if let Some(c) = build(child, env)? {
            node.base_mut().children.push(c);
        }
    }

    Ok(Some(node))
}

/// 展开 `<Include src="...">`：经资源读取子 XML 并 build，含循环/深度防护。
fn build_include(el: &Element, env: &mut Env) -> Result<Option<Node>, LoadError> {
    let src = el
        .attr("src")
        .ok_or_else(|| LoadError("<Include> 缺少 src".into()))?
        .to_string();
    let rm = env
        .res
        .ok_or_else(|| LoadError("<Include> 需要资源管理器（用 load_res/load_window_res）".into()))?;
    if env.includes.contains(&src) {
        return Err(LoadError(format!("<Include> 循环引用: {src}")));
    }
    if env.includes.len() >= MAX_INCLUDE_DEPTH {
        return Err(LoadError("<Include> 嵌套过深".into()));
    }
    let xml = rm
        .read_string(&src)
        .map_err(|e| LoadError(format!("读取 Include {src} 失败: {e}")))?;
    let sub = parser::parse(&xml)?;
    env.includes.push(src);
    let node = build(&sub, env)?;
    env.includes.pop();
    Ok(node)
}

/// 按标签名构造控件。
fn make_node(tag: &str, el: &Element, res: Option<&ResourceManager>) -> Result<Node, LoadError> {
    let node: Node = match tag {
        "vbox" => Box::new(VBox::new()),
        "hbox" => Box::new(HBox::new()),
        "box" | "panel" => Box::new(Panel::new()),
        "label" => Box::new(Label::new("")),
        "button" => Box::new(Button::new("")),
        "checkbox" => Box::new(CheckBox::new("")),
        "radio" => Box::new(Radio::new("")),
        "tabbox" => Box::new(TabBox::new()),
        "scroll" | "scrollview" => Box::new(flexui_core::ScrollView::new()),
        "edit" => Box::new(Edit::new()),
        "image" => Box::new(Image::new(resolve_image(res, el.attr("src").unwrap_or("")))),
        other => return Err(LoadError(format!("未知标签 <{other}>"))),
    };
    Ok(node)
}

/// 把属性应用到 Base（通用属性 + 分状态样式）。
fn apply_attrs(base: &mut Base, tag: &str, attrs: &[(String, String)], res: Option<&ResourceManager>) {
    // 分状态样式槽临时表（键含 base/focus/selected 维度）。
    let mut slots: HashMap<VisualState, StyleSpec> = HashMap::new();

    for (k, v) in attrs {
        let key = k.to_lowercase();
        match key.as_str() {
            // 已在别处处理的属性。
            "v-if" | "src" | "bindgroup" => {}
            "name" => base.name = Some(v.clone()),
            "text" => base.text = v.clone(),
            "width" => base.width = parse_sizing(v),
            "height" => base.height = parse_sizing(v),
            "padding" => {
                if let Ok(p) = v.parse::<f32>() {
                    base.padding = Insets::all(p);
                }
            }
            "margin" => {
                if let Ok(p) = v.parse::<f32>() {
                    base.margin = Insets::all(p);
                }
            }
            "spacing" => base.spacing = v.parse().unwrap_or(0.0),
            "flex" => base.flex_grow = v.parse().unwrap_or(0.0),
            "x" => {
                let (_, y) = base.pos.unwrap_or((0.0, 0.0));
                base.pos = Some((v.parse().unwrap_or(0.0), y));
            }
            "y" => {
                let (x, _) = base.pos.unwrap_or((0.0, 0.0));
                base.pos = Some((x, v.parse().unwrap_or(0.0)));
            }
            "justify" => base.justify = parse_justify(v),
            "align" => base.align = parse_align_items(v),
            "enabled" => base.enabled = parse_bool(v),
            "multiline" => base.multiline = parse_bool(v),
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
            _ => apply_style_attr(&mut slots, &key, v, res),
        }
    }

    // 组装 StyleSet。
    if !slots.is_empty() {
        let mut set = StyleSet::new();
        for (vs, spec) in slots {
            set.set(vs, spec);
        }
        base.style = set;
    }
}

/// 解析形如 `[state-][focus-][selected-]prop` 的样式属性并写入对应槽。
fn apply_style_attr(
    slots: &mut HashMap<VisualState, StyleSpec>,
    key: &str,
    val: &str,
    res: Option<&ResourceManager>,
) {
    let parts: Vec<&str> = key.split('-').collect();
    // 消费前缀关键字（base/focus/selected，任意顺序）。
    let mut idx = 0;
    let mut state = BaseState::Normal;
    let mut focused = false;
    let mut selected = false;
    while idx < parts.len() {
        if let Some(s) = parse_state(parts[idx]) {
            state = s;
        } else if parts[idx] == "focus" {
            focused = true;
        } else if parts[idx] == "selected" {
            selected = true;
        } else {
            break;
        }
        idx += 1;
    }
    // 剩余部分即属性名；拼接以兼容带连字符的写法（如 border-width → borderwidth）。
    if idx >= parts.len() {
        return;
    }
    let prop = parts[idx..].join("");

    let vs = VisualState::with_selected(state, focused, selected);
    let spec = slots.entry(vs).or_default();
    match prop.as_str() {
        "bgcolor" => spec.bg_color = parse_color(val),
        "fgcolor" => spec.fg_color = parse_color(val),
        "bordercolor" => spec.border_color = parse_color(val),
        "borderwidth" => spec.border_width = val.parse().ok(),
        "cornerradius" => spec.corner_radius = val.parse().ok().map(Corners::all),
        "bgimage" => spec.bg_image = Some(resolve_image(res, val)),
        "bgtint" => spec.bg_tint = parse_color(val),
        "bgfit" => spec.bg_fit = parse_fit(val),
        "fgimage" => spec.fg_image = Some(resolve_image(res, val)),
        "fgtint" => spec.fg_tint = parse_color(val),
        "fgfit" => spec.fg_fit = parse_fit(val),
        "textalign" => spec.text_align = parse_align(val),
        _ => {} // 未知属性忽略
    }
}

/// 解析渲染方式：stretch/center/tile/ninepatch(l,t,r,b)。
fn parse_fit(v: &str) -> Option<ImageFit> {
    let s = v.trim().to_lowercase();
    if s == "stretch" {
        Some(ImageFit::Stretch)
    } else if s == "center" {
        Some(ImageFit::Center)
    } else if s == "tile" {
        Some(ImageFit::Tile)
    } else if let Some(inner) = s.strip_prefix("ninepatch") {
        // ninepatch 或 ninepatch(l,t,r,b)
        let nums: Vec<f32> = inner
            .trim_matches(|c| c == '(' || c == ')')
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect();
        let ins = match nums.len() {
            4 => Insets::new(nums[0], nums[1], nums[2], nums[3]),
            1 => Insets::all(nums[0]),
            _ => Insets::all(0.0),
        };
        Some(ImageFit::NinePatch(ins))
    } else {
        None
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

/// 解析尺寸模式：auto/content→Content，fill/stretch→Fill，数字→Fixed。
fn parse_sizing(v: &str) -> Sizing {
    match v.trim().to_lowercase().as_str() {
        "auto" | "content" => Sizing::Content,
        "fill" | "stretch" => Sizing::Fill,
        _ => v.parse::<f32>().map(Sizing::Fixed).unwrap_or(Sizing::Content),
    }
}

/// 解析主轴对齐。
fn parse_justify(v: &str) -> Justify {
    match v.to_lowercase().as_str() {
        "center" => Justify::Center,
        "end" => Justify::End,
        "space-between" | "between" => Justify::SpaceBetween,
        "space-around" | "around" => Justify::SpaceAround,
        _ => Justify::Start,
    }
}

/// 解析交叉轴对齐。
fn parse_align_items(v: &str) -> Align {
    match v.to_lowercase().as_str() {
        "center" => Align::Center,
        "end" => Align::End,
        "start" => Align::Start,
        _ => Align::Stretch,
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
    use flexui_core::{find_by_name, layout_node, Canvas, Font, Rect, Size};
    use flexui_resource::{DirProvider, ResourceManager};

    #[test]
    fn window_根解析配置() {
        let xml = r#"<Window title="演示" width="800" height="560" titlebar="hidden" resizable="false">
            <VBox><Label name="hello" text="hi"/></VBox>
        </Window>"#;
        let doc = load_window_str(xml, &Context::new()).unwrap();
        let cfg = doc.config.expect("应有窗口配置");
        assert_eq!(cfg.title, "演示");
        assert_eq!(cfg.width, 800.0);
        assert!(!cfg.resizable);
        assert_eq!(cfg.titlebar, flexui_core::TitlebarMode::HiddenKeepControls);
        // 内容根含具名控件
        assert!(find_by_name(doc.root.as_ref(), "hello").is_some());
    }

    #[test]
    fn include_子xml展开() {
        let dir = std::env::temp_dir().join(format!("flexui_inc_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("parts.xml"), r#"<Label name="fromInclude" text="子"/>"#).unwrap();
        std::fs::write(
            dir.join("main.xml"),
            r#"<VBox><Include src="parts.xml"/></VBox>"#,
        )
        .unwrap();
        let mut rm = ResourceManager::new();
        rm.mount(DirProvider::new(&dir));
        let res = load_res(&rm, "main.xml", &Context::new()).unwrap();
        assert!(find_by_name(res.root.as_ref(), "fromInclude").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_循环报错() {
        let dir = std::env::temp_dir().join(format!("flexui_cyc_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.xml"), r#"<VBox><Include src="b.xml"/></VBox>"#).unwrap();
        std::fs::write(dir.join("b.xml"), r#"<VBox><Include src="a.xml"/></VBox>"#).unwrap();
        let mut rm = ResourceManager::new();
        rm.mount(DirProvider::new(&dir));
        assert!(load_res(&rm, "a.xml", &Context::new()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_fragment_动态构建() {
        let frag = build_fragment_str(r#"<HBox><Button name="ok" text="确定"/></HBox>"#, &Context::new()).unwrap();
        assert!(find_by_name(frag.as_ref(), "ok").is_some());
    }

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
