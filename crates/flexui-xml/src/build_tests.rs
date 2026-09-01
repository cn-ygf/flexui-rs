use super::*;
use flexui_core::{
    apply_localizations, find_by_name, layout_node, Canvas, Font, Rect, Size, WidgetPropertyKey,
};
use flexui_resource::{DirProvider, ResourceManager};

#[test]
fn 内置平台谓词覆盖三个后端() {
    let ctx = Context::new();
    assert_eq!(ctx.get("platform.macos"), cfg!(target_os = "macos"));
    assert_eq!(ctx.get("platform.windows"), cfg!(target_os = "windows"));
    assert_eq!(ctx.get("platform.linux"), cfg!(target_os = "linux"));
}

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
    std::fs::write(
        dir.join("parts.xml"),
        r#"<Label name="fromInclude" text="子"/>"#,
    )
    .unwrap();
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
    let frag = build_fragment_str(
        r#"<HBox><Button name="ok" text="确定"/></HBox>"#,
        &Context::new(),
    )
    .unwrap();
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
    assert_eq!(
        parse_color("#FFFFFF"),
        Some(Color::from_u8(255, 255, 255, 255))
    );
    assert_eq!(parse_color("#F00"), Some(Color::from_u8(255, 0, 0, 255)));
    assert_eq!(parse_color("#80000000"), Some(Color::from_u8(0, 0, 0, 128)));
}

