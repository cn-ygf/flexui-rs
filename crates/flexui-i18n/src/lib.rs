//! 跨平台本地化：BCP 47 语言标签、回退链、参数插值、CLDR 复数与 String Catalog。

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

use flexui_resource::ResourceManager;
use intl_pluralrules::{PluralCategory, PluralRuleType, PluralRules};
use serde::Deserialize;
use serde_json::Value;
use unic_langid::LanguageIdentifier;

/// 可插入本地化消息的参数值。
#[derive(Clone, Debug, PartialEq)]
pub enum LocalizationValue {
    String(String),
    Number(f64),
    Bool(bool),
}

impl LocalizationValue {
    fn display(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Number(value) if value.fract() == 0.0 => format!("{value:.0}"),
            Self::Number(value) => value.to_string(),
            Self::Bool(value) => value.to_string(),
        }
    }
}

impl From<&str> for LocalizationValue {
    fn from(value: &str) -> Self { Self::String(value.to_owned()) }
}
impl From<String> for LocalizationValue {
    fn from(value: String) -> Self { Self::String(value) }
}
impl From<i32> for LocalizationValue {
    fn from(value: i32) -> Self { Self::Number(value as f64) }
}
impl From<i64> for LocalizationValue {
    fn from(value: i64) -> Self { Self::Number(value as f64) }
}
impl From<usize> for LocalizationValue {
    fn from(value: usize) -> Self { Self::Number(value as f64) }
}
impl From<f64> for LocalizationValue {
    fn from(value: f64) -> Self { Self::Number(value) }
}
impl From<bool> for LocalizationValue {
    fn from(value: bool) -> Self { Self::Bool(value) }
}

/// SwiftUI `LocalizedStringKey` 风格的稳定字符串标识。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LocalizedStringKey(String);

impl LocalizedStringKey {
    pub fn new(key: impl Into<String>) -> Self { Self(key.into()) }
    pub fn as_str(&self) -> &str { &self.0 }
}

impl From<&str> for LocalizedStringKey {
    fn from(value: &str) -> Self { Self::new(value) }
}
impl From<String> for LocalizedStringKey {
    fn from(value: String) -> Self { Self::new(value) }
}

/// 可携带默认值、表名和插值参数的本地化资源。
#[derive(Clone, Debug, PartialEq)]
pub struct LocalizedStringResource {
    pub key: LocalizedStringKey,
    pub default_value: Option<String>,
    pub table: String,
    pub arguments: BTreeMap<String, LocalizationValue>,
}

impl LocalizedStringResource {
    pub fn new(key: impl Into<LocalizedStringKey>) -> Self {
        Self { key: key.into(), default_value: None, table: "Localizable".into(), arguments: BTreeMap::new() }
    }
    pub fn default_value(mut self, value: impl Into<String>) -> Self {
        self.default_value = Some(value.into()); self
    }
    pub fn table(mut self, table: impl Into<String>) -> Self { self.table = table.into(); self }
    pub fn arg(mut self, name: impl Into<String>, value: impl Into<LocalizationValue>) -> Self {
        self.arguments.insert(name.into(), value.into()); self
    }
}

impl From<LocalizedStringKey> for LocalizedStringResource {
    fn from(value: LocalizedStringKey) -> Self { Self::new(value) }
}
impl From<&str> for LocalizedStringResource {
    fn from(value: &str) -> Self { Self::new(value) }
}

#[derive(Clone, Debug, PartialEq)]
enum Message {
    Text(String),
    Plural { variable: String, forms: HashMap<String, String> },
}

#[derive(Clone, Debug, Default)]
struct Catalog {
    tables: HashMap<String, HashMap<String, Message>>,
}

#[derive(Clone, Debug)]
struct State {
    revision: u64,
    locale: LanguageIdentifier,
    development_locale: LanguageIdentifier,
    catalogs: HashMap<String, Catalog>,
}

