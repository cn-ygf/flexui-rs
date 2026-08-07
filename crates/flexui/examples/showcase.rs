//! 全控件 XML 布局 + 事件响应示例。
//! 运行：`cargo run -p flexui --example showcase`
//!
//! 布局来自 assets/showcase.xml（覆盖所有已实现控件）；事件在代码里按 name 挂接：
//! - 按钮点击 → 更新 status 标签（EventCtx 按 name 改别的控件，可见响应）
//! - 「禁用/启用」按钮 → 切换主要按钮的可用态
//! - 复选框切换 → status 显示勾选状态
//! - 单选切换 → 驱动 TabBox 翻页（tabbar，XML 绑定自动生效）
//! - 输入框 → 直接键入（自带光标与文本更新）

use std::cell::Cell;
use std::rc::Rc;

use flexui::{find_mut_by_name, load_xml_str, run, Context, WindowConfig};

const UI: &str = include_str!("assets/showcase.xml");

fn main() {
    // 把 $ASSETS 占位替换为资源目录绝对路径（正式版会由资源系统统一解析，见 09 号计划）。
    let assets = format!("{}/examples/assets", env!("CARGO_MANIFEST_DIR"));
    let xml = UI.replace("$ASSETS", &assets);

    let ctx = Context::new();
    let res = match load_xml_str(&xml, &ctx) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("加载 XML 失败: {e}");
            return;
        }
    };
    let mut root = res.root;

    // —— 按 name 挂接事件响应 ——
    let count = Rc::new(Cell::new(0u32));

    // 主要按钮：点击计数 + 更新状态标签。
    {
        let c = count.clone();
        if let Some(w) = find_mut_by_name(root.as_mut(), "btnPrimary") {
            w.base_mut().on_click = Some(Box::new(move |ctx| {
                c.set(c.get() + 1);
                ctx.set_text("status", format!("状态：点击『主要按钮』，共 {} 次", c.get()));
            }));
        }
    }
    // 次要按钮。
    if let Some(w) = find_mut_by_name(root.as_mut(), "btnGhost") {
        w.base_mut().on_click = Some(Box::new(|ctx| {
            ctx.set_text("status", "状态：点击了『次要按钮』");
        }));
    }
    // 禁用/启用主要按钮。
    {
        let enabled = Rc::new(Cell::new(true));
        if let Some(w) = find_mut_by_name(root.as_mut(), "btnToggle") {
            w.base_mut().on_click = Some(Box::new(move |ctx| {
                enabled.set(!enabled.get());
                ctx.set_enabled("btnPrimary", enabled.get());
                ctx.set_text(
                    "status",
                    format!("状态：主要按钮已{}", if enabled.get() { "启用" } else { "禁用" }),
                );
            }));
        }
    }
    // 复选框：切换后（selected 已更新）读取并显示。
    for name in ["chkRemember", "chkNews"] {
        if let Some(w) = find_mut_by_name(root.as_mut(), name) {
            let label = if name == "chkRemember" { "记住我" } else { "订阅通知" };
            w.base_mut().on_click = Some(Box::new(move |ctx| {
                let on = ctx.is_selected(name).unwrap_or(false);
                ctx.set_text(
                    "status",
                    format!("状态：{} = {}", label, if on { "✓ 已选" } else { "✗ 未选" }),
                );
            }));
        }
    }

    // 分发器 + tabbar 绑定（来自 XML）。
    let mut disp = flexui::Dispatcher::new();
    for (group, tabbox) in res.bindings {
        disp.bind_tab(group, tabbox);
    }

    run(
        WindowConfig {
            title: "flexui-rs 控件总览 (macOS)".to_string(),
            width: 680.0,
            height: 560.0,
        },
        root,
        disp,
    );
}