#[test]
fn 边距简写解析() {
    // 1 值：四边
    assert_eq!(parse_insets("10"), Some(Insets::all(10.0)));
    // 2 值：纵 横 → new(left=h, top=v, right=h, bottom=v)
    assert_eq!(
        parse_insets("10 20"),
        Some(Insets::new(20.0, 10.0, 20.0, 10.0))
    );
    // 3 值：上 横 下
    assert_eq!(
        parse_insets("10 20 30"),
        Some(Insets::new(20.0, 10.0, 20.0, 30.0))
    );
    // 4 值：上 右 下 左（CSS 顺序）→ new(left, top, right, bottom)
    assert_eq!(
        parse_insets("10 20 30 40"),
        Some(Insets::new(40.0, 10.0, 20.0, 30.0))
    );
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
    let res = load_str(
        r##"<combobox options="a,b,c" selected="1" tooltip="选一个"/>"##,
        &ctx,
    )
    .unwrap();
    let b = res.root.base();
    assert_eq!(b.text, "b");
    assert_eq!(res.root.selected_index(), Some(1));
    assert_eq!(b.tooltip.as_deref(), Some("选一个"));
    assert_eq!(b.children.len(), 0, "item 不作为子节点");
    // <item> 子元素形式。
    let res2 = load_str(
        r##"<select><item text="X"/><item label="Y"/></select>"##,
        &ctx,
    )
    .unwrap();
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
fn xml_switch_使用独立控件类型() {
    let res = load_str(
        r##"<Switch checked="true" normal-trackcolor="#112233" selected-trackcolor="#00CFC4"/>"##,
        &Context::new(),
    )
    .unwrap();
    assert_eq!(res.root.base().kind, flexui_core::WidgetKind::Switch);
    assert!(res.root.base().selected);
    assert_eq!(
        res.root.base().resolved_style().track_color,
        Some(Color::from_u8(0, 207, 196, 255))
    );
}

#[test]
fn xml_listview_项与选中() {
    let ctx = Context::new();
    let res = load_str(
        r##"<listview items="一,二,三" selected="2" row-height="24"/>"##,
        &ctx,
    )
    .unwrap();
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
fn xml_主题变体类名与令牌可动态刷新() {
    let mut result = load_str(
        r#"<Button variant="primary" class="compact emphasized" fgcolor="@on-brand"/>"#,
        &Context::new(),
    )
    .unwrap();
    assert_eq!(result.root.base().variant, "primary");
    assert_eq!(result.root.base().classes, ["compact", "emphasized"]);

    let light = flexui_core::Theme::light();
    flexui_core::apply_theme(result.root.as_mut(), &light);
    assert_eq!(
        result.root.base().resolved_style().fg_color,
        Some(light.palette.on_brand)
    );

    let dark = flexui_core::Theme::dark();
    flexui_core::apply_theme(result.root.as_mut(), &dark);
    assert_eq!(
        result.root.base().resolved_style().fg_color,
        Some(dark.palette.on_brand)
    );
}

#[test]
fn frame_pattern_supports_padding_and_descending_ranges() {
    assert_eq!(
        expand_frame_pattern("loading_{01..03}@2.00x.png").unwrap(),
        [
            "loading_01@2.00x.png",
            "loading_02@2.00x.png",
            "loading_03@2.00x.png"
        ]
    );
    assert_eq!(
        expand_frame_pattern("frame_{3..1}.png").unwrap(),
        ["frame_3.png", "frame_2.png", "frame_1.png"]
    );
}

#[test]
fn invalid_frame_patterns_are_reported() {
    assert!(expand_frame_pattern("loading.png").is_err());
    assert!(expand_frame_pattern("loading_{1-3}.png").is_err());
    assert!(expand_frame_pattern("loading_{1..20000}.png").is_err());
    assert!(load_str(r#"<Panel normal-fgframes="loading.png"/>"#, &Context::new()).is_err());
}

#[test]
fn xml_frame_animations_parse_state_combination_and_click() {
    let xml = r#"<Button normal-fgframes="normal_{1..2}.png" normal-fgframes-fps="12"
            normal-fgframes-interval="300"
            hot-focus-fgframes="combined_{01..03}.png" hot-focus-fgframes-play="once"
            hot-focus-fgframes-finish="last"
            click-bgframes="click_{1..4}.png" click-bgframes-fps="20"/>"#;
    let result = load_str(xml, &Context::new()).unwrap();
    let base = result.root.base();
    let normal_animation = base
        .style
        .resolve(VisualState::default())
        .fg_animation
        .unwrap();
    assert_eq!(normal_animation.frames.len(), 2);
    assert_eq!(normal_animation.fps, 12.0);
    assert_eq!(normal_animation.loop_interval, 0.3);
    assert_eq!(normal_animation.playback, FramePlayback::Loop);

    let combined_animation = base
        .style
        .resolve(VisualState::new(BaseState::Hot, true))
        .fg_animation
        .unwrap();
    assert_eq!(combined_animation.frames.len(), 3);
    assert_eq!(combined_animation.playback, FramePlayback::Once);
    assert_eq!(combined_animation.finish, FrameFinish::Last);

    let click = base.click_bg_animation.as_ref().unwrap();
    assert_eq!(click.frames.len(), 4);
    assert_eq!(click.fps, 20.0);
    assert_eq!(click.playback, FramePlayback::Once);
    assert_eq!(click.finish, FrameFinish::Restore);
}

#[test]
fn xml_frame_animation_rejects_invalid_interval() {
    for interval in ["-1", "abc", "NaN", "inf"] {
        let xml = format!(
            r#"<Panel normal-fgframes="frame_{{1..2}}.png" normal-fgframes-interval="{interval}"/>"#
        );
        assert!(
            load_str(&xml, &Context::new()).is_err(),
            "interval={interval}"
        );
    }
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

fn test_localizer() -> Localizer {
    let localizer = Localizer::new("en").unwrap();
    localizer
        .load_json_str(
            r#"{
            "locale":"en",
            "strings":{
                "window.title":"Settings for {name}",
                "label.title":"Settings",
                "edit.hint":"Phone",
                "button.tip":"Close",
                "choice.system":"System",
                "choice.english":"English"
            }
        }"#,
        )
        .unwrap();
    localizer
        .load_json_str(
            r#"{
            "locale":"zh-Hans",
            "strings":{
                "window.title":"{name} 的设置",
                "label.title":"设置",
                "edit.hint":"手机号",
                "button.tip":"关闭",
                "choice.system":"跟随系统",
                "choice.english":"英语"
            }
        }"#,
        )
        .unwrap();
    localizer.set_locale("en").unwrap();
    localizer
}

#[test]
fn xml_本地化自动键_强制键_字面量及参数() {
    let localizer = test_localizer();
    let mut ctx = Context::new();
    ctx.set_localizer(localizer);
    let root = load_str(
        r#"<VBox>
                <Label name="auto" text="label.title"/>
                <Label name="forced" text="loc:missing.key"/>
                <Label name="literal" text-verbatim="label.title"/>
                <Label name="plain" text="not.in.catalog"/>
            </VBox>"#,
        &ctx,
    )
    .unwrap()
    .root;
    assert_eq!(root.base().children[0].base().text, "Settings");
    assert_eq!(root.base().children[1].base().text, "missing.key");
    assert_eq!(root.base().children[2].base().text, "label.title");
    assert_eq!(root.base().children[3].base().text, "not.in.catalog");
    assert_eq!(root.base().children[0].base().localizations.len(), 1);
    assert_eq!(root.base().children[1].base().localizations.len(), 1);
    assert!(root.base().children[2].base().localizations.is_empty());
}

#[test]
fn xml_无本地化环境时强制键隐藏loc前缀且verbatim优先() {
    let root = load_str(
            r#"<Label text="loc:label.title" text-verbatim="Fixed" tooltip="loc:tip" tooltip-verbatim="Tip"/>"#,
            &Context::new(),
        ).unwrap().root;
    assert_eq!(root.base().text, "Fixed");
    assert_eq!(root.base().tooltip.as_deref(), Some("Tip"));

    let key_only = load_str(r#"<Label text="loc:label.title"/>"#, &Context::new())
        .unwrap()
        .root;
    assert_eq!(key_only.base().text, "label.title");
}

#[test]
fn xml_本地化placeholder_tooltip_title并可切换语言() {
    let localizer = test_localizer();
    let mut ctx = Context::new();
    ctx.set_localizer(localizer.clone());
    let mut doc = load_window_str(
        r#"<Window title="window.title" title-args="name=UU">
                <VBox>
                    <Button tooltip="button.tip"/>
                    <Edit name="phone" text-verbatim="138" placeholder="edit.hint"/>
                </VBox>
            </Window>"#,
        &ctx,
    )
    .unwrap();
    let config = doc.config.as_ref().unwrap();
    assert_eq!(config.title, "Settings for UU");
    assert!(config.localized_title.is_some());
    assert_eq!(
        doc.root.base().children[0].base().tooltip.as_deref(),
        Some("Close")
    );
    assert_eq!(doc.root.base().children[1].base().text, "138");

    localizer.set_locale("zh-Hans").unwrap();
    apply_localizations(&mut doc.root, &localizer);
    assert_eq!(
        doc.root.base().children[0].base().tooltip.as_deref(),
        Some("关闭")
    );
    assert_eq!(
        doc.root.base().children[1].base().text,
        "138",
        "切换语言不能覆盖 Edit 输入"
    );
    assert_eq!(
        localizer.text(config.localized_title.clone().unwrap()),
        "UU 的设置"
    );
}

#[test]
fn xml_本地化列表项在切换语言后保持选中索引() {
    let localizer = test_localizer();
    let mut ctx = Context::new();
    ctx.set_localizer(localizer.clone());
    let mut root = load_str(
            r#"<VBox>
                <ComboBox name="language" options="choice.system,choice.english,Literal" selected="1"/>
                <ListView items="loc:choice.system,choice.english" selected="0"/>
            </VBox>"#,
            &ctx,
        ).unwrap().root;
    assert_eq!(root.base().children[0].base().text, "English");
    localizer.set_locale("zh-Hans").unwrap();
    apply_localizations(&mut root, &localizer);
    assert_eq!(root.base().children[0].base().text, "英语");
    assert_eq!(root.base().children[0].selected_index(), Some(1));
    assert_eq!(
        root.base().children[1].menu_items(),
        None,
        "ListView 不应被误识别为弹出菜单"
    );
}

#[test]
fn window_title_verbatim_不创建本地化绑定() {
    let localizer = test_localizer();
    let mut ctx = Context::new();
    ctx.set_localizer(localizer);
    let doc = load_window_str(
        r#"<Window title="window.title" title-verbatim="Fixed"><Panel/></Window>"#,
        &ctx,
    )
    .unwrap();
    let config = doc.config.unwrap();
    assert_eq!(config.title, "Fixed");
    assert!(config.localized_title.is_none());
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
fn xml_滚动条可见性与自动滚动() {
    let xml = r#"<Edit multiline="true" scrollbar="always" autoscroll="true"/>"#;
    let root = load_str(xml, &Context::new()).unwrap().root;
    assert!(matches!(
        root.property(flexui_core::WidgetPropertyKey::ScrollBar),
        Some(WidgetProperty::ScrollBar(ScrollBarVisibility::Always))
    ));
    assert!(matches!(
        root.property(flexui_core::WidgetPropertyKey::AutoScroll),
        Some(WidgetProperty::AutoScroll(true))
    ));
    // ScrollView 也支持 scrollbar 属性。
    let sv = load_str(r#"<ScrollView scrollbar="hidden"/>"#, &Context::new())
        .unwrap()
        .root;
    assert!(matches!(
        sv.property(flexui_core::WidgetPropertyKey::ScrollBar),
        Some(WidgetProperty::ScrollBar(ScrollBarVisibility::Hidden))
    ));
}

#[test]
fn xml_虚拟列表列与静态行() {
    let xml = r#"
            <VirtualList row-height="30" header-height="38" selection-mode="multiple"
                         overscan="5" selected="1">
              <Column key="id" title-verbatim="ID" width="72" align="right"/>
              <Column key="name" title-verbatim="Name" width="180" flex="1"/>
              <Row id="41" values="41|Alpha"/>
              <Row id="42" values="42|Beta"/>
            </VirtualList>
        "#;
    let root = load_str(xml, &Context::new()).unwrap().root;
    assert_eq!(root.base().kind, flexui_core::WidgetKind::VirtualList);
    assert!(matches!(
        root.property(flexui_core::WidgetPropertyKey::VirtualColumns),
        Some(WidgetProperty::VirtualColumns(columns))
            if columns.len() == 2 && columns[1].key == "name" && columns[1].flex == 1.0
    ));
    assert!(matches!(
        root.property(flexui_core::WidgetPropertyKey::VirtualSelectionMode),
        Some(WidgetProperty::VirtualSelectionMode(
            VirtualSelectionMode::Multiple
        ))
    ));
    let source = match root.property(flexui_core::WidgetPropertyKey::VirtualSource) {
        Some(WidgetProperty::VirtualSource(source)) => source,
        _ => panic!("应生成静态虚拟列表数据源"),
    };
    assert_eq!(source.row_count(), 2);
    assert_eq!(source.row_id(1), 42);
    assert_eq!(source.cell_text(1, "name"), "Beta");
    assert_eq!(root.selected_index(), Some(1));
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
fn focusable属性可关闭按钮焦点() {
    let res = load_str(
        r#"<Button name="code" focusable="false"/>"#,
        &Context::new(),
    )
    .unwrap();
    assert!(!res.root.base().focusable);
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
    assert_eq!(
        res.root.base().children.len(),
        0,
        "macOS 上不渲染系统按钮组"
    );

    // 强制 macos=false → 应生成
    let mut ctx2 = Context::new();
    ctx2.set("platform.macos", false);
    let res2 = load_str(xml, &ctx2).unwrap();
    assert_eq!(res2.root.base().children.len(), 1);
}

#[test]
fn hot_focus_组合样式可覆盖单独状态() {
    let xml = r##"<Edit normal-bgcolor="#FFFFFF" hot-bgcolor="#000000"
            focus-bgcolor="#00FF00" hot-focus-bgcolor="#FF0000"/>"##;
    let result = load_str(xml, &Context::new()).unwrap();
    let style = result
        .root
        .base()
        .style
        .resolve(VisualState::new(BaseState::Hot, true));
    assert_eq!(style.bg_color, Some(Color::from_u8(255, 0, 0, 255)));
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

#[test]
fn button_图标与文本矩形属性可由_xml_设置() {
    let xml = r#"<Button text-verbatim="Export" icon="export.png"
            icon-rect="12 10 20 20" text-rect="44 0 88 40"/>"#;
    let result = load_str(xml, &Context::new()).unwrap();
    assert!(matches!(
        result.root.property(WidgetPropertyKey::Icon),
        Some(WidgetProperty::Icon(Some(ImageSource::Path(path)))) if path == "export.png"
    ));
    assert!(matches!(
        result.root.property(WidgetPropertyKey::IconRect),
        Some(WidgetProperty::IconRect(Some(rect)))
            if rect == Rect::new(12.0, 10.0, 20.0, 20.0)
    ));
    assert!(matches!(
        result.root.property(WidgetPropertyKey::TextRect),
        Some(WidgetProperty::TextRect(Some(rect)))
            if rect == Rect::new(44.0, 0.0, 88.0, 40.0)
    ));
}