/// 线程安全、可克隆的本地化环境。克隆实例共享当前语言和目录。
#[derive(Clone, Debug)]
pub struct Localizer(Arc<RwLock<State>>);

impl Localizer {
    pub fn new(development_locale: &str) -> Result<Self, I18nError> {
        let development_locale = parse_locale(development_locale)?;
        let locale = sys_locale::get_locale()
            .as_deref()
            .and_then(|value| parse_locale(value).ok())
            .unwrap_or_else(|| development_locale.clone());
        Ok(Self(Arc::new(RwLock::new(State {
            revision: 0,
            locale,
            development_locale,
            catalogs: HashMap::new(),
        }))))
    }

    pub fn locale(&self) -> String { self.0.read().unwrap().locale.to_string() }
    pub fn revision(&self) -> u64 { self.0.read().unwrap().revision }

    pub fn set_locale(&self, locale: &str) -> Result<(), I18nError> {
        let locale = parse_locale(locale)?;
        let mut state = self.0.write().unwrap();
        if state.locale != locale {
            state.locale = locale;
            state.revision = state.revision.wrapping_add(1);
        }
        Ok(())
    }

    /// 重新采用操作系统当前语言；无法读取时回退到开发语言。
    pub fn set_system_locale(&self) {
        let locale = sys_locale::get_locale()
            .as_deref()
            .and_then(|value| parse_locale(value).ok());
        let mut state = self.0.write().unwrap();
        let locale = locale.unwrap_or_else(|| state.development_locale.clone());
        if state.locale != locale {
            state.locale = locale;
            state.revision = state.revision.wrapping_add(1);
        }
    }

    /// 加载轻量 JSON：`{"locale":"zh-Hans","strings":{"key":"值"}}`。
    pub fn load_json_str(&self, json: &str) -> Result<(), I18nError> {
        let source: JsonCatalog = serde_json::from_str(json).map_err(I18nError::Json)?;
        let locale = parse_locale(&source.locale)?.to_string();
        let table_name = source.table.unwrap_or_else(default_table);
        let mut messages = HashMap::new();
        for (key, value) in source.strings {
            messages.insert(key, parse_message(value)?);
        }
        let mut state = self.0.write().unwrap();
        state.catalogs.entry(locale).or_default().tables
            .entry(table_name).or_default().extend(messages);
        state.revision = state.revision.wrapping_add(1);
        Ok(())
    }

    pub fn load_json_res(&self, resources: &ResourceManager, path: &str) -> Result<(), I18nError> {
        let json = resources.read_string(path).map_err(|error| I18nError::Resource(error.to_string()))?;
        self.load_json_str(&json)
    }

    /// 加载 Apple String Catalog (`.xcstrings`) 的常用字符串与复数 variation。
    pub fn load_xcstrings_str(&self, json: &str) -> Result<(), I18nError> {
        let root: Value = serde_json::from_str(json).map_err(I18nError::Json)?;
        let development = root.get("sourceLanguage").and_then(Value::as_str).unwrap_or("en");
        let strings = root.get("strings").and_then(Value::as_object)
            .ok_or_else(|| I18nError::Format("xcstrings 缺少 strings".into()))?;
        let mut state = self.0.write().unwrap();
        state.development_locale = parse_locale(development)?;
        for (key, entry) in strings {
            let Some(localizations) = entry.get("localizations").and_then(Value::as_object) else { continue };
            for (locale, localization) in localizations {
                let locale = parse_locale(locale)?.to_string();
                let Some(message) = parse_xc_message(localization) else { continue };
                state.catalogs.entry(locale).or_default().tables
                    .entry(default_table()).or_default().insert(key.clone(), message);
            }
        }
        state.revision = state.revision.wrapping_add(1);
        Ok(())
    }

    pub fn load_xcstrings_res(&self, resources: &ResourceManager, path: &str) -> Result<(), I18nError> {
        let json = resources.read_string(path).map_err(|error| I18nError::Resource(error.to_string()))?;
        self.load_xcstrings_str(&json)
    }

