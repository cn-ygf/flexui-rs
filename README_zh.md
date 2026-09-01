# FlexUI

面向 Rust 的跨平台桌面 UI 框架，提供原生窗口、自绘控件树、XML 布局和纯 Rust API。

简体中文 | [English](README.md)

> [!IMPORTANT]
> FlexUI 仍处于积极开发的早期阶段（`0.0.x`）。目前适合技术验证和内部工具，首个稳定版本发布前 API 仍可能调整。

<p align="center">
  <img src="docs/gallery_1.png" alt="使用默认浅色主题的 FlexUI Gallery" width="49%">
  <img src="docs/gallery_2.png" alt="使用默认深色主题的 FlexUI Gallery" width="49%">
</p>

FlexUI 让应用逻辑始终留在 Rust 中，同时在合适的位置调用系统能力：macOS 使用 AppKit / CoreGraphics，Windows 使用 Win32 / GDI+，Linux 使用 X11 / Cairo / Pango。布局、控件、状态、主题和绘制共享同一套平台无关核心。

## 核心能力

- **两种界面开发方式**：使用 XML 描述界面，也可以用纯 Rust 构建同样的控件树。
- **原生桌面窗口**：支持多窗口、模态窗口、系统/自绘标题栏、DPI、IME、剪贴板、文件拖放和原生文件对话框。
- **实用布局系统**：`VBox`、`HBox`、`Panel`、`ScrollView`、`TabBox`，支持固定/内容/填充尺寸、Flex 伸缩、对齐、内外边距和绝对定位。
- **基础控件齐全**：文本、图片、按钮、编辑框、CheckBox、Switch、Radio、ComboBox、Slider、Progress、ListView、Separator、滚动视图和菜单。
- **分状态样式**：支持 normal、hot、pushed、disabled、focus、selected 及组合状态；颜色、边框、图片、渐变、阴影、透明度和文本对齐均可按状态定义。
- **主题系统**：内置浅色/深色主题，支持语义色令牌、控件配方、variant、class 和运行时切换主题。
- **资源化渲染**：支持 PNG、JPEG、SVG、`@2.00x` 密度资源、居中/拉伸/九宫格绘制，以及目录、ZIP、内嵌 ZIP 资源提供器。
- **帧动画**：支持自动循环、悬停播放、点击播放一次和按住播放，可配置帧速、暂停/恢复及循环间隔。
- **多语言**：支持 JSON 词典和 Apple String Catalog，具备语言回退、参数插值、复数规则和运行时切换语言。
- **两类菜单**：既能使用 FlexUI 自绘浮层菜单，也能调用系统原生菜单；支持图标、快捷键、分割线和子菜单，可由 XML 或 Rust 创建。
- **C ABI**：`flexui-ffi` 可输出 `cdylib` 和 `staticlib`，方便非 Rust 程序接入。

## 控件与主题 Gallery

Gallery 不依赖贴图展示基础控件，同时演示内置浅色/深色主题，以及基于 `#FB7299` 扩展的自定义主题。

<p align="center">
  <img src="docs/gallery_3.png" alt="使用自定义粉色主题的 FlexUI Gallery" width="49%">
  <img src="docs/gallery_4.png" alt="FlexUI 自绘菜单与系统原生菜单对比" width="49%">
</p>

```bash
cargo run -p flexui-gallery
cargo run -p flexui-gallery -- --dark
```

## 复杂业务界面示例

`flexui-examples` 是用于验证真实桌面工作流的重资源示例，覆盖子 XML 页面、图片资源、自绘窗口、弹出菜单、登录/设置模态窗口、多语言和帧动画。

<p align="center">
  <img src="docs/uu_macos_1.png" alt="FlexUI 复杂应用首页" width="49%">
  <img src="docs/uu_macos_2.png" alt="复杂应用中的 FlexUI 自绘弹出菜单" width="49%">
</p>
<p align="center">
  <img src="docs/uu_macos_3.png" alt="FlexUI 绘制的登录模态窗口" width="49%">
  <img src="docs/uu_macos_4.png" alt="具备多语言和基础控件的设置窗口" width="49%">
</p>
<p align="center">
  <img src="docs/uu_macos_5.png" alt="按控件状态驱动的帧动画示例" width="49%">
  <img src="docs/uu_win_1.png" alt="Windows" width="49%">
</p>

```bash
cargo run -p flexui-examples
```

该示例复刻第三方产品界面仅用于验证框架渲染能力，与原产品官方无关，也不是其正式客户端。

## 快速开始

### 环境要求

- 当前稳定版 Rust 工具链
- macOS、Windows，或使用 X11 的 Linux
- 平台构建工具：macOS 使用 Xcode Command Line Tools，Windows 使用与 Rust 工具链兼容的 C/C++ 构建环境，Linux 需要 Cairo/Pango/X11 开发包

克隆仓库并运行 Gallery：

```bash
git clone https://github.com/cn-ygf/flexui-rs.git
cd flexui-rs
cargo run -p flexui-gallery
```

### 最小 XML 窗口

