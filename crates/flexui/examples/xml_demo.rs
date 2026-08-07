//! XML 布局演示（类 duilib）：整个界面用 XML 描述，含分状态样式、
//! v-if 平台条件渲染（仅非 macOS 渲染系统标题栏按钮）、Radio+TabBox 组成的 tabbar。
//!
//! 运行：`cargo run -p flexui --example xml_demo`

use flexui::{run_xml, Context, WindowConfig};

const UI: &str = r##"
<VBox spacing="12" padding="18" normal-bgcolor="#1A1C22">
    <!-- 自绘标题栏：系统按钮仅在非 macOS 渲染（macOS 有原生交通灯） -->
    <HBox height="28" spacing="6" v-if="!platform.macos">
        <Button name="min" text="—" width="40" height="24" normal-bgcolor="#3A3F4B"/>
        <Button name="max" text="□" width="40" height="24" normal-bgcolor="#3A3F4B"/>
        <Button name="close" text="×" width="40" height="24" normal-bgcolor="#C0453B"/>
    </HBox>

    <Label text="flexui-rs · XML 布局演示" height="26" normal-fgcolor="#FFFFFF"/>

    <!-- tabbar 标签行 -->
    <HBox height="26" spacing="8">
        <Radio group="1" tab-index="0" selected="true" text="页面一" width="90" height="24" normal-fgcolor="#E6EBF5"/>
        <Radio group="1" tab-index="1" text="页面二" width="90" height="24" normal-fgcolor="#E6EBF5"/>
        <Radio group="1" tab-index="2" text="页面三" width="90" height="24" normal-fgcolor="#E6EBF5"/>
    </HBox>

    <!-- tabbar 内容区：与上面 radio 组 1 绑定 -->
    <TabBox bindgroup="1" flex="1">
        <Panel normal-bgcolor="#2C3E5A" corner-radius="10">
            <Label text="第一页内容" normal-fgcolor="#FFFFFF" normal-textalign="center"/>
        </Panel>
        <Panel normal-bgcolor="#3C3054" corner-radius="10">
            <Label text="第二页内容" normal-fgcolor="#FFFFFF" normal-textalign="center"/>
        </Panel>
        <Panel normal-bgcolor="#2C4A3E" corner-radius="10">
            <Label text="第三页内容" normal-fgcolor="#FFFFFF" normal-textalign="center"/>
        </Panel>
    </TabBox>

    <HBox height="40" spacing="10">
        <Button name="ok" text="确定" width="100" height="36"
                normal-bgcolor="#3478F6" hot-bgcolor="#4A8CFF" pushed-bgcolor="#2A5FD0"
                normal-fgcolor="#FFFFFF" corner-radius="8"/>
        <CheckBox text="记住我" height="24" normal-fgcolor="#E6EBF5"/>
        <Edit width="160" height="30" normal-bgcolor="#2A2E38" normal-fgcolor="#FFFFFF"
              border-width="1" normal-bordercolor="#3A4050" corner-radius="6"/>
    </HBox>
</VBox>
"##;

fn main() {
    // 上下文自动注入 platform.macos/windows；也可手动覆盖用于演示。
    let ctx = Context::new();
    let config = WindowConfig {
        title: "flexui-rs XML 演示 (macOS)".to_string(),
        width: 660.0,
        height: 480.0,
    };
    if let Err(e) = run_xml(config, UI, &ctx) {
        eprintln!("加载 XML 失败: {e}");
    }
}