    pub fn text(&self, resource: impl Into<LocalizedStringResource>) -> String {
        let resource = resource.into();
        let state = self.0.read().unwrap();
        let message = fallback_locales(&state.locale, &state.development_locale)
            .into_iter()
            .find_map(|locale| state.catalogs.get(&locale)?.tables.get(&resource.table)?.get(resource.key.as_str()));
        let template = match message {
            Some(Message::Text(value)) => value.clone(),
            Some(Message::Plural { variable, forms }) => {
                select_plural(&state.locale, variable, forms, &resource.arguments)
            }
            None => resource.default_value.clone().unwrap_or_else(|| resource.key.as_str().to_owned()),
        };
        interpolate(&template, &resource.arguments)
    }

    pub fn contains(&self, key: &str, table: &str) -> bool {
        let state = self.0.read().unwrap();
        fallback_locales(&state.locale, &state.development_locale).into_iter().any(|locale| {
            state.catalogs.get(&locale).and_then(|catalog| catalog.tables.get(table))
                .is_some_and(|messages| messages.contains_key(key))
        })
    }
}

#[derive(Debug)]
pub enum I18nError {
    InvalidLocale(String),
    Json(serde_json::Error),
    Format(String),
    Resource(String),
}

impl std::fmt::Display for I18nError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLocale(value) => write!(f, "无效语言标签: {value}"),
            Self::Json(error) => write!(f, "本地化 JSON 错误: {error}"),
            Self::Format(error) => write!(f, "本地化格式错误: {error}"),
            Self::Resource(error) => write!(f, "本地化资源错误: {error}"),
        }
    }
}
impl std::error::Error for I18nError {}

#[derive(Deserialize)]
struct JsonCatalog {
    locale: String,
    #[serde(default)]
    table: Option<String>,
    strings: HashMap<String, Value>,
}

fn default_table() -> String { "Localizable".into() }

fn parse_locale(value: &str) -> Result<LanguageIdentifier, I18nError> {
    value.replace('_', "-").parse().map_err(|_| I18nError::InvalidLocale(value.into()))
}

fn fallback_locales(locale: &LanguageIdentifier, development: &LanguageIdentifier) -> Vec<String> {
    let mut result = Vec::new();
    for candidate in [locale, development] {
        let candidate = candidate.to_string();
        push_locale_fallback(&mut result, &candidate);
    }
    result
}

fn push_locale_fallback(result: &mut Vec<String>, candidate: &str) {
    let mut parts: Vec<&str> = candidate.split('-').collect();
    while parts.len() > 1 {
        push_valid_locale(result, &parts.join("-"));
        parts.pop();
    }
    if let Some(script) = inferred_chinese_script(candidate) {
        push_valid_locale(result, script);
    }
    if let Some(language) = parts.first() {
        push_valid_locale(result, language);
    }
}

fn push_valid_locale(result: &mut Vec<String>, locale: &str) {
    if locale.parse::<LanguageIdentifier>().is_ok() && !result.iter().any(|item| item == locale) {
        result.push(locale.to_owned());
    }
}

fn inferred_chinese_script(locale: &str) -> Option<&'static str> {
    let mut subtags = locale.split('-');
    if subtags.next()? != "zh" {
        return None;
    }
    let subtags: Vec<&str> = subtags.collect();
    if subtags.iter().any(|subtag| *subtag == "Hans" || *subtag == "Hant") {
        return None;
    }
    if subtags.iter().any(|subtag| matches!(*subtag, "TW" | "HK" | "MO")) {
        Some("zh-Hant")
    } else if subtags.iter().any(|subtag| matches!(*subtag, "CN" | "SG")) {
        Some("zh-Hans")
    } else {
        None
    }
}

