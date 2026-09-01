//! XML → 控件树的构建（L4/L5）。对应需求 C10（XML 布局）、C11（v-if）、C12（平台谓词）。

use std::collections::HashMap;
use std::rc::Rc;

use flexui_core::{
    Align, BaseState, Button, CheckBox, Color, ComboBox, Corners, Edit, FrameAnimation,
    FrameFinish, FramePlayback, Gradient, HBox, HitPolicy, Image, ImageFit, ImageSource, Insets,
    Justify, Label, ListView, Node, Panel, PlaceholderStyleSet, PlaceholderStyleSpec, Progress,
    Radio, Rect, ScrollBarVisibility, Separator, Shadow, Sizing, Slider, StyleSet, StyleSpec,
    Switch, TabBox, TextAlign, ThemeColorBinding, ThemeColorProperty, TitlebarMode, VBox,
    VirtualColumn, VirtualList, VirtualListRow, VirtualListRows, VirtualSelectionMode, VisualState,
    Widget, WidgetId, WidgetProperty, WindowConfig, WindowDragRegion,
};
use flexui_i18n::{LocalizationValue, LocalizedStringResource, Localizer};
use flexui_resource::ResourceManager;

use crate::parser::{self, Element};

/// v-if 求值上下文：变量表 + 内置平台谓词。
pub struct Context {
    vars: HashMap<String, bool>,
    localizer: Option<Localizer>,
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;

impl Context {
    /// 新建上下文，按当前编译目标注入三个支持平台的谓词。
    pub fn new() -> Self {
        let mut vars = HashMap::new();
        vars.insert("platform.macos".to_string(), cfg!(target_os = "macos"));
        vars.insert("platform.windows".to_string(), cfg!(target_os = "windows"));
        vars.insert("platform.linux".to_string(), cfg!(target_os = "linux"));
        Self {
            vars,
            localizer: None,
        }
    }

    /// 设置/覆盖一个布尔变量（也可覆盖平台谓词用于测试）。
    pub fn set(&mut self, key: impl Into<String>, val: bool) -> &mut Self {
        self.vars.insert(key.into(), val);
        self
    }

    /// 注入 SwiftUI Environment 风格的本地化环境。
    pub fn set_localizer(&mut self, localizer: Localizer) -> &mut Self {
        self.localizer = Some(localizer);
        self
    }

    pub fn localizer(&self) -> Option<&Localizer> {
        self.localizer.as_ref()
    }

