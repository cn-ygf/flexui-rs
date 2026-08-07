//! flexui：面向用户的门面 crate（L5）。
//!
//! 一次性再导出核心控件/类型与 XML 加载器，并按平台选择后端。上层只依赖本 crate。

// 核心：控件、状态、样式、布局、事件、分发。
pub use flexui_core::*;

// XML 布局加载（代码布局 vs XML 布局二选一，产出同一棵控件树）。
pub use flexui_xml::{load_str as load_xml_str, Context, LoadError, LoadResult};

// —— 平台后端选择（编译期按目标平台二选一）——
#[cfg(target_os = "macos")]
pub use flexui_macos::{run, WindowConfig};
#[cfg(target_os = "windows")]
pub use flexui_windows::{run, WindowConfig};

/// 用 XML 描述直接启动应用（代码更少）。加载 → 注册 tabbar 绑定 → 运行。
///
/// 在有可用后端的平台编译（macOS / Windows）。
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn run_xml(config: WindowConfig, xml: &str, ctx: &Context) -> Result<(), LoadError> {
    let res = load_xml_str(xml, ctx)?;
    let mut disp = Dispatcher::new();
    for (group, tabbox) in res.bindings {
        disp.bind_tab(group, tabbox);
    }
    run(config, res.root, disp);
    Ok(())
}