fn parse_message(value: Value) -> Result<Message, I18nError> {
    if let Some(text) = value.as_str() { return Ok(Message::Text(text.into())); }
    let object = value.as_object().ok_or_else(|| I18nError::Format("字符串条目必须是文本或对象".into()))?;
    if let Some(text) = object.get("value").and_then(Value::as_str) { return Ok(Message::Text(text.into())); }
    let variable = object.get("variable").and_then(Value::as_str).unwrap_or("count").to_owned();
    let forms_value = object.get("plural").or_else(|| object.get("variations"))
        .and_then(Value::as_object).ok_or_else(|| I18nError::Format("复数条目缺少 plural".into()))?;
    let forms = forms_value.iter().filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned()))).collect();
    Ok(Message::Plural { variable, forms })
}

fn parse_xc_message(localization: &Value) -> Option<Message> {
    if let Some(value) = localization.pointer("/stringUnit/value").and_then(Value::as_str) {
        return Some(Message::Text(value.into()));
    }
    let plural = localization.pointer("/variations/plural")?.as_object()?;
    let mut forms = HashMap::new();
    for (category, unit) in plural {
        let value = unit.pointer("/stringUnit/value").and_then(Value::as_str)?;
        forms.insert(category.clone(), value.into());
    }
    Some(Message::Plural { variable: "count".into(), forms })
}

fn select_plural(
    locale: &LanguageIdentifier,
    variable: &str,
    forms: &HashMap<String, String>,
    arguments: &BTreeMap<String, LocalizationValue>,
) -> String {
    let number = match arguments.get(variable) {
        Some(LocalizationValue::Number(value)) => *value,
        _ => 0.0,
    };
    let operand = if number.fract() == 0.0 { format!("{number:.0}") } else { number.to_string() };
    let rules = PluralRules::create(locale.clone(), PluralRuleType::CARDINAL).ok().or_else(|| {
        locale.language.to_string().parse::<LanguageIdentifier>().ok()
            .and_then(|language| PluralRules::create(language, PluralRuleType::CARDINAL).ok())
    });
    let category = rules.and_then(|rules| rules.select(operand.as_str()).ok())
        .map(plural_name).unwrap_or("other");
    forms.get(category).or_else(|| forms.get("other")).cloned().unwrap_or_default()
}

fn plural_name(category: PluralCategory) -> &'static str {
    match category {
        PluralCategory::ZERO => "zero",
        PluralCategory::ONE => "one",
        PluralCategory::TWO => "two",
        PluralCategory::FEW => "few",
        PluralCategory::MANY => "many",
        PluralCategory::OTHER => "other",
    }
}

