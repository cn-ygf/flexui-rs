# flexui-rs 跨平台 Rust UI 框架

从零构建的跨平台（macOS + Windows）Rust GUI 库：不依赖第三方渲染库/窗口管理器，
macOS 用 NSView 自绘 + NSWindow，Windows 用 GDI+ + Win32，统一 Rust API，并提供 C ABI FFI。

## 文档

- [XML 布局与控件属性参考](docs/XML布局与控件属性参考.md) —— XML 语法、全部控件及属性、样式系统、条件渲染、事件响应、完整示例。

## 示例

```bash
cargo run -p flexui-examples
```

包含主窗口、登录与设置对话框，演示 XML 布局、图片资源、本地化、菜单和多窗口。
