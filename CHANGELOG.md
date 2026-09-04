# Changelog

本项目的显著变更记录在此。版本号遵循语义化版本；`0.x` 阶段仍可能调整公开 API。

## [0.1.0] - 2026-09-01

首个三平台技术预览版本。

### 新增

- macOS、Windows 与 Linux 原生窗口后端，共用平台无关的控件树、布局、事件、主题与绘制核心。
- XML 布局与纯 Rust 双入口，支持 Include、条件渲染、运行时属性修改、主题与本地化绑定。
- Button、Edit、Label、Image、CheckBox、Radio、Switch、ComboBox、Slider、Progress、ListView、VirtualList、TabBox、ScrollView、菜单与 Tooltip。
- 多窗口、模态窗口、完整关闭生命周期、UI 线程代理、IME、剪贴板、文件对话框、拖放和系统托盘示例。
- PNG/JPEG/SVG、HiDPI 密度资源、九宫格、渐变、阴影、帧动画与深浅色主题。
- 控件/子树二维平移、缩放、旋转，以及矩形、圆角、椭圆命中区域；绘制、输入、滚动与原生锚点保持一致。
- C ABI 静态库/动态库接口及自动校验的 C 头文件。

### 工程化

- 增加跨平台窗口生命周期冒烟程序与 macOS/Linux/Windows 运行脚本。
- 增加工作区版本一致性、C ABI 版本和头文件防漂移校验。
- 拆分事件上下文与核心热点测试模块，统一三平台浮层请求入口。

### 已知边界

- Linux 使用 X11；Wayland 通过 XWayland 运行，GNOME 托盘需要 AppIndicator 扩展。
- Accessibility、TreeView、系统通知与 3D 透视变换不在本版本范围。
- crates.io 发布需由项目所有者先确定并添加开源许可证；当前准备的是源码发布版本。

[0.1.0]: https://github.com/cn-ygf/flexui-rs/releases/tag/v0.1.0