    pub(crate) fn get(&self, key: &str) -> bool {
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
pub fn build_fragment_res(
    res: &ResourceManager,
    path: &str,
    ctx: &Context,
) -> Result<Node, LoadError> {
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
pub fn load_window_res(
    res: &ResourceManager,
    path: &str,
    ctx: &Context,
) -> Result<WindowDoc, LoadError> {
    let xml = res
        .read_string(path)
        .map_err(|e| LoadError(format!("读取 skin 失败: {e}")))?;
    load_window(&xml, ctx, Some(res))
}

/// 加载以 `<Window>` 为根的窗口 XML 字符串（W6）。
pub fn load_window_str(xml: &str, ctx: &Context) -> Result<WindowDoc, LoadError> {
    load_window(xml, ctx, None)
}

fn load_root(
    xml: &str,
    ctx: &Context,
    res: Option<&ResourceManager>,
) -> Result<LoadResult, LoadError> {
    let el = parser::parse(xml)?;
    let mut env = Env {
        ctx,
        res,
        bindings: Vec::new(),
        includes: Vec::new(),
    };
    let root =
        build(&el, &mut env)?.ok_or_else(|| LoadError("根节点被 v-if 求值为 false".into()))?;
    Ok(LoadResult {
        root,
        bindings: env.bindings,
    })
}

fn load_window(
    xml: &str,
    ctx: &Context,
    res: Option<&ResourceManager>,
) -> Result<WindowDoc, LoadError> {
    let el = parser::parse(xml)?;
    let mut env = Env {
        ctx,
        res,
        bindings: Vec::new(),
        includes: Vec::new(),
    };
    if el.tag.to_lowercase() == "window" {
        let config = parse_window_config(&el, env.ctx.localizer())?;
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
        let root =
            build(&el, &mut env)?.ok_or_else(|| LoadError("根节点被 v-if 求值为 false".into()))?;
        Ok(WindowDoc {
            config: None,
            root,
            bindings: env.bindings,
        })
    }
}

/// 从 `<Window>` 属性解析窗口配置。
fn parse_window_config(
    el: &Element,
    localizer: Option<&Localizer>,
) -> Result<WindowConfig, LoadError> {
    let (title, localized_title) = if let Some(title) = el.attr("title-verbatim") {
        (title.to_owned(), None)
    } else {
        resolve_localized(
            el.attr("title").unwrap_or("flexui-rs"),
            el.attr("title-args"),
            localizer,
        )
    };
    let mut cfg = WindowConfig::new(
        title,
        el.attr("width")
            .and_then(|s| s.parse().ok())
            .unwrap_or(640.0),
        el.attr("height")
            .and_then(|s| s.parse().ok())
            .unwrap_or(440.0),
    );
    cfg.localized_title = localized_title;
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
pub(crate) fn resolve_image(res: Option<&ResourceManager>, path: &str) -> ImageSource {
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

    let mut node = make_node(&tag, el, env)?;
    apply_attrs(node.as_mut(), &tag, &el.attrs, env)?;
    let initial_text = node.base().text.clone();
    node.set_text_value(initial_text);

    // TabBox 的 tabbar 绑定。
    if tag == "tabbox" {
        if let Some(g) = el.attr("bindgroup").and_then(|s| s.parse::<u32>().ok()) {
            node.apply_property(WidgetProperty::BindGroup(Some(g)));
            env.bindings.push((g, node.base().id));
        }
    }

    // 数据控件的元数据子元素已在 make_node 收进数据，不作为控件子节点。
    if matches!(
        tag.as_str(),
        "combobox" | "select" | "listview" | "list" | "virtuallist" | "virtual-list"
    ) {
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
    let rm = env.res.ok_or_else(|| {
        LoadError("<Include> 需要资源管理器（用 load_res/load_window_res）".into())
    })?;
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
fn make_node(tag: &str, el: &Element, env: &Env) -> Result<Node, LoadError> {
    let res = env.res;
    let node: Node = match tag {
        "vbox" => Box::new(VBox::new()),
        "hbox" => Box::new(HBox::new()),
        "box" | "panel" => Box::new(Panel::new()),
        "label" => Box::new(Label::new("")),
        "button" => Box::new(Button::new("")),
        "checkbox" => Box::new(CheckBox::new("")),
        "switch" => Box::new(Switch::new()),
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
            let mut resources: Vec<LocalizedStringResource> = Vec::new();
            let mut has_binding = false;
            if let Some(o) = el.attr("options") {
                for value in o
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    push_localized_item(
                        &mut opts,
                        &mut resources,
                        &mut has_binding,
                        value,
                        el.attr("options-args"),
                        env.ctx.localizer(),
                    );
                }
            }
            for c in &el.children {
                if c.tag.eq_ignore_ascii_case("item") {
                    if let Some(value) =
                        c.attr("text-verbatim").or_else(|| c.attr("label-verbatim"))
                    {
                        opts.push(value.to_owned());
                        resources.push(verbatim_resource(value));
                    } else if let Some(value) = c.attr("text").or_else(|| c.attr("label")) {
                        push_localized_item(
                            &mut opts,
                            &mut resources,
                            &mut has_binding,
                            value,
                            c.attr("args"),
                            env.ctx.localizer(),
                        );
                    }
                }
            }
            let mut cb = ComboBox::new().options(opts);
            if let Some(i) = el.attr("selected").and_then(|s| s.parse::<usize>().ok()) {
                cb = cb.selected(i);
            }
            let mut node: Node = Box::new(cb);
            if has_binding {
                node.base_mut()
                    .localizations
                    .push(flexui_core::LocalizationBinding::Items(resources));
            }
            node
        }
        "listview" | "list" => {
            let mut items: Vec<String> = Vec::new();
            let mut resources: Vec<LocalizedStringResource> = Vec::new();
            let mut has_binding = false;
            if let Some(o) = el.attr("items") {
                for value in o
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    push_localized_item(
                        &mut items,
                        &mut resources,
                        &mut has_binding,
                        value,
                        el.attr("items-args"),
                        env.ctx.localizer(),
                    );
                }
            }
            for c in &el.children {
                if c.tag.eq_ignore_ascii_case("item") {
                    if let Some(value) =
                        c.attr("text-verbatim").or_else(|| c.attr("label-verbatim"))
                    {
                        items.push(value.to_owned());
                        resources.push(verbatim_resource(value));
                    } else if let Some(value) = c.attr("text").or_else(|| c.attr("label")) {
                        push_localized_item(
                            &mut items,
                            &mut resources,
                            &mut has_binding,
                            value,
                            c.attr("args"),
                            env.ctx.localizer(),
                        );
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
            let mut node: Node = Box::new(lv);
            if has_binding {
                node.base_mut()
                    .localizations
                    .push(flexui_core::LocalizationBinding::Items(resources));
            }
            node
        }
        "virtuallist" | "virtual-list" => {
            let columns = el
                .children
                .iter()
                .filter(|child| child.tag.eq_ignore_ascii_case("column"))
                .enumerate()
                .map(|(index, child)| parse_virtual_column(child, index))
                .collect::<Vec<_>>();
            let rows = el
                .children
                .iter()
                .filter(|child| child.tag.eq_ignore_ascii_case("row"))
                .enumerate()
                .map(|(index, child)| {
                    let id = child
                        .attr("id")
                        .and_then(|value| value.parse::<u64>().ok())
                        .unwrap_or(index as u64 + 1);
                    let values = child
                        .attr("values")
                        .unwrap_or("")
                        .split('|')
                        .map(str::trim)
                        .collect::<Vec<_>>();
                    let mut row = VirtualListRow::new(id);
                    for (column_index, column) in columns.iter().enumerate() {
                        let value = child
                            .attr(&column.key)
                            .or_else(|| values.get(column_index).copied())
                            .unwrap_or("");
                        row = row.cell(column.key.clone(), value);
                    }
                    row
                })
                .collect::<Vec<_>>();
            let source = Rc::new(VirtualListRows::from_rows(rows));
            let mut list = VirtualList::new().columns(columns).source(source);
            if let Some(value) = el.attr("row-height").and_then(|value| value.parse().ok()) {
                list = list.row_height(value);
            }
            if let Some(value) = el
                .attr("header-height")
                .and_then(|value| value.parse().ok())
            {
                list = list.header_height(value);
            }
            if let Some(value) = el.attr("show-header") {
                list = list.show_header(parse_bool(value));
            }
            if let Some(value) = el.attr("striped") {
                list = list.striped(parse_bool(value));
            }
            if let Some(value) = el.attr("fill-last-column") {
                list = list.fill_last_column(parse_bool(value));
            }
            if let Some(value) = el.attr("overscan").and_then(|value| value.parse().ok()) {
                list = list.overscan(value);
            }
            if let Some(value) = el.attr("selection-mode") {
                list = list.selection_mode(parse_virtual_selection_mode(value));
            }
            if let Some(index) = el.attr("selected").and_then(|value| value.parse().ok()) {
                list.set_selected_index(index);
            }
            Box::new(list)
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
fn apply_attrs(
    node: &mut dyn Widget,
    tag: &str,
    attrs: &[(String, String)],
    env: &Env,
) -> Result<(), LoadError> {
    let res = env.res;
    // 分状态样式槽临时表（键含 base/focus/selected 维度）。
    let mut slots: HashMap<VisualState, StyleSpec> = HashMap::new();
    let mut placeholder_slots: HashMap<VisualState, PlaceholderStyleSpec> = HashMap::new();
    let mut theme_colors = Vec::new();

    for (k, v) in attrs {
        let key = k.to_lowercase();
        match key.as_str() {
            // 已在别处处理的属性（Separator orientation/thickness、Image src、
            // ComboBox options、ListView items/row-height）。
            "v-if" | "src" | "bindgroup" | "orientation" | "thickness" | "options" | "items"
            | "options-args" | "items-args" | "row-height" | "header-height" | "show-header"
            | "striped" | "fill-last-column" | "overscan" | "selection-mode" | "text-args"
            | "placeholder-args" | "tooltip-args" | "title-args" => {}
            "name" => node.base_mut().name = Some(v.clone()),
            "variant" => node.base_mut().variant = v.trim().to_owned(),
            "class" | "classes" => {
                node.base_mut().classes = v
                    .split_whitespace()
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            "tooltip" if attr_value(attrs, "tooltip-verbatim").is_none() => {
                let (value, binding) =
                    resolve_localized(v, attr_value(attrs, "tooltip-args"), env.ctx.localizer());
                node.base_mut().tooltip = Some(value);
                if let Some(resource) = binding {
                    node.base_mut()
                        .localizations
                        .push(flexui_core::LocalizationBinding::Tooltip(resource));
                }
            }
            "tooltip" => {}
            "tooltip-verbatim" => node.base_mut().tooltip = Some(v.clone()),
            "placeholder" if attr_value(attrs, "placeholder-verbatim").is_none() => {
                let (value, binding) = resolve_localized(
                    v,
                    attr_value(attrs, "placeholder-args"),
                    env.ctx.localizer(),
                );
                node.apply_property(WidgetProperty::Placeholder(value));
                if let Some(resource) = binding {
                    node.base_mut()
                        .localizations
                        .push(flexui_core::LocalizationBinding::Placeholder(resource));
                }
            }
            "placeholder" => {}
            "placeholder-verbatim" => {
                node.apply_property(WidgetProperty::Placeholder(v.clone()));
            }
            "value" => {
                node.apply_property(WidgetProperty::Value(v.parse::<f32>().unwrap_or(0.0)));
            }
            "font-size" | "fontsize" => {
                if let Ok(s) = v.parse::<f32>() {
                    node.base_mut().font.size = s;
                }
            }
            "font-family" | "font" => node.base_mut().font.family = Some(v.clone()),
            "bold" => node.base_mut().font.bold = parse_bool(v),
            "italic" => node.base_mut().font.italic = parse_bool(v),
            "underline" => node.base_mut().font.underline = parse_bool(v),
            "text" if attr_value(attrs, "text-verbatim").is_none() => {
                let (value, binding) =
                    resolve_localized(v, attr_value(attrs, "text-args"), env.ctx.localizer());
                node.base_mut().text = value;
                if let Some(resource) = binding {
                    node.base_mut()
                        .localizations
                        .push(flexui_core::LocalizationBinding::Text(resource));
                }
            }
            "text" => {}
            "text-verbatim" => node.base_mut().text = v.clone(),
            "icon" if tag == "button" => {
                node.apply_property(WidgetProperty::Icon(Some(resolve_image(res, v))));
            }
            "icon-rect" | "iconrect" if tag == "button" => {
                node.apply_property(WidgetProperty::IconRect(Some(parse_local_rect(
                    v,
                    "icon-rect",
                )?)));
            }
            "text-rect" | "textrect" if tag == "button" => {
                node.apply_property(WidgetProperty::TextRect(Some(parse_local_rect(
                    v,
                    "text-rect",
                )?)));
            }
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
            "selectable" => {
                let on = parse_bool(v);
                node.base_mut().selectable = on;
                if on {
                    node.base_mut().focusable = true;
                }
            }
            "focusable" | "tabstop" => node.base_mut().focusable = parse_bool(v),
            "focus-within" | "focuswithin" => node.base_mut().focus_within = parse_bool(v),
            "indicator" | "show-indicator" => {
                node.apply_property(WidgetProperty::IndicatorVisible(parse_bool(v)));
            }
            "multiline" => {
                node.apply_property(WidgetProperty::Multiline(parse_bool(v)));
            }
            "readonly" | "read-only" => {
                node.apply_property(WidgetProperty::ReadOnly(parse_bool(v)));
            }
            "numberonly" | "number-only" => {
                node.apply_property(WidgetProperty::NumberOnly(parse_bool(v)));
            }
            "password" => {
                node.apply_property(WidgetProperty::Password(parse_bool(v)));
            }
            "passwordchar" | "password-char" | "mask-char" => {
                if let Some(ch) = v.chars().next() {
                    node.apply_property(WidgetProperty::PasswordChar(ch));
                }
            }
            "maxchar" | "max-chars" | "max-length" => {
                node.apply_property(WidgetProperty::MaxChars(v.parse().ok()));
            }
            "wrap-width" | "wrapwidth" | "wrap" => {
                node.apply_property(WidgetProperty::WrapWidth(v.trim().parse().ok()));
            }
            "autoselall" | "auto-select-all" | "select-all-on-focus" => {
                node.apply_property(WidgetProperty::AutoSelectAll(parse_bool(v)));
            }
            "autoscroll" | "auto-scroll" | "follow-tail" | "stick-to-bottom" => {
                node.apply_property(WidgetProperty::AutoScroll(parse_bool(v)));
            }
            "scrollbar" | "scroll-bar" | "scrollbars" => {
                node.apply_property(WidgetProperty::ScrollBar(parse_scrollbar_visibility(v)));
            }
            "mouse" => {
                node.base_mut().hit = if v.eq_ignore_ascii_case("transparent") {
                    HitPolicy::Transparent
                } else {
                    HitPolicy::Solid
                };
            }
            "group" => {
                node.apply_property(WidgetProperty::Group(v.parse().ok()));
            }
            "tab-index" | "tabindex" => {
                node.apply_property(WidgetProperty::TabIndex(v.parse().ok()));
            }
            "checked" => node.base_mut().selected = parse_bool(v),
            "selected" => {
                if matches!(
                    tag,
                    "tabbox"
                        | "combobox"
                        | "select"
                        | "listview"
                        | "list"
                        | "virtuallist"
                        | "virtual-list"
                ) {
                    node.apply_property(WidgetProperty::SelectedIndex(v.parse().unwrap_or(0)));
                } else {
                    node.base_mut().selected = parse_bool(v);
                }
            }
            // 其余按分状态样式属性解析。
            _ => {
                if let Some(binding) = parse_placeholder_theme_binding(&key, v) {
                    theme_colors.push(binding);
                } else if !apply_placeholder_style_attr(&mut placeholder_slots, &key, v) {
                    apply_style_attr(&mut slots, &mut theme_colors, &key, v, res)
                }
            }
        }
    }

    apply_frame_animation_attrs(node, attrs, &mut slots, res)?;

    // 组装 StyleSet。
    if !slots.is_empty() {
        let mut set = StyleSet::new();
        for (vs, spec) in slots {
            set.set(vs, spec);
        }
        node.base_mut().style = set;
    }
    node.base_mut().theme_colors = theme_colors;
    if !placeholder_slots.is_empty() {
        let mut set = PlaceholderStyleSet::new();
        for (vs, spec) in placeholder_slots {
            set.set(vs, spec);
        }
        node.apply_property(WidgetProperty::PlaceholderStyle(set));
    }
    Ok(())
}

fn parse_local_rect(value: &str, property: &str) -> Result<Rect, LoadError> {
    let values = value
        .split([',', ' '])
        .filter(|part| !part.is_empty())
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| LoadError(format!("{property} 格式错误: {value}")))?;
    if values.len() != 4 || values[2] < 0.0 || values[3] < 0.0 {
        return Err(LoadError(format!(
            "{property} 需要 x y width height，且宽高不能为负数: {value}"
        )));
    }
    Ok(Rect::new(values[0], values[1], values[2], values[3]))
}

#[derive(Clone, Copy)]
enum FrameTarget {
    State(VisualState, FrameImageLayer),
    Click(FrameImageLayer),
}

#[derive(Clone, Copy)]
enum FrameImageLayer {
    Background,
    Foreground,
}

/// 解析帧动画主属性。附属的 fps/interval/play/finish 属性按相同前缀读取，故不依赖属性顺序。
fn apply_frame_animation_attrs(
    node: &mut dyn Widget,
    attrs: &[(String, String)],
    slots: &mut HashMap<VisualState, StyleSpec>,
    res: Option<&ResourceManager>,
) -> Result<(), LoadError> {
    for (key, pattern) in attrs {
        let key = key.to_ascii_lowercase();
        let Some((target, prefix)) = parse_frame_target(&key) else {
            continue;
        };
        let frames = expand_frame_pattern(pattern)?
            .into_iter()
            .map(|path| resolve_image(res, &path))
            .collect::<Vec<_>>();
        let fps = attr_value(attrs, &format!("{prefix}-fps"))
            .map(|value| {
                value
                    .parse::<f32>()
                    .ok()
                    .filter(|fps| fps.is_finite() && *fps > 0.0)
                    .ok_or_else(|| LoadError(format!("{prefix}-fps 必须是大于 0 的数字: {value}")))
            })
            .transpose()?
            .unwrap_or(25.0);
        let default_playback = if matches!(target, FrameTarget::Click(_)) {
            FramePlayback::Once
        } else {
            FramePlayback::Loop
        };
        let playback = attr_value(attrs, &format!("{prefix}-play"))
            .map(parse_frame_playback)
            .transpose()?
            .unwrap_or(default_playback);
        let finish = attr_value(attrs, &format!("{prefix}-finish"))
            .map(parse_frame_finish)
            .transpose()?
            .unwrap_or_default();
        let interval_secs = attr_value(attrs, &format!("{prefix}-interval"))
            .map(|value| {
                value
                    .parse::<f32>()
                    .ok()
                    .filter(|ms| ms.is_finite() && *ms >= 0.0)
                    .map(|ms| ms / 1000.0)
                    .ok_or_else(|| {
                        LoadError(format!("{prefix}-interval 必须是非负毫秒数: {value}"))
                    })
            })
            .transpose()?
            .unwrap_or(0.0);
        let animation = FrameAnimation::new(frames, fps)
            .loop_interval(interval_secs)
            .playback(playback)
            .finish(finish);
        match target {
            FrameTarget::State(state, FrameImageLayer::Background) => {
                slots.entry(state).or_default().bg_animation = Some(animation);
            }
            FrameTarget::State(state, FrameImageLayer::Foreground) => {
                slots.entry(state).or_default().fg_animation = Some(animation);
            }
            FrameTarget::Click(FrameImageLayer::Background) => {
                node.base_mut().click_bg_animation = Some(animation)
            }
            FrameTarget::Click(FrameImageLayer::Foreground) => {
                node.base_mut().click_fg_animation = Some(animation)
            }
        }
    }
    Ok(())
}

fn parse_frame_target(key: &str) -> Option<(FrameTarget, String)> {
    let parts: Vec<&str> = key.split('-').collect();
    let (mut idx, mut state, mut focused, mut selected, mut click) =
        (0, BaseState::Normal, false, false, false);
    while idx < parts.len() {
        if let Some(parsed) = parse_state(parts[idx]) {
            state = parsed;
        } else if parts[idx] == "focus" {
            focused = true;
        } else if parts[idx] == "selected" {
            selected = true;
        } else if parts[idx] == "click" {
            click = true;
        } else {
            break;
        }
        idx += 1;
    }
    if idx + 1 != parts.len() {
        return None;
    }
    let layer = match parts[idx] {
        "bgframes" => FrameImageLayer::Background,
        "fgframes" => FrameImageLayer::Foreground,
        _ => return None,
    };
    let target = if click {
        if state != BaseState::Normal || focused || selected {
            return None;
        }
        FrameTarget::Click(layer)
    } else {
        FrameTarget::State(VisualState::with_selected(state, focused, selected), layer)
    };
    Some((target, key.to_owned()))
}

fn parse_frame_playback(value: &str) -> Result<FramePlayback, LoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "loop" => Ok(FramePlayback::Loop),
        "once" => Ok(FramePlayback::Once),
        "paused" | "pause" => Ok(FramePlayback::Paused),
        _ => Err(LoadError(format!(
            "帧动画 play 只支持 loop/once/paused: {value}"
        ))),
    }
}

fn parse_frame_finish(value: &str) -> Result<FrameFinish, LoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "restore" => Ok(FrameFinish::Restore),
        "first" => Ok(FrameFinish::First),
        "last" => Ok(FrameFinish::Last),
        _ => Err(LoadError(format!(
            "帧动画 finish 只支持 restore/first/last: {value}"
        ))),
    }
}

/// 展开 `path/frame_{1..25}.png`，端点有前导零时保持补零宽度。
fn expand_frame_pattern(pattern: &str) -> Result<Vec<String>, LoadError> {
    let open = pattern
        .find('{')
        .ok_or_else(|| LoadError(format!("帧动画路径需要 {{start..end}} 范围: {pattern}")))?;
    let close = pattern[open + 1..]
        .find('}')
        .map(|offset| open + 1 + offset)
        .ok_or_else(|| LoadError(format!("帧动画路径缺少 }}: {pattern}")))?;
    if pattern[close + 1..].contains(['{', '}']) || pattern[..open].contains('}') {
        return Err(LoadError(format!("帧动画路径只支持一个范围: {pattern}")));
    }
    let range = &pattern[open + 1..close];
    let (start_text, end_text) = range
        .split_once("..")
        .ok_or_else(|| LoadError(format!("帧动画范围格式应为 {{start..end}}: {pattern}")))?;
    if start_text.is_empty() || end_text.is_empty() || end_text.contains("..") {
        return Err(LoadError(format!(
            "帧动画范围格式应为 {{start..end}}: {pattern}"
        )));
    }
    let start = start_text
        .parse::<i32>()
        .map_err(|_| LoadError(format!("帧动画起始序号无效: {start_text}")))?;
    let end = end_text
        .parse::<i32>()
        .map_err(|_| LoadError(format!("帧动画结束序号无效: {end_text}")))?;
    let count = start.abs_diff(end) as usize + 1;
    if count > 10_000 {
        return Err(LoadError(format!(
            "帧动画范围过大（最多 10000 帧）: {pattern}"
        )));
    }
    let padded = (start_text.starts_with('0') && start_text.len() > 1)
        || (end_text.starts_with('0') && end_text.len() > 1);
    let width = start_text.len().max(end_text.len());
    let (prefix, suffix) = (&pattern[..open], &pattern[close + 1..]);
    let step = if start <= end { 1 } else { -1 };
    Ok((0..count)
        .map(|index| {
            let value = start + step * index as i32;
            let number = if padded {
                format!("{value:0width$}", width = width)
            } else {
                value.to_string()
            };
            format!("{prefix}{number}{suffix}")
        })
        .collect())
}

fn attr_value<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.as_str())
}

/// 目录中存在的普通字符串自动作为 key；`loc:key` 强制绑定，verbatim 属性强制字面量。
pub(crate) fn resolve_localized(
    value: &str,
    args: Option<&str>,
    localizer: Option<&Localizer>,
) -> (String, Option<LocalizedStringResource>) {
    let (key, forced) = value
        .strip_prefix("loc:")
        .map_or((value, false), |key| (key, true));
    let Some(localizer) = localizer else {
        return (key.to_owned(), None);
    };
    if !forced && !localizer.contains(key, "Localizable") {
        return (value.to_owned(), None);
    }
    let mut resource = LocalizedStringResource::new(key);
    if let Some(args) = args {
        for pair in args.split([',', ';']) {
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let value = value.trim();
            let value = value
                .parse::<f64>()
                .map(LocalizationValue::Number)
                .unwrap_or_else(|_| LocalizationValue::String(value.to_owned()));
            resource.arguments.insert(name.trim().to_owned(), value);
        }
    }
    (localizer.text(resource.clone()), Some(resource))
}

/// 解析列表项，并为整个列表保留可在运行时重新求值的资源描述。
fn push_localized_item(
    values: &mut Vec<String>,
    resources: &mut Vec<LocalizedStringResource>,
    has_binding: &mut bool,
    value: &str,
    args: Option<&str>,
    localizer: Option<&Localizer>,
) {
    let (resolved, binding) = resolve_localized(value, args, localizer);
    values.push(resolved);
    if let Some(resource) = binding {
        *has_binding = true;
        resources.push(resource);
    } else {
        resources.push(verbatim_resource(value));
    }
}

/// 用不会与正常目录冲突的私有 table 表示列表中的字面量。
fn verbatim_resource(value: &str) -> LocalizedStringResource {
    LocalizedStringResource::new(value)
        .table("__flexui_verbatim")
        .default_value(value)
}

/// 解析 `[state-][focus-][selected-]placeholder-*` 属性。
fn apply_placeholder_style_attr(
    slots: &mut HashMap<VisualState, PlaceholderStyleSpec>,
    key: &str,
    val: &str,
) -> bool {
    let parts: Vec<&str> = key.split('-').collect();
    let (mut idx, mut state, mut focused, mut selected) = (0, BaseState::Normal, false, false);
    while idx < parts.len() {
        if let Some(parsed) = parse_state(parts[idx]) {
            state = parsed;
        } else if parts[idx] == "focus" {
            focused = true;
        } else if parts[idx] == "selected" {
            selected = true;
        } else {
            break;
        }
        idx += 1;
    }
    if parts.get(idx) != Some(&"placeholder") || idx + 1 >= parts.len() {
        return false;
    }
    let spec = slots
        .entry(VisualState::with_selected(state, focused, selected))
        .or_default();
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

fn parse_placeholder_theme_binding(key: &str, value: &str) -> Option<ThemeColorBinding> {
    let token = value.trim().strip_prefix('@')?;
    let parts: Vec<&str> = key.split('-').collect();
    let (mut idx, mut state, mut focused, mut selected) = (0, BaseState::Normal, false, false);
    while idx < parts.len() {
        if let Some(parsed) = parse_state(parts[idx]) {
            state = parsed;
        } else if parts[idx] == "focus" {
            focused = true;
        } else if parts[idx] == "selected" {
            selected = true;
        } else {
            break;
        }
        idx += 1;
    }
    (parts.get(idx) == Some(&"placeholder")
        && matches!(parts[idx + 1..].join("").as_str(), "fgcolor" | "color"))
    .then(|| ThemeColorBinding {
        state: VisualState::with_selected(state, focused, selected),
        property: ThemeColorProperty::Placeholder,
        token: token.to_owned(),
    })
}

/// 解析形如 `[state-][focus-][selected-]prop` 的样式属性并写入对应槽。
fn apply_style_attr(
    slots: &mut HashMap<VisualState, StyleSpec>,
    theme_colors: &mut Vec<ThemeColorBinding>,
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
    if let Some(token) = val.trim().strip_prefix('@') {
        let property = match prop.as_str() {
            "bgcolor" => Some(ThemeColorProperty::Background),
            "fgcolor" => Some(ThemeColorProperty::Foreground),
            "bordercolor" => Some(ThemeColorProperty::Border),
            "bgtint" => Some(ThemeColorProperty::BackgroundTint),
            "fgtint" => Some(ThemeColorProperty::ForegroundTint),
            "accentcolor" => Some(ThemeColorProperty::Accent),
            "thumbcolor" => Some(ThemeColorProperty::Thumb),
            "trackcolor" => Some(ThemeColorProperty::Track),
            "selectioncolor" => Some(ThemeColorProperty::Selection),
            "scrollbarcolor" => Some(ThemeColorProperty::Scrollbar),
            "placeholdercolor" => Some(ThemeColorProperty::Placeholder),
            _ => None,
        };
        if let Some(property) = property {
            theme_colors.push(ThemeColorBinding {
                state: vs,
                property,
                token: token.to_owned(),
            });
            return;
        }
    }
    let spec = slots.entry(vs).or_default();
    match prop.as_str() {
        "bgcolor" => spec.bg_color = parse_color(val),
        "fgcolor" => spec.fg_color = parse_color(val),
        "bordercolor" => spec.border_color = parse_color(val),
        "borderwidth" => spec.border_width = val.parse().ok(),
        "accentcolor" => spec.accent_color = parse_color(val),
        "thumbcolor" => spec.thumb_color = parse_color(val),
        "trackcolor" => spec.track_color = parse_color(val),
        "selectioncolor" => spec.selection_color = parse_color(val),
        "scrollbarcolor" => spec.scrollbar_color = parse_color(val),
        "placeholdercolor" => spec.placeholder_color = parse_color(val),
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
        _ => v
            .parse::<f32>()
            .map(Sizing::Fixed)
            .unwrap_or(Sizing::Content),
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

fn parse_virtual_column(element: &Element, index: usize) -> VirtualColumn {
    let key = element
        .attr("key")
        .map(str::to_owned)
        .unwrap_or_else(|| format!("column_{index}"));
    let title = element
        .attr("title-verbatim")
        .or_else(|| element.attr("title"))
        .unwrap_or(&key)
        .to_owned();
    let width = element
        .attr("width")
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(120.0);
    let mut column = VirtualColumn::new(key, title, width);
    if let Some(value) = element
        .attr("min-width")
        .and_then(|value| value.parse().ok())
    {
        column = column.min_width(value);
    }
    if let Some(value) = element
        .attr("max-width")
        .and_then(|value| value.parse().ok())
    {
        column = column.max_width(value);
    }
    if let Some(value) = element.attr("flex").and_then(|value| value.parse().ok()) {
        column = column.flex(value);
    }
    if let Some(value) = element.attr("align") {
        column = column.align(parse_align(value).unwrap_or(TextAlign::Left));
    }
    if let Some(value) = element.attr("sortable") {
        column = column.sortable(parse_bool(value));
    }
    if let Some(value) = element.attr("resizable") {
        column = column.resizable(parse_bool(value));
    }
    column
}

fn parse_virtual_selection_mode(value: &str) -> VirtualSelectionMode {
    match value.to_ascii_lowercase().as_str() {
        "none" | "off" => VirtualSelectionMode::None,
        "multiple" | "multi" | "extended" => VirtualSelectionMode::Multiple,
        _ => VirtualSelectionMode::Single,
    }
}

/// 解析滚动条可见性：always/on → 始终；hidden/none/off → 隐藏；其余按 auto。
fn parse_scrollbar_visibility(s: &str) -> ScrollBarVisibility {
    match s.to_lowercase().as_str() {
        "always" | "on" | "visible" | "show" => ScrollBarVisibility::Always,
        "hidden" | "none" | "off" | "hide" => ScrollBarVisibility::Hidden,
        _ => ScrollBarVisibility::Auto,
    }
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
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.')
                {
                    i += 1;
                }
                out.push(Tok::Ident(
                    String::from_utf8_lossy(&b[start..i]).into_owned(),
                ));
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
        let mut p = P { toks, pos: 0, ctx };
        p.or_expr()
    }
}
