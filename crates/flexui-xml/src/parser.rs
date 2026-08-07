//! 极简 XML 解析器（自写，不依赖第三方）。
//!
//! 支持子集：元素、属性（单/双引号）、自闭合标签、注释 `<!-- -->`、
//! XML 声明 `<?xml ...?>`。足以表达 duilib 风格的界面描述。

/// 解析出的元素节点。
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
}

impl Element {
    /// 取某属性值。
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// 解析错误。
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "XML 解析错误: {}", self.0)
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            s: s.as_bytes(),
            i: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn starts_with(&self, pat: &str) -> bool {
        self.s[self.i..].starts_with(pat.as_bytes())
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    /// 跳过声明/注释/空白等无关内容。
    fn skip_misc(&mut self) {
        loop {
            self.skip_ws();
            if self.starts_with("<?") {
                // XML 声明
                while self.i < self.s.len() && !self.starts_with("?>") {
                    self.i += 1;
                }
                self.i += 2; // 跳过 ?>
            } else if self.starts_with("<!--") {
                self.i += 4;
                while self.i < self.s.len() && !self.starts_with("-->") {
                    self.i += 1;
                }
                self.i += 3; // 跳过 -->
            } else {
                break;
            }
        }
    }

    fn read_name(&mut self) -> String {
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.' || c == b':' {
                self.i += 1;
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.s[start..self.i]).into_owned()
    }

    fn parse_element(&mut self) -> Result<Element, ParseError> {
        self.skip_misc();
        if self.peek() != Some(b'<') {
            return Err(ParseError(format!("期望 '<' 于位置 {}", self.i)));
        }
        self.i += 1; // 吃掉 '<'
        let tag = self.read_name();
        if tag.is_empty() {
            return Err(ParseError("标签名为空".into()));
        }

        let mut attrs = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'/') => {
                    // 自闭合
                    self.i += 1;
                    if self.peek() != Some(b'>') {
                        return Err(ParseError("自闭合标签缺少 '>'".into()));
                    }
                    self.i += 1;
                    return Ok(Element {
                        tag,
                        attrs,
                        children: Vec::new(),
                    });
                }
                Some(b'>') => {
                    self.i += 1;
                    break;
                }
                Some(_) => {
                    let (k, v) = self.read_attr()?;
                    attrs.push((k, v));
                }
                None => return Err(ParseError("标签未闭合".into())),
            }
        }

        // 解析子节点，直到匹配的结束标签。
        let mut children = Vec::new();
        loop {
            self.skip_misc();
            if self.starts_with("</") {
                self.i += 2;
                let close = self.read_name();
                self.skip_ws();
                if self.peek() != Some(b'>') {
                    return Err(ParseError("结束标签缺少 '>'".into()));
                }
                self.i += 1;
                if close != tag {
                    return Err(ParseError(format!("标签不匹配: <{tag}> 对 </{close}>")));
                }
                break;
            } else if self.peek() == Some(b'<') {
                children.push(self.parse_element()?);
            } else if self.peek().is_none() {
                return Err(ParseError(format!("<{tag}> 未找到结束标签")));
            } else {
                // 文本节点：一期忽略（控件文本用属性表达）。
                self.i += 1;
            }
        }

        Ok(Element {
            tag,
            attrs,
            children,
        })
    }

    fn read_attr(&mut self) -> Result<(String, String), ParseError> {
        let name = self.read_name();
        if name.is_empty() {
            return Err(ParseError(format!("非法属性名于位置 {}", self.i)));
        }
        self.skip_ws();
        if self.peek() != Some(b'=') {
            return Err(ParseError(format!("属性 {name} 缺少 '='")));
        }
        self.i += 1;
        self.skip_ws();
        let quote = match self.peek() {
            Some(q @ b'"') | Some(q @ b'\'') => q,
            _ => return Err(ParseError(format!("属性 {name} 值缺少引号"))),
        };
        self.i += 1;
        let start = self.i;
        while let Some(c) = self.peek() {
            if c == quote {
                break;
            }
            self.i += 1;
        }
        let value = String::from_utf8_lossy(&self.s[start..self.i]).into_owned();
        self.i += 1; // 吃掉结束引号
        Ok((name, value))
    }
}

/// 解析 XML 文本，返回根元素。
pub fn parse(input: &str) -> Result<Element, ParseError> {
    let mut p = Parser::new(input);
    let root = p.parse_element()?;
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析嵌套与属性() {
        let xml = r#"<?xml version="1.0"?>
        <!-- 注释 -->
        <VBox spacing="10">
            <Label text="标题"/>
            <HBox>
                <Button name="ok" text="确定"/>
            </HBox>
        </VBox>"#;
        let el = parse(xml).unwrap();
        assert_eq!(el.tag, "VBox");
        assert_eq!(el.attr("spacing"), Some("10"));
        assert_eq!(el.children.len(), 2);
        assert_eq!(el.children[0].tag, "Label");
        assert_eq!(el.children[0].attr("text"), Some("标题"));
        assert_eq!(el.children[1].children[0].attr("name"), Some("ok"));
    }

    #[test]
    fn 标签不匹配报错() {
        let xml = "<VBox><Label></HBox></VBox>";
        assert!(parse(xml).is_err());
    }
}