fn interpolate(template: &str, arguments: &BTreeMap<String, LocalizationValue>) -> String {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.char_indices().peekable();
    while let Some((_, ch)) = chars.next() {
        if ch == '{' {
            if chars.peek().is_some_and(|(_, next)| *next == '{') {
                chars.next();
                result.push('{');
                continue;
            }
            let mut name = String::new();
            let mut closed = false;
            for (_, next) in chars.by_ref() {
                if next == '}' {
                    closed = true;
                    break;
                }
                name.push(next);
            }
            if closed {
                if let Some(value) = arguments.get(&name) {
                    result.push_str(&value.display());
                } else {
                    result.push('{');
                    result.push_str(&name);
                    result.push('}');
                }
            } else {
                result.push('{');
                result.push_str(&name);
            }
        } else if ch == '}' && chars.peek().is_some_and(|(_, next)| *next == '}') {
            chars.next();
            result.push('}');
        } else {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_interpolation_and_plural_follow_locale() {
        let localizer = Localizer::new("en").unwrap();
        localizer.load_json_str(r#"{"locale":"en","strings":{"hello":"Hello, {name}","items":{"variable":"count","plural":{"one":"{count} item","other":"{count} items"}}}}"#).unwrap();
        localizer.load_json_str(r#"{"locale":"zh-Hans","strings":{"hello":"你好，{name}","items":{"variable":"count","plural":{"other":"{count} 个项目"}}}}"#).unwrap();
        localizer.set_locale("en-US").unwrap();
        assert_eq!(localizer.text(LocalizedStringResource::new("hello").arg("name", "Ada")), "Hello, Ada");
        assert_eq!(localizer.text(LocalizedStringResource::new("items").arg("count", 1)), "1 item");
        assert_eq!(localizer.text(LocalizedStringResource::new("items").arg("count", 2)), "2 items");
        localizer.set_locale("zh-Hans-CN").unwrap();
        assert_eq!(localizer.text(LocalizedStringResource::new("items").arg("count", 2)), "2 个项目");
    }

    #[test]
    fn xcstrings_regular_and_plural_are_supported() {
        let localizer = Localizer::new("en").unwrap();
        localizer.load_xcstrings_str(r#"{"sourceLanguage":"en","strings":{"title":{"localizations":{"en":{"stringUnit":{"state":"translated","value":"Settings"}},"zh-Hans":{"stringUnit":{"state":"translated","value":"设置"}}}},"files":{"localizations":{"en":{"variations":{"plural":{"one":{"stringUnit":{"value":"{count} file"}},"other":{"stringUnit":{"value":"{count} files"}}}}}}}}}"#).unwrap();
        localizer.set_locale("zh-Hans").unwrap();
        assert_eq!(localizer.text("title"), "设置");
        localizer.set_locale("en").unwrap();
        assert_eq!(localizer.text(LocalizedStringResource::new("files").arg("count", 3)), "3 files");
    }

    #[test]
    fn missing_key_uses_default_value_then_key() {
        let localizer = Localizer::new("en").unwrap();
        assert_eq!(
            localizer.text(LocalizedStringResource::new("missing").default_value("Fallback")),
            "Fallback"
        );
        assert_eq!(localizer.text("missing"), "missing");
    }

    #[test]
    fn malformed_locale_is_rejected() {
        assert!(matches!(Localizer::new("not a locale"), Err(I18nError::InvalidLocale(_))));
        let localizer = Localizer::new("en").unwrap();
        assert!(matches!(localizer.set_locale("en-@"), Err(I18nError::InvalidLocale(_))));
    }

    #[test]
    fn region_and_script_fallback_keep_the_most_specific_match() {
        let localizer = Localizer::new("en").unwrap();
        localizer.load_json_str(r#"{"locale":"zh","strings":{"name":"中文"}}"#).unwrap();
        localizer.load_json_str(r#"{"locale":"zh-Hant","strings":{"name":"繁體中文"}}"#).unwrap();
        localizer.set_locale("zh-Hant-TW").unwrap();
        assert_eq!(localizer.text("name"), "繁體中文");
        localizer.set_locale("zh-Hans-CN").unwrap();
        assert_eq!(localizer.text("name"), "中文");
    }

    #[test]
    fn chinese_regions_infer_simplified_or_traditional_script() {
        let localizer = Localizer::new("en").unwrap();
        localizer.load_json_str(r#"{"locale":"zh-Hans","strings":{"name":"简体中文"}}"#).unwrap();
        localizer.load_json_str(r#"{"locale":"zh-Hant","strings":{"name":"繁體中文"}}"#).unwrap();
        for locale in ["zh-TW", "zh-HK", "zh-MO"] {
            localizer.set_locale(locale).unwrap();
            assert_eq!(localizer.text("name"), "繁體中文");
        }
        for locale in ["zh-CN", "zh-SG"] {
            localizer.set_locale(locale).unwrap();
            assert_eq!(localizer.text("name"), "简体中文");
        }
    }

    #[test]
    fn catalogs_merge_and_escaped_braces_stay_literal() {
        let localizer = Localizer::new("en").unwrap();
        localizer.load_json_str(r#"{"locale":"en","strings":{"first":"First"}}"#).unwrap();
        localizer.load_json_str(r#"{"locale":"en","strings":{"second":"{{name}} = {name}"}}"#).unwrap();
        localizer.set_locale("en").unwrap();
        assert_eq!(localizer.text("first"), "First");
        assert_eq!(
            localizer.text(LocalizedStringResource::new("second").arg("name", "Ada")),
            "{name} = Ada"
        );
    }
}
