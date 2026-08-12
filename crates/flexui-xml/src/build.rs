//! XML → 控件树的构建（L4/L5）。对应需求 C10（XML 布局）、C11（v-if）、C12（平台谓词）。

use std::collections::HashMap;

use flexui_core::{
    Align, BaseState, Button, CheckBox, Color, ComboBox, Corners, Edit, Gradient, HBox,
    HitPolicy, Image, ImageFit, ImageSource, Insets, Justify, Label, ListView, Node, Panel,
    PlaceholderStyleSet, PlaceholderStyleSpec, Progress, Radio, Rect, Separator, Shadow, Sizing, Slider, StyleSet, StyleSpec, TabBox,
    TextAlign, TitlebarMode, VBox, VisualState, Widget, WidgetId, WidgetProperty, WindowConfig,
    WindowDragRegion,
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
        let config = parse_window_config(&el)?;
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
fn parse_window_config(el: &Element) -> Result<WindowConfig, LoadError> {
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
    if let Some(v) = el.attr("system-corners") {
        cfg.system_corners = parse_bool(v);
    }
    if let Some(v) = el.attr("system-shadow") {
        cfg.system_shadow = parse_bool(v);
    }
    if let Some(v) = el.attr("drag-region") {
        cfg.drag_region = parse_drag_region(v)?;
    }
    Ok(cfg)
}

/// 解析 `<Window drag-region>`：`x y width height`、`none` 或 `platform`。
fn parse_drag_region(s: &str) -> Result<WindowDragRegion, LoadError> {
    match s.trim().to_ascii_lowercase().as_str() {
        "none" | "disabled" | "false" => Ok(WindowDragRegion::Disabled),
        "platform" | "default" | "auto" => Ok(WindowDragRegion::PlatformDefault),
        _ => {
            let values = s
                .split(|c: char| c == ',' || c.is_ascii_whitespace())
                .filter(|part| !part.is_empty())
                .map(str::parse::<f32>)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| LoadError(format!("drag-region 格式错误: {s}")))?;
            if values.len() != 4 || values[2] <= 0.0 || values[3] <= 0.0 {
                return Err(LoadError(format!(
                    "drag-region 需要 x y width height，且宽高必须大于 0: {s}"
                )));
            }
            Ok(WindowDragRegion::Rect(Rect::new(
                values[0], values[1], values[2], values[3],
            )))
        }
    }
}

