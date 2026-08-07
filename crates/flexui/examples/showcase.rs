//! 全控件 XML 布局 + 事件响应示例（面向对象用法，参考 duilib WinImplBase/Window）。
//! 运行：`cargo run -p flexui --example showcase`
//!
//! 用法：定义 `MainWindow` 实现 `WindowImpl`（≈ 继承 WindowImplBase），重写 config/skin
//! 与 on_click（≈ Notify）；`Window::new(MainWindow{..}).center().run()`（≈ Window）。

use flexui::{Skin, Window, WindowConfig, WindowCtx, WindowImpl};

const UI: &str = include_str!("assets/showcase.xml");

/// 主窗口：持有自己的状态（点击计数、主按钮启用态），重写窗口钩子。
struct MainWindow {
    clicks: u32,
    primary_enabled: bool,
}

impl WindowImpl for MainWindow {
    fn config(&self) -> WindowConfig {
        WindowConfig::new("flexui-rs 控件总览 (OO)", 680.0, 560.0)
    }

    fn skin(&self) -> Skin {
        // $ASSETS 占位替换为资源目录（资源系统落地前的临时做法）。
        let assets = format!("{}/examples/assets", env!("CARGO_MANIFEST_DIR"));
        Skin::xml(UI.replace("$ASSETS", &assets))
    }

    fn on_init(&mut self, ctx: &mut WindowCtx) {
        ctx.set_text("status", "状态：就绪，试试点击 / 勾选 / 切页 / 打字");
    }

    /// 统一点击通知（≈ duilib Notify）：按控件 name 分派。
    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        match name {
            "btnPrimary" => {
                self.clicks += 1;
                ctx.set_text("status", format!("状态：点击『主要按钮』，共 {} 次", self.clicks));
            }
            "btnGhost" => ctx.set_text("status", "状态：点击了『次要按钮』"),
            "btnToggle" => {
                self.primary_enabled = !self.primary_enabled;
                ctx.set_enabled("btnPrimary", self.primary_enabled);
                ctx.set_text(
                    "status",
                    format!("状态：主要按钮已{}", if self.primary_enabled { "启用" } else { "禁用" }),
                );
            }
            "chkRemember" => {
                let on = ctx.is_selected("chkRemember").unwrap_or(false);
                ctx.set_text("status", format!("状态：记住我 = {}", if on { "✓" } else { "✗" }));
            }
            "chkNews" => {
                let on = ctx.is_selected("chkNews").unwrap_or(false);
                ctx.set_text("status", format!("状态：订阅通知 = {}", if on { "✓" } else { "✗" }));
            }
            _ => {}
        }
    }
}

fn main() {
    Window::new(MainWindow {
        clicks: 0,
        primary_enabled: true,
    })
    .center()
    .run();
}