```rust
use flexui::{Skin, Window, WindowCtx, WindowImpl};

struct HelloWindow;

impl WindowImpl for HelloWindow {
    fn skin(&self) -> Skin {
        Skin::xml(
            r#"
            <Window title="FlexUI" width="420" height="240">
              <VBox padding="24" spacing="16" align="center" justify="center">
                <Label text-verbatim="Hello from FlexUI" font-size="24" bold="true"/>
                <Button name="close" text-verbatim="Close" variant="primary"
                        width="120" height="40"/>
              </VBox>
            </Window>
            "#,
        )
    }

    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        if name == "close" {
            ctx.close();
        }
    }
}

fn main() {
    Window::new(HelloWindow).center().run();
}
```

### UI 线程投递与窗口生命周期

`MainProxy` 不绑定具体 async runtime。初始化时取得并克隆句柄，在线程或异步任务完成后
调用 `post`；闭包会在所属窗口的 UI 线程执行，`WindowCtx` 的属性接口会自动触发重绘或
重新布局。

```rust
fn on_init(&mut self, ctx: &mut WindowCtx) {
    let ui = ctx.main_proxy().expect("window proxy");
    std::thread::spawn(move || {
        let result = load_data();
        ui.post(move |ctx| {
            ctx.set_text("status", result);
            ctx.set_enabled("retry", true);
        });
    });
}
```

在 Tokio、async-std 或其他执行器中，也可以在 `.await` 之后调用同一个 `post`，FlexUI
不强制依赖某个异步运行时。窗口钩子的顺序为 `on_before_init` → `on_init` →
`on_initialized` → `on_closing` → `on_closed`。`on_closing` 返回 `false` 可以取消关闭；
`on_closed` 不提供 `WindowCtx`，因为原生窗口此时已经销毁。最小化、最大化和恢复统一通过
`on_window_event` 上报为 `WindowEvent::Minimized`、`Maximized` 和 `Restored`。

当前在其他工作区中可以通过路径依赖引入门面 crate：

```toml
[dependencies]
flexui = { path = "../flexui-rs/crates/flexui" }
```

FlexUI 暂未发布到 crates.io。

## XML 概览

XML 不是必选项，但很适合贴图界面以及设计与业务代码分离的项目。

```xml
<Window title="Demo" width="720" height="480" titlebar="system">
  <VBox width="fill" height="fill">
    <HBox height="56" padding="16" align="center" bgcolor="@surface">
      <Label text-verbatim="Dashboard" width="fill" font-size="20" bold="true"/>
      <Switch name="dark_mode" width="44" height="24"/>
    </HBox>
    <TabBox name="pages" bindgroup="1" width="fill" height="fill">
      <Include src="pages/home.xml"/>
      <Include src="pages/settings.xml"/>
    </TabBox>
  </VBox>
</Window>
```

状态前缀可以组合，例如 `hot-bgcolor`、`focus-bordercolor`、`selected-fgcolor` 和 `hot-focus-bgimage`。通过代码更新属性时，框架会根据属性类型自动触发局部重绘或重新布局。

## 平台后端

| 能力 | macOS | Windows | Linux |
| --- | --- | --- | --- |
| 原生窗口 | AppKit `NSWindow` + 自绘 `NSView` | Win32 窗口 | X11（`x11rb`） |
| 绘制 | CoreGraphics | GDI+ | Cairo + Pango |
| 原生菜单 | AppKit 菜单 | Win32 菜单 | X11 override-redirect 菜单 |
| 剪贴板、IME、文件对话框 | 支持 | 支持 | 支持 |
| 自绘/系统标题栏 | 支持 | 支持 | 支持 |
| 文件拖放 | 支持 | 支持 | 计划中 |
| 系统托盘示例 | 支持 | 支持 | 计划中 |

## 工作区结构

| Crate | 职责 |
| --- | --- |
| `flexui` | 面向使用者的统一入口和平台选择 |
| `flexui-core` | 控件树、布局、事件、样式、主题和动画 |
| `flexui-xml` | XML 解析、Include、绑定和窗口文档 |
| `flexui-macos` / `flexui-windows` / `flexui-linux` | 原生窗口、输入和绘制后端 |
| `flexui-gfx` | 画布接口与几何基础类型 |
| `flexui-resource` / `flexui-svg` | 资源提供器与 SVG 光栅化 |
| `flexui-i18n` | 多语言词典、回退和复数规则 |
| `flexui-native-menu` | 跨平台系统弹出菜单 |
| `flexui-ffi` | C ABI 输出 |
| `flexui-gallery` | 主题与基础控件 Gallery |
| `flexui-examples` | 复杂 XML/资源应用示例 |

## 文档

- [XML 布局与控件属性参考](docs/XML布局与控件属性参考.md)
- [多语言与本地化](docs/i18n.md)
- 主题和控件示例见 [`crates/flexui-gallery`](crates/flexui-gallery)。
- 多窗口、资源、菜单、多语言和动画示例见 [`crates/flexui-examples`](crates/flexui-examples)。

## 构建与测试

```bash
cargo test --workspace --all-targets
cargo check --workspace --all-targets
```

将复杂示例打包为临时签名的 macOS `.app`：

```bash
./scripts/build-macos-app.sh
open target/release/bundle/macos/FlexUIExamples.app
```

## 开源协议

FlexUI 使用 [MIT License](LICENSE)。
