//! flexui-xml：XML 布局描述（L4/L5）。类 duilib 的运行时 XML 加载，
//! 支持分状态样式属性、v-if 条件渲染与平台谓词。

mod build;
mod native_menu;
mod parser;

pub use build::{
    build_fragment_res, build_fragment_str, load_res, load_str, load_window_res, load_window_str,
    Context, LoadError, LoadResult, WindowDoc,
};
pub use native_menu::{load_native_menu_res, load_native_menu_str};
pub use parser::{parse, Element, ParseError};
