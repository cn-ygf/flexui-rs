//! 控件树本地化绑定与运行时刷新。

use flexui_i18n::{LocalizedStringResource, Localizer};

use crate::{visit_all_mut, Node, WidgetProperty};

/// XML/代码声明的本地化目标。资源 key 与当前显示字符串分开保存。
#[derive(Clone, Debug, PartialEq)]
pub enum LocalizationBinding {
    Text(LocalizedStringResource),
    Tooltip(LocalizedStringResource),
    Placeholder(LocalizedStringResource),
    Items(Vec<LocalizedStringResource>),
}

/// 按本地化环境刷新整棵控件树，保留焦点、选择、滚动和 Edit 输入内容。
pub fn apply_localizations(root: &mut Node, localizer: &Localizer) {
    visit_all_mut(root.as_mut(), &mut |widget| {
        let bindings = widget.base().localizations.clone();
        for binding in bindings {
            match binding {
                LocalizationBinding::Text(resource) => widget.set_text_value(localizer.text(resource)),
                LocalizationBinding::Tooltip(resource) => {
                    widget.base_mut().tooltip = Some(localizer.text(resource));
                }
                LocalizationBinding::Placeholder(resource) => {
                    widget.apply_property(WidgetProperty::Placeholder(localizer.text(resource)));
                }
                LocalizationBinding::Items(resources) => {
                    let items = resources.into_iter().map(|resource| localizer.text(resource)).collect();
                    widget.apply_property(WidgetProperty::Items(items));
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Edit, Label, Panel, Widget};

    #[test]
    fn refreshes_bound_text_and_placeholder_without_replacing_edit_value() {
        let localizer = Localizer::new("en").unwrap();
        localizer.load_json_str(r#"{"locale":"en","strings":{"title":"Settings","hint":"Phone"}}"#).unwrap();
        localizer.load_json_str(r#"{"locale":"zh-Hans","strings":{"title":"设置","hint":"手机号"}}"#).unwrap();
        let mut label = Label::new("");
        label.base_mut().localizations.push(LocalizationBinding::Text("title".into()));
        let mut edit = Edit::new().text("138");
        edit.base_mut().localizations.push(LocalizationBinding::Placeholder("hint".into()));
        let mut root: Node = Box::new(Panel::new().push(label).push(edit));
        localizer.set_locale("zh-Hans").unwrap();
        apply_localizations(&mut root, &localizer);
        assert_eq!(root.base().children[0].base().text, "设置");
        assert_eq!(root.base().children[1].base().text, "138");
    }
}
