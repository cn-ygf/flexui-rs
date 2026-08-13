//! 系统原生菜单 XML 解析。

use std::collections::HashSet;

use flexui_core::{NativeMenu, NativeMenuEntry, NativeMenuItem, NativeSubmenu};
use flexui_resource::ResourceManager;

use crate::build::{resolve_image, resolve_localized};
use crate::parser::Element;
use crate::{parser, Context, LoadError};

/// 从 XML 字符串动态创建原生菜单。
pub fn load_native_menu_str(xml: &str, ctx: &Context) -> Result<NativeMenu, LoadError> {
    load_native_menu(xml, ctx, None)
}

/// 从资源管理器加载原生菜单，图标同样通过资源管理器解析。
pub fn load_native_menu_res(
    resources: &ResourceManager,
    path: &str,
    ctx: &Context,
) -> Result<NativeMenu, LoadError> {
    let xml = resources
        .read_string(path)
        .map_err(|error| LoadError(format!("读取原生菜单失败: {error}")))?;
    load_native_menu(&xml, ctx, Some(resources))
}

fn load_native_menu(
    xml: &str,
    ctx: &Context,
    resources: Option<&ResourceManager>,
) -> Result<NativeMenu, LoadError> {
    let root = parser::parse(xml)?;
    if !root.tag.eq_ignore_ascii_case("NativeMenu") {
        return Err(LoadError(format!(
            "原生菜单根标签必须是 <NativeMenu>，实际为 <{}>",
            root.tag
        )));
    }
    let mut ids = HashSet::new();
    Ok(NativeMenu::with_items(parse_entries(
        &root.children,
        ctx,
        resources,
        &mut ids,
    )?))
}

fn parse_entries(
    elements: &[Element],
    ctx: &Context,
    resources: Option<&ResourceManager>,
    ids: &mut HashSet<String>,
) -> Result<Vec<NativeMenuEntry>, LoadError> {
    let mut entries = Vec::new();
    for element in elements {
        if let Some(condition) = element.attr("v-if") {
            let expected = !condition.starts_with('!');
            let key = condition.strip_prefix('!').unwrap_or(condition);
            if ctx.get(key) != expected {
                continue;
            }
        }
        match element.tag.to_ascii_lowercase().as_str() {
            "separator" => entries.push(NativeMenuEntry::Separator),
            "menuitem" => {
                let id = required_attr(element, "id")?.to_owned();
                if !ids.insert(id.clone()) {
                    return Err(LoadError(format!("原生菜单命令 id 重复: {id}")));
                }
                let mut item = NativeMenuItem::new(id, menu_text(element, ctx)?);
                item.enabled = bool_attr(element, "enabled", true)?;
                item.checked = bool_attr(element, "checked", false)?;
                item.shortcut = element.attr("shortcut").map(str::to_owned);
                item.icon = element
                    .attr("icon")
                    .filter(|path| !path.is_empty())
                    .map(|path| resolve_image(resources, path));
                entries.push(item.into());
            }
            "submenu" => {
                let mut submenu = NativeSubmenu::new(menu_text(element, ctx)?);
                submenu.enabled = bool_attr(element, "enabled", true)?;
                submenu.icon = element
                    .attr("icon")
                    .filter(|path| !path.is_empty())
                    .map(|path| resolve_image(resources, path));
                submenu.items = parse_entries(&element.children, ctx, resources, ids)?;
                entries.push(submenu.into());
            }
            tag => {
                return Err(LoadError(format!(
                    "原生菜单不支持 <{}> 子标签",
                    if tag.is_empty() { &element.tag } else { tag }
                )))
            }
        }
    }
    Ok(entries)
}

fn menu_text(element: &Element, ctx: &Context) -> Result<String, LoadError> {
    if let Some(text) = element.attr("text-verbatim") {
        return Ok(text.to_owned());
    }
    let text = required_attr(element, "text")?;
    Ok(resolve_localized(text, element.attr("text-args"), ctx.localizer()).0)
}

fn required_attr<'a>(element: &'a Element, name: &str) -> Result<&'a str, LoadError> {
    element
        .attr(name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| LoadError(format!("<{}> 缺少 {name} 属性", element.tag)))
}

fn bool_attr(element: &Element, name: &str, default: bool) -> Result<bool, LoadError> {
    match element.attr(name) {
        None => Ok(default),
        Some("true" | "1" | "yes") => Ok(true),
        Some("false" | "0" | "no") => Ok(false),
        Some(value) => Err(LoadError(format!(
            "<{}> 的 {name} 不是布尔值: {value}",
            element.tag
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flexui_core::{ImageSource, NativeMenuEntry};
    use flexui_resource::{ResourceManager, ResourceProvider};
    use std::collections::HashMap;

    struct MemoryProvider(HashMap<String, Vec<u8>>);

    impl ResourceProvider for MemoryProvider {
        fn read(&self, path: &str) -> Result<Vec<u8>, flexui_resource::ResError> {
            self.0
                .get(path)
                .cloned()
                .ok_or_else(|| flexui_resource::ResError::NotFound(path.to_owned()))
        }
    }

    #[test]
    fn 解析菜单项分割线子菜单与状态() {
        let menu = load_native_menu_str(
            r#"<NativeMenu>
                <MenuItem id="new" text-verbatim="New" shortcut="CmdOrCtrl+N" checked="true"/>
                <Separator/>
                <Submenu text-verbatim="Share">
                    <MenuItem id="copy_link" text-verbatim="Copy link" enabled="false"/>
                </Submenu>
            </NativeMenu>"#,
            &Context::new(),
        )
        .unwrap();
        assert_eq!(menu.items.len(), 3);
        let NativeMenuEntry::Item(item) = &menu.items[0] else {
            panic!("第一项应为命令");
        };
        assert!(item.checked);
        assert_eq!(item.shortcut.as_deref(), Some("CmdOrCtrl+N"));
    }

    #[test]
    fn 资源菜单图标读取为内存字节() {
        let mut files = HashMap::new();
        files.insert(
            "menu.xml".into(),
            br#"<NativeMenu><MenuItem id="open" text-verbatim="Open" icon="open.png"/></NativeMenu>"#.to_vec(),
        );
        files.insert("open.png".into(), vec![1, 2, 3]);
        let mut resources = ResourceManager::new();
        resources.mount(MemoryProvider(files));
        let menu = load_native_menu_res(&resources, "menu.xml", &Context::new()).unwrap();
        let NativeMenuEntry::Item(item) = &menu.items[0] else {
            panic!("第一项应为命令");
        };
        assert!(matches!(item.icon, Some(ImageSource::Bytes(_))));
    }

    #[test]
    fn 拒绝重复命令_id() {
        let error = load_native_menu_str(
            r#"<NativeMenu>
                <MenuItem id="same" text-verbatim="One"/>
                <MenuItem id="same" text-verbatim="Two"/>
            </NativeMenu>"#,
            &Context::new(),
        )
        .err()
        .unwrap();
        assert!(error.0.contains("重复"));
    }
}
