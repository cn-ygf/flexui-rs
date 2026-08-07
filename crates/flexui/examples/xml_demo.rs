//! XML 布局演示（面向对象用法）：整个界面用 XML 描述，含分状态样式、
//! v-if 平台条件渲染（仅非 macOS 渲染系统标题栏按钮）、Radio+TabBox 组成的 tabbar。
//!
//! 运行：`cargo run -p flexui --example xml_demo`

use flexui::{Skin, Window, WindowConfig, WindowImpl};

const UI: &str = r##"
<VBox spacing="12" padding="18" normal-bgcolor="#1A1C22">
    <!-- 自绘标题栏：系统按钮仅在非 macOS 渲染（macOS 有原生交通灯） -->
    <HBox height="28" spacing="6" v-if="!platform.macos">
        <Button name="min" text="—" width="40" height="24" normal-bgcolor="#3A3F4B"/>
        <Button name="max" text="□" width="40" height="24" normal-bgcolor="#3A3F4B"/>
        <Button name="close" text="×" width="40" height="24" normal-bgcolor="#C0453B"/>
    </HBox>

    <Label text="flexui-rs · XML 布局演示" height="26" normal-fgcolor="#FFFFFF"/>

    <HBox height="26" spacing="8">
        <Radio group="1" tab-index="0" selected="true" text="页面一" width="90" height="24" normal-fgcolor="#E6EBF5"/>
        <Radio group="1" tab-index="1" text="页面二" width="90" height="24" normal-fgcolor="#E6EBF5"/>
        <Radio group="1" tab-index="2" text="页面三" width="90" height="24" normal-fgcolor="#E6EBF5"/>
    </HBox>

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
</VBox>
"##;

struct DemoWindow;

impl WindowImpl for DemoWindow {
    fn config(&self) -> WindowConfig {
        WindowConfig::new("flexui-rs XML 演示", 660.0, 480.0)
    }
    fn skin(&self) -> Skin {
        Skin::xml(UI)
    }
}

fn main() {
    Window::new(DemoWindow).run();
}
