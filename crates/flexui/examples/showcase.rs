//! 全控件 XML 布局 + 事件响应示例（面向对象用法，参考 duilib WinImplBase/Window）。
//! 运行：`cargo run -p flexui --example showcase`
//!
//! 用法：定义 `MainWindow` 实现 `WindowImpl`（≈ 继承 WindowImplBase），重写 config/skin
//! 与 on_click（≈ Notify）；`Window::new(MainWindow{..}).center().run()`（≈ Window）。

use flexui::{
    AnimProp, DirProvider, Easing, Rect, ResourceManager, Skin, Window, WindowConfig, WindowCtx,
    WindowImpl,
};

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
        // 经资源系统加载：XML 与图片(logo.bmp)都走下面的 ResourceManager（逻辑路径）。
        Skin::res("showcase.xml")
    }

    fn resources(&self) -> ResourceManager {
        // 挂目录 provider（RM1）；也可换成带密码 zip / 内嵌 zip（RM2/3）。
        let assets = format!("{}/examples/assets", env!("CARGO_MANIFEST_DIR"));
        let mut rm = ResourceManager::new();
        rm.mount(DirProvider::new(assets));
        rm
    }

    fn on_init(&mut self, ctx: &mut WindowCtx) {
        ctx.set_text("status", "状态：就绪，试试点击 / 勾选 / 切页 / 打字");
        // 后台线程演示：1.2s 后经 MainProxy 投递消息到主线程。
        if let Some(proxy) = ctx.main_proxy() {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1200));
                proxy.send("后台任务完成 ✅");
            });
        }
    }

    /// 后台线程投递的消息（主线程处理）。
    fn on_message(&mut self, msg: &str, ctx: &mut WindowCtx) {
        ctx.set_text("status", format!("状态：收到后台消息「{msg}」"));
    }

    /// 文件拖入窗口。
    fn on_drop_files(&mut self, paths: &[String], ctx: &mut WindowCtx) {
        let first = paths.first().map(|s| s.as_str()).unwrap_or("");
        ctx.set_text("status", format!("状态：拖入 {} 个文件，首个 {first}", paths.len()));
    }

    /// 统一点击通知（≈ duilib Notify）：按控件 name 分派。
    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        match name {
            "btnPrimary" => {
                self.clicks += 1;
                ctx.set_text("status", format!("状态：点击『主要按钮』，共 {} 次", self.clicks));
                // 动画：进度条平滑补间到满。
                ctx.animate("prog", AnimProp::Value, 1.0, 0.8, Easing::EaseInOut);
            }
            "btnGhost" => {
                ctx.set_text("status", "状态：点击了『次要按钮』（进度回落）");
                ctx.animate("prog", AnimProp::Value, 0.15, 0.6, Easing::EaseOut);
            }
            // 系统文件对话框：选择一个文件，显示路径。
            "btnOpen" => {
                let opts = flexui::dialog::FileDialog::new()
                    .title("选择一个图片")
                    .filter("图片", &["png", "jpg", "jpeg", "bmp"]);
                match flexui::dialog::open_file(&opts) {
                    Some(p) => ctx.set_text("status", format!("状态：选择了 {}", p.display())),
                    None => ctx.set_text("status", "状态：已取消选择文件"),
                }
            }
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
            // ComboBox 选中：读回填后的当前文本。
            "theme" => {
                let cur = ctx.with("theme", |w| w.base().text.clone()).unwrap_or_default();
                ctx.set_text("status", format!("状态：主题切换为『{cur}』"));
            }
            // ListView 选中行：读取选中索引。
            "cities" => {
                let idx = ctx.with("cities", |w| w.base().selected_index).unwrap_or(0);
                ctx.set_text("status", format!("状态：选中城市 #{idx}"));
            }
            // 右键菜单项。
            "ctxRefresh" => ctx.set_text("status", "状态：右键菜单 → 刷新"),
            "ctxAbout" => ctx.set_text("status", "状态：右键菜单 → 关于 flexui-rs"),
            _ => {}
        }
    }

    /// 右键任意具名控件 → 在点位弹出上下文菜单（选项经 on_click 上报）。
    fn on_context(&mut self, _name: &str, x: f32, y: f32, ctx: &mut WindowCtx) {
        ctx.open_menu(
            Rect::new(x, y, 0.0, 0.0),
            vec![
                ("刷新".to_string(), "ctxRefresh".to_string()),
                ("关于".to_string(), "ctxAbout".to_string()),
            ],
        );
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
