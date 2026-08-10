//! `<Window>` 作为 XML 根 + `<Include>` 子 XML 演示（阶段4 / W6+W7+W8）。
//! 运行：`cargo run -p flexui --example window_xml`
//!
//! main.xml 以 <Window> 为根（窗口配置来自其属性，覆盖 config()）；
//! 顶部工具栏由 <Include src="toolbar.xml"> 展开；按钮点击经 on_click 更新状态。

use flexui::{DirProvider, ResourceManager, Skin, Window, WindowConfig, WindowCtx, WindowImpl};

struct MainWindow;

impl WindowImpl for MainWindow {
    // 该 config 会被 <Window> 根属性覆盖（仅作 fallback）。
    fn config(&self) -> WindowConfig {
        WindowConfig::new("fallback", 400.0, 300.0)
    }
    fn skin(&self) -> Skin {
        Skin::res("main.xml")
    }
    fn resources(&self) -> ResourceManager {
        let dir = format!("{}/examples/assets/win", env!("CARGO_MANIFEST_DIR"));
        let mut rm = ResourceManager::new();
        rm.mount(DirProvider::new(dir));
        rm
    }
    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        ctx.set_text("status", format!("点击了工具栏按钮：{name}"));
    }
}

fn main() {
    Window::new(MainWindow).run();
}
