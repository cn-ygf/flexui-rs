//! 无边框 / 自绘标题栏演示（窗口阶段3）。
//! 运行：`cargo run -p flexui --example frameless`
//!
//! macOS：隐藏系统标题栏但保留交通灯（HiddenKeepControls），左侧用 v-if 平台谓词留位。
//! Windows：无系统标题栏，自绘 min/max/close 按钮，拖顶部空白移动窗口 + 系统 Aero Snap。
//! 自绘按钮通过 on_click(name) 调 WindowCtx.minimize/maximize/close 真正控制窗口。

use flexui::{Skin, TitlebarMode, Window, WindowConfig, WindowCtx, WindowImpl};

const UI: &str = r##"
<VBox normal-bgcolor="#1E2230">
  <!-- 自绘标题栏 -->
  <HBox height="40" normal-bgcolor="#2A2E3C">
    <!-- macOS 保留交通灯：左侧留位 -->
    <Panel width="72" v-if="platform.macos"/>
    <Label text="  flexui-rs · 自绘标题栏" flex="1" height="40" normal-fgcolor="#E6EBF5"/>
    <Button name="min"   text="—" width="46" height="40" normal-bgcolor="#2A2E3C" hot-bgcolor="#3A3F4B" normal-fgcolor="#E6EBF5"/>
    <Button name="max"   text="▢" width="46" height="40" normal-bgcolor="#2A2E3C" hot-bgcolor="#3A3F4B" normal-fgcolor="#E6EBF5"/>
    <Button name="close" text="×" width="46" height="40" normal-bgcolor="#2A2E3C" hot-bgcolor="#C0453B" pushed-bgcolor="#9A342C" normal-fgcolor="#FFFFFF"/>
  </HBox>

  <!-- 内容区 -->
  <VBox flex="1" padding="22" spacing="12">
    <Label text="无边框窗口 + 自绘标题栏" height="26" normal-fgcolor="#FFFFFF"/>
    <Label name="status" text="拖动顶部空白处移动窗口；点右上角按钮控制窗口" height="22" normal-fgcolor="#8FE3A0"/>
    <Panel flex="1" normal-bgcolor="#2C3E5A" corner-radius="10">
      <Label text="内容面板（四角圆角 / Flex 撑开）" normal-fgcolor="#FFFFFF" normal-textalign="center"/>
    </Panel>
  </VBox>
</VBox>
"##;

struct FramelessWindow;

impl WindowImpl for FramelessWindow {
    fn config(&self) -> WindowConfig {
        WindowConfig::new("flexui-rs 无边框", 560.0, 400.0)
            .titlebar(TitlebarMode::HiddenKeepControls)
    }
    fn skin(&self) -> Skin {
        Skin::xml(UI)
    }
    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        match name {
            "min" => ctx.minimize(),
            "max" => ctx.maximize(),
            "close" => ctx.close(),
            _ => {}
        }
    }
}

fn main() {
    Window::new(FramelessWindow).run();
}
