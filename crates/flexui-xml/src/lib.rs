//! flexui-xml：XML 布局描述（L4/L5）。类 duilib 的运行时 XML 加载，
//! 支持分状态样式属性、v-if 条件渲染与平台谓词。

mod build;
mod parser;

pub use build::{load_str, Context, LoadError, LoadResult};
pub use parser::{parse, Element, ParseError};