/// 解析图片来源：有资源管理器则读成字节（支持 zip/内嵌），否则按文件路径。
/// `.svg` 走矢量光栅化路径。
fn resolve_image(res: Option<&ResourceManager>, path: &str) -> ImageSource {
    let is_svg = path.to_lowercase().ends_with(".svg");
    let density = flexui_core::image_density_from_path(path);
    if let Some(rm) = res {
        if let Ok(bytes) = rm.read(path) {
            return if is_svg {
                ImageSource::svg(bytes)
            } else if density != 1.0 {
                ImageSource::bytes_scaled(bytes, density)
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
    apply_attrs(node.as_mut(), &tag, &el.attrs, env.res);
    let initial_text = node.base().text.clone();
    node.set_text_value(initial_text);

    // TabBox 的 tabbar 绑定。
    if tag == "tabbox" {
        if let Some(g) = el.attr("bindgroup").and_then(|s| s.parse::<u32>().ok()) {
            env.bindings.push((g, node.base().id));
        }
    }

    // ComboBox/ListView 的 <item> 子元素已在 make_node 收进数据，不作为控件子节点。
    if matches!(tag.as_str(), "combobox" | "select" | "listview" | "list") {
        return Ok(Some(node));
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
        "progress" => Box::new(Progress::new()),
        "slider" => Box::new(Slider::new()),
        "combobox" | "select" => {
            // 选项来自 options="a,b,c" 与/或 <item text="..."/> 子元素。
            let mut opts: Vec<String> = Vec::new();
            if let Some(o) = el.attr("options") {
                opts.extend(o.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
            }
            for c in &el.children {
                if c.tag.eq_ignore_ascii_case("item") {
                    if let Some(t) = c.attr("text").or_else(|| c.attr("label")) {
                        opts.push(t.to_string());
                    }
                }
            }
            let mut cb = ComboBox::new().options(opts);
            if let Some(i) = el.attr("selected").and_then(|s| s.parse::<usize>().ok()) {
                cb = cb.selected(i);
            }
            Box::new(cb)
        }
        "listview" | "list" => {
            let mut items: Vec<String> = Vec::new();
            if let Some(o) = el.attr("items") {
                items.extend(o.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
            }
            for c in &el.children {
                if c.tag.eq_ignore_ascii_case("item") {
                    if let Some(t) = c.attr("text").or_else(|| c.attr("label")) {
                        items.push(t.to_string());
                    }
                }
            }
            let mut lv = ListView::new().items(items);
            if let Some(h) = el.attr("row-height").and_then(|s| s.parse::<f32>().ok()) {
                lv = lv.row_height(h);
            }
            if let Some(i) = el.attr("selected").and_then(|s| s.parse::<usize>().ok()) {
                lv = lv.selected(i);
            }
            Box::new(lv)
        }
        "separator" | "hr" => {
            let vertical = el
                .attr("orientation")
                .map(|o| o.eq_ignore_ascii_case("vertical"))
                .unwrap_or(false);
            let mut s = Separator::new().vertical(vertical);
            if let Some(t) = el.attr("thickness").and_then(|t| t.parse::<f32>().ok()) {
                s = s.thickness(t);
            }
            Box::new(s)
        }
        other => return Err(LoadError(format!("未知标签 <{other}>"))),
    };
    Ok(node)
}

/// 把属性应用到 Base（通用属性 + 分状态样式）。
fn apply_attrs(node: &mut dyn Widget, tag: &str, attrs: &[(String, String)], res: Option<&ResourceManager>) {
    // 分状态样式槽临时表（键含 base/focus/selected 维度）。
    let mut slots: HashMap<VisualState, StyleSpec> = HashMap::new();
    let mut placeholder_slots: HashMap<VisualState, PlaceholderStyleSpec> = HashMap::new();

    for (k, v) in attrs {
        let key = k.to_lowercase();
        match key.as_str() {
            // 已在别处处理的属性（Separator orientation/thickness、Image src、
            // ComboBox options、ListView items/row-height）。
            "v-if" | "src" | "bindgroup" | "orientation" | "thickness" | "options" | "items"
            | "row-height" => {}
            "name" => node.base_mut().name = Some(v.clone()),
            "tooltip" => node.base_mut().tooltip = Some(v.clone()),
            "placeholder" => { node.apply_property(WidgetProperty::Placeholder(v.clone())); }
            "value" => { node.apply_property(WidgetProperty::Value(v.parse::<f32>().unwrap_or(0.0))); }
            "font-size" | "fontsize" => {
                if let Ok(s) = v.parse::<f32>() {
                    node.base_mut().font.size = s;
                }
            }
            "font-family" | "font" => node.base_mut().font.family = Some(v.clone()),
            "bold" => node.base_mut().font.bold = parse_bool(v),
            "italic" => node.base_mut().font.italic = parse_bool(v),
            "underline" => node.base_mut().font.underline = parse_bool(v),
            "text" => node.base_mut().text = v.clone(),
            "width" => node.base_mut().width = parse_sizing(v),
            "height" => node.base_mut().height = parse_sizing(v),
            "padding" => {
                if let Some(p) = parse_insets(v) {
                    node.base_mut().padding = p;
                }
            }
            "margin" => {
                if let Some(m) = parse_insets(v) {
                    node.base_mut().margin = m;
                }
            }
            "spacing" => node.base_mut().spacing = v.parse().unwrap_or(0.0),
            "flex" => node.base_mut().flex_grow = v.parse().unwrap_or(0.0),
            "x" => {
                let base = node.base_mut();
                let (_, y) = base.pos.unwrap_or((0.0, 0.0));
                base.pos = Some((v.parse().unwrap_or(0.0), y));
            }
            "y" => {
                let base = node.base_mut();
                let (x, _) = base.pos.unwrap_or((0.0, 0.0));
                base.pos = Some((x, v.parse().unwrap_or(0.0)));
            }
            "justify" => node.base_mut().justify = parse_justify(v),
            "align" => node.base_mut().align = parse_align_items(v),
            "enabled" => node.base_mut().enabled = parse_bool(v),
            "visible" => node.base_mut().visible = parse_bool(v),
            "switch" => { node.apply_property(WidgetProperty::SwitchStyle(tag == "checkbox" && parse_bool(v))); }
            "multiline" => { node.apply_property(WidgetProperty::Multiline(parse_bool(v))); }
            "readonly" | "read-only" => { node.apply_property(WidgetProperty::ReadOnly(parse_bool(v))); }
            "numberonly" | "number-only" => { node.apply_property(WidgetProperty::NumberOnly(parse_bool(v))); }
            "password" => { node.apply_property(WidgetProperty::Password(parse_bool(v))); }
            "passwordchar" | "password-char" | "mask-char" => {
                if let Some(ch) = v.chars().next() { node.apply_property(WidgetProperty::PasswordChar(ch)); }
            }
            "maxchar" | "max-chars" | "max-length" => { node.apply_property(WidgetProperty::MaxChars(v.parse().ok())); }
            "autoselall" | "auto-select-all" | "select-all-on-focus" => {
                node.apply_property(WidgetProperty::AutoSelectAll(parse_bool(v)));
            }
            "mouse" => {
                node.base_mut().hit = if v.eq_ignore_ascii_case("transparent") {
                    HitPolicy::Transparent
                } else {
                    HitPolicy::Solid
                };
            }
            "group" => { node.apply_property(WidgetProperty::Group(v.parse().ok())); }
            "tab-index" | "tabindex" => { node.apply_property(WidgetProperty::TabIndex(v.parse().ok())); }
            "checked" => node.base_mut().selected = parse_bool(v),
            "selected" => {
                if matches!(tag, "tabbox" | "combobox" | "select" | "listview" | "list") {
                    node.apply_property(WidgetProperty::SelectedIndex(v.parse().unwrap_or(0)));
                } else {
                    node.base_mut().selected = parse_bool(v);
                }
            }
            // 其余按分状态样式属性解析。
            _ => if !apply_placeholder_style_attr(&mut placeholder_slots, &key, v) {
                apply_style_attr(&mut slots, &key, v, res)
            },
        }
    }

    // 组装 StyleSet。
    if !slots.is_empty() {
        let mut set = StyleSet::new();
        for (vs, spec) in slots {
            set.set(vs, spec);
        }
        node.base_mut().style = set;
    }
    if !placeholder_slots.is_empty() {
        let mut set = PlaceholderStyleSet::new();
        for (vs, spec) in placeholder_slots { set.set(vs, spec); }
        node.apply_property(WidgetProperty::PlaceholderStyle(set));
    }
}

/// 解析 `[state-][focus-][selected-]placeholder-*` 属性。
fn apply_placeholder_style_attr(slots: &mut HashMap<VisualState, PlaceholderStyleSpec>, key: &str, val: &str) -> bool {
    let parts: Vec<&str> = key.split('-').collect();
    let (mut idx, mut state, mut focused, mut selected) = (0, BaseState::Normal, false, false);
    while idx < parts.len() {
        if let Some(parsed) = parse_state(parts[idx]) { state = parsed; }
        else if parts[idx] == "focus" { focused = true; }
        else if parts[idx] == "selected" { selected = true; }
        else { break; }
        idx += 1;
    }
    if parts.get(idx) != Some(&"placeholder") || idx + 1 >= parts.len() { return false; }
    let spec = slots.entry(VisualState::with_selected(state, focused, selected)).or_default();
    match parts[idx + 1..].join("").as_str() {
        "fontfamily" | "font" => spec.font_family = Some(val.to_string()),
        "fontsize" => spec.font_size = val.parse().ok(),
        "fgcolor" | "color" => spec.fg_color = parse_color(val),
        "bold" => spec.bold = Some(parse_bool(val)),
        "italic" => spec.italic = Some(parse_bool(val)),
        "underline" => spec.underline = Some(parse_bool(val)),
        _ => return false,
    }
    true
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
        "opacity" => spec.opacity = val.parse().ok(),
        "bggradient" | "gradient" => spec.gradient = parse_gradient(val),
        "shadow" => spec.shadow = parse_shadow(val),
        _ => {} // 未知属性忽略
    }
}

/// 解析渐变："色A,色B[,h|v]"（默认竖直）。
fn parse_gradient(v: &str) -> Option<Gradient> {
    let parts: Vec<&str> = v.split(',').map(|s| s.trim()).collect();
    if parts.len() < 2 {
        return None;
    }
    let from = parse_color(parts[0])?;
    let to = parse_color(parts[1])?;
    let vertical = parts
        .get(2)
        .map(|d| !d.eq_ignore_ascii_case("h") && !d.eq_ignore_ascii_case("horizontal"))
        .unwrap_or(true);
    Some(Gradient { from, to, vertical })
}

/// 解析投影："dx dy #color"。
fn parse_shadow(v: &str) -> Option<Shadow> {
    let p: Vec<&str> = v.split_whitespace().collect();
    if p.len() < 3 {
        return None;
    }
    let dx = p[0].parse().ok()?;
    let dy = p[1].parse().ok()?;
    let color = parse_color(p[2])?;
    Some(Shadow { dx, dy, color })
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

/// 解析 CSS 风格的边距简写（空格或逗号分隔的 1/2/3/4 个数值）。
///
/// - `10`          四边都是 10
/// - `10 20`       纵(上下)=10，横(左右)=20
/// - `10 20 30`    上=10，横(左右)=20，下=30
/// - `10 20 30 40` 上=10，右=20，下=30，左=40（CSS 顺序 top right bottom left）
fn parse_insets(s: &str) -> Option<Insets> {
    let vals: Vec<f32> = s
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .map(|t| t.parse::<f32>())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some(match vals.as_slice() {
        [a] => Insets::all(*a),
        [v, h] => Insets::new(*h, *v, *h, *v),
        [t, h, b] => Insets::new(*h, *t, *h, *b),
        [t, r, b, l] => Insets::new(*l, *t, *r, *b),
        _ => return None,
    })
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
        let xml = r#"<Window title="演示" width="800" height="560" titlebar="hidden" resizable="false"
            system-corners="false" system-shadow="false" drag-region="12 8 760 36">
            <VBox><Label name="hello" text="hi"/></VBox>
        </Window>"#;
        let doc = load_window_str(xml, &Context::new()).unwrap();
        let cfg = doc.config.expect("应有窗口配置");
        assert_eq!(cfg.title, "演示");
        assert_eq!(cfg.width, 800.0);
        assert!(!cfg.resizable);
        assert_eq!(cfg.titlebar, flexui_core::TitlebarMode::HiddenKeepControls);
        assert!(!cfg.system_corners);
        assert!(!cfg.system_shadow);
        assert_eq!(
            cfg.drag_region,
            WindowDragRegion::Rect(Rect::new(12.0, 8.0, 760.0, 36.0))
        );
        // 内容根含具名控件
        assert!(find_by_name(doc.root.as_ref(), "hello").is_some());
    }

    #[test]
    fn window_拖动区域支持关闭和平台默认值() {
        for (value, expected) in [
            ("none", WindowDragRegion::Disabled),
            ("platform", WindowDragRegion::PlatformDefault),
        ] {
            let xml = format!("<Window drag-region=\"{value}\"><Panel/></Window>");
            let doc = load_window_str(&xml, &Context::new()).unwrap();
            assert_eq!(doc.config.unwrap().drag_region, expected);
        }
    }

    #[test]
    fn window_拖动区域格式错误会报错() {
        let xml = r#"<Window drag-region="0 bad 100 30"><Panel/></Window>"#;
        assert!(load_window_str(xml, &Context::new()).is_err());
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
    fn 图片资源名_保留像素密度() {
        assert!(matches!(
            resolve_image(None, "icon@2.00x.png"),
            ImageSource::ScaledPath(_, density) if density == 2.0
        ));

        let dir = std::env::temp_dir().join(format!("flexui_density_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("icon@2.00x.png"), [1, 2, 3]).unwrap();
        let mut rm = ResourceManager::new();
        rm.mount(DirProvider::new(&dir));
        assert!(matches!(
            resolve_image(Some(&rm), "icon@2.00x.png"),
            ImageSource::ScaledBytes(_, density) if density == 2.0
        ));
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
    fn 边距简写解析() {
        // 1 值：四边
        assert_eq!(parse_insets("10"), Some(Insets::all(10.0)));
        // 2 值：纵 横 → new(left=h, top=v, right=h, bottom=v)
        assert_eq!(parse_insets("10 20"), Some(Insets::new(20.0, 10.0, 20.0, 10.0)));
        // 3 值：上 横 下
        assert_eq!(parse_insets("10 20 30"), Some(Insets::new(20.0, 10.0, 20.0, 30.0)));
        // 4 值：上 右 下 左（CSS 顺序）→ new(left, top, right, bottom)
        assert_eq!(parse_insets("10 20 30 40"), Some(Insets::new(40.0, 10.0, 20.0, 30.0)));
        // 逗号分隔亦可
        assert_eq!(parse_insets("5,5"), Some(Insets::new(5.0, 5.0, 5.0, 5.0)));
        // 非法
        assert_eq!(parse_insets("a b"), None);
        assert_eq!(parse_insets(""), None);
    }

    #[test]
    fn xml_combobox_选项与tooltip() {
        let ctx = Context::new();
        // options 属性 + selected 索引 + tooltip。
        let res = load_str(r##"<combobox options="a,b,c" selected="1" tooltip="选一个"/>"##, &ctx).unwrap();
        let b = res.root.base();
        assert_eq!(b.text, "b");
        assert_eq!(res.root.selected_index(), Some(1));
        assert_eq!(b.tooltip.as_deref(), Some("选一个"));
        assert_eq!(b.children.len(), 0, "item 不作为子节点");
        // <item> 子元素形式。
        let res2 = load_str(r##"<select><item text="X"/><item label="Y"/></select>"##, &ctx).unwrap();
        assert_eq!(res2.root.base().text, "X");
        assert_eq!(res2.root.base().children.len(), 0);
    }

    #[test]
    fn xml_visible_属性控制初始可见性() {
        let mut res = load_str(
            r#"<Panel><VBox name="overlay" visible="false"><Label text="modal"/></VBox></Panel>"#,
            &Context::new(),
        )
        .unwrap();
        let overlay = find_by_name(res.root.as_ref(), "overlay").unwrap();
        let mut visible = true;
        flexui_core::visit_all_mut(res.root.as_mut(), &mut |w| {
            if w.base().id == overlay {
                visible = w.base().visible;
            }
        });
        assert!(!visible);
    }

    #[test]
    fn xml_checkbox_switch_显式启用开关外观() {
        let mut res = load_str(r#"<CheckBox switch="true"/>"#, &Context::new()).unwrap();
        assert!(res.root.apply_property(WidgetProperty::SwitchStyle(false)));
    }

    #[test]
    fn xml_listview_项与选中() {
        let ctx = Context::new();
        let res = load_str(r##"<listview items="一,二,三" selected="2" row-height="24"/>"##, &ctx).unwrap();
        let b = res.root.base();
        assert_eq!(res.root.selected_index(), Some(2));
        assert_eq!(b.children.len(), 0, "item 不作为子节点");
    }

    #[test]
    fn xml_渐变阴影透明度() {
        let ctx = Context::new();
        let xml = r##"<Box normal-bggradient="#FF0000,#0000FF" normal-shadow="0 2 #80000000" normal-opacity="0.5"/>"##;
        let res = load_str(xml, &ctx).unwrap();
        let s = res.root.base().resolved_style();
        assert_eq!(s.opacity, Some(0.5));
        let g = s.gradient.unwrap();
        assert_eq!(g.from, Color::from_u8(255, 0, 0, 255));
        assert_eq!(g.to, Color::from_u8(0, 0, 255, 255));
        assert!(g.vertical);
        let sh = s.shadow.unwrap();
        assert_eq!((sh.dx, sh.dy), (0.0, 2.0));
    }

    #[test]
    fn xml_字体样式() {
        let ctx = Context::new();
        let xml = r##"<Label text="x" bold="true" italic="true" underline="true" font-size="20" font-family="Menlo"/>"##;
        let res = load_str(xml, &ctx).unwrap();
        let f = &res.root.base().font;
        assert!(f.bold && f.italic && f.underline);
        assert_eq!(f.size, 20.0);
        assert_eq!(f.family.as_deref(), Some("Menlo"));
    }

    #[test]
    fn xml_edit_placeholder_base_and_state_font_styles() {
        let xml = r##"<Edit placeholder="请输入手机号" placeholder-font-family="Microsoft YaHei"
            placeholder-font-size="14" placeholder-fgcolor="#9095BB" placeholder-bold="true"
            placeholder-italic="false" placeholder-underline="true" hot-placeholder-font-size="16"
            hot-placeholder-fgcolor="#FFFFFF" focus-placeholder-italic="true" disabled-placeholder-bold="false"/>"##;
        let res = load_str(xml, &Context::new()).unwrap();
        assert!(res.root.text_input_state().is_some());
    }

    #[test]
    fn xml_edit输入行为属性() {
        let xml = r#"<Edit text="12a34" readonly="true" number-only="true"
            password="true" password-char="*" max-length="3" auto-select-all="true"/>"#;
        let mut root = load_str(xml, &Context::new()).unwrap().root;
        let state = root.text_input_state().unwrap();
        assert_eq!(state.text, "123");
        assert_eq!(state.cursor, 3);
        assert!(!root.replace_selection("9"), "readonly 应阻止修改");
    }

    #[test]
    fn xml_新控件_progress_slider_separator() {
        let ctx = Context::new();
        let xml = r##"<VBox>
            <Progress value="0.6"/>
            <Slider value="0.25"/>
            <Separator orientation="vertical" thickness="2"/>
        </VBox>"##;
        let res = load_str(xml, &ctx).unwrap();
        let ch = &res.root.base().children;
        assert_eq!(ch.len(), 3);
        assert!((ch[0].animation_value(flexui_core::AnimProp::Value).unwrap() - 0.6).abs() < 1e-3);
        assert!((ch[1].animation_value(flexui_core::AnimProp::Value).unwrap() - 0.25).abs() < 1e-3);
        // 纵向分隔条：宽固定 2。
        assert_eq!(ch[2].base().width, Sizing::Fixed(2.0));
    }

    #[test]
    fn xml_padding_margin_简写() {
        let ctx = Context::new();
        let xml = r##"<Box padding="4 8" margin="1 2 3 4"/>"##;
        let res = load_str(xml, &ctx).unwrap();
        let b = res.root.base();
        assert_eq!(b.padding, Insets::new(8.0, 4.0, 8.0, 4.0));
        assert_eq!(b.margin, Insets::new(4.0, 1.0, 2.0, 3.0));
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
