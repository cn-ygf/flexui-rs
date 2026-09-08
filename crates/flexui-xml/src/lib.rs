//! flexui-xml：XML 布局描述（L4/L5）。类 duilib 的运行时 XML 加载，
//! 支持分状态样式属性、v-if 条件渲染与平台谓词。
//!
//! 可在文档根节点下声明复用属性：
//! ```xml
//! <VBox>
//!   <Styles>
//!     <Default type="Button" height="32"/>
//!     <Style name="primary" normal-bgcolor="#3478F6"/>
//!   </Styles>
//!   <Button style="primary" height="40"/>
//! </VBox>
//! ```
//! 覆盖顺序固定为：控件类型默认属性 → `style` 中从左到右列出的命名样式 → 节点内联属性。

mod build;
mod native_menu;
mod parser;

pub use build::{
    build_fragment_res, build_fragment_str, build_fragment_str_res, load_res, load_str,
    load_window_res, load_window_str, Context, LoadError, LoadResult, WidgetFactory,
    WidgetFactoryContext, WindowDoc,
};
pub use native_menu::{load_native_menu_res, load_native_menu_str};
pub use parser::{parse, Element, ParseError};
