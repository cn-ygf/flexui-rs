# flexui-rs 跨平台 Rust UI 框架

从零构建的跨平台（macOS + Windows）Rust GUI 库：不依赖第三方渲染库/窗口管理器，
macOS 用 NSView 自绘 + NSWindow，Windows 用 GDI+ + Win32，统一 Rust API，并提供 C ABI FFI。

## 文档

- [XML 布局与控件属性参考](docs/XML布局与控件属性参考.md) —— XML 语法、全部控件及属性、样式系统、条件渲染、事件响应、完整示例。

## 示例

```bash
cargo run -p flexui --example showcase
```

覆盖：VBox/HBox/Box/Label/Button(4 状态)/CheckBox/Radio+分组/TabBox/Edit(选区/剪贴板/IME/多行)/
Image/Progress/Slider/Separator/ComboBox/ListView，以及 Tooltip、右键菜单、动画、文件对话框等。
