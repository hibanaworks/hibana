//! Small strict JSON decoder for private CI inventory metadata. No dependencies.
use std::collections::BTreeMap;

#[derive(Debug, PartialEq)]
pub enum Json {
    Object(BTreeMap<String, Json>),
    Array(Vec<Json>),
    String(String),
    Number(String),
    Bool(bool),
    Null,
}
impl Json {
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(map) => map.get(key),
            _ => None,
        }
    }
}

struct Parser<'a> {
    text: &'a str,
    index: usize,
}
type Result<T> = std::result::Result<T, String>;
impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.index).copied()
    }
    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.index += 1;
        }
    }
    fn expect(&mut self, c: u8) -> Result<()> {
        if self.peek() != Some(c) {
            return Err(format!("expected JSON byte {c} at {}", self.index));
        }
        self.index += 1;
        Ok(())
    }
    fn hex(&mut self) -> Result<u32> {
        let mut result = 0;
        for _ in 0..4 {
            let Some(c) = self.peek().and_then(|c| char::from(c).to_digit(16)) else {
                return Err("invalid JSON Unicode escape".into());
            };
            self.index += 1;
            result = result * 16 + c;
        }
        Ok(result)
    }
    fn string(&mut self) -> Result<String> {
        self.expect(b'"')?;
        let mut result = String::new();
        loop {
            match self.peek() {
                None => return Err("unterminated JSON string".into()),
                Some(b'"') => {
                    self.index += 1;
                    return Ok(result);
                }
                Some(b'\\') => {
                    self.index += 1;
                    let escape = self.peek().ok_or("unterminated JSON escape")?;
                    self.index += 1;
                    result.push(match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let first = self.hex()?;
                            let scalar = if (0xd800..=0xdbff).contains(&first) {
                                self.expect(b'\\')?;
                                self.expect(b'u')?;
                                let second = self.hex()?;
                                if !(0xdc00..=0xdfff).contains(&second) {
                                    return Err("invalid JSON surrogate pair".into());
                                }
                                0x10000 + ((first - 0xd800) << 10) + second - 0xdc00
                            } else {
                                first
                            };
                            char::from_u32(scalar).ok_or("invalid JSON Unicode scalar")?
                        }
                        _ => return Err("invalid JSON escape".into()),
                    });
                }
                Some(0..=31) => return Err("unescaped JSON control character".into()),
                Some(_) => {
                    let c = self.text[self.index..].chars().next().unwrap();
                    self.index += c.len_utf8();
                    result.push(c);
                }
            }
        }
    }
    fn number(&mut self) -> Result<Json> {
        let start = self.index;
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        match self.peek() {
            Some(b'0') => self.index += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.index += 1;
                }
            }
            _ => return Err("invalid JSON number".into()),
        }
        if self.peek() == Some(b'.') {
            self.index += 1;
            let fraction = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == fraction {
                return Err("invalid JSON fraction".into());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.index += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.index += 1;
            }
            let exponent = self.index;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.index += 1;
            }
            if self.index == exponent {
                return Err("invalid JSON exponent".into());
            }
        }
        Ok(Json::Number(self.text[start..self.index].into()))
    }
    fn value(&mut self, depth: usize) -> Result<Json> {
        if depth > 128 {
            return Err("JSON nesting limit exceeded".into());
        }
        self.whitespace();
        match self.peek() {
            Some(b'{') => {
                self.index += 1;
                self.whitespace();
                let mut map = BTreeMap::new();
                if self.peek() == Some(b'}') {
                    self.index += 1;
                    return Ok(Json::Object(map));
                }
                loop {
                    self.whitespace();
                    let key = self.string()?;
                    self.whitespace();
                    self.expect(b':')?;
                    let value = self.value(depth + 1)?;
                    if map.insert(key, value).is_some() {
                        return Err("duplicate JSON object key".into());
                    }
                    self.whitespace();
                    if self.peek() == Some(b'}') {
                        self.index += 1;
                        return Ok(Json::Object(map));
                    }
                    self.expect(b',')?;
                }
            }
            Some(b'[') => {
                self.index += 1;
                self.whitespace();
                let mut array = Vec::new();
                if self.peek() == Some(b']') {
                    self.index += 1;
                    return Ok(Json::Array(array));
                }
                loop {
                    array.push(self.value(depth + 1)?);
                    self.whitespace();
                    if self.peek() == Some(b']') {
                        self.index += 1;
                        return Ok(Json::Array(array));
                    }
                    self.expect(b',')?;
                }
            }
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b't') if self.text[self.index..].starts_with("true") => {
                self.index += 4;
                Ok(Json::Bool(true))
            }
            Some(b'f') if self.text[self.index..].starts_with("false") => {
                self.index += 5;
                Ok(Json::Bool(false))
            }
            Some(b'n') if self.text[self.index..].starts_with("null") => {
                self.index += 4;
                Ok(Json::Null)
            }
            _ => Err(format!("invalid JSON value at {}", self.index)),
        }
    }
}

pub fn parse(text: &str) -> Result<Json> {
    let mut parser = Parser { text, index: 0 };
    let result = parser.value(0)?;
    parser.whitespace();
    if parser.index != text.len() {
        return Err("trailing JSON data".into());
    }
    Ok(result)
}

// Other private host tools reuse the JSON decoder without the Kani schema.
#[allow(dead_code)]
pub fn inventory_total(text: &str, expected_version: &str) -> Result<u64> {
    let inventory = parse(text)?;
    if inventory.get("kani-version") != Some(&Json::String(expected_version.into())) {
        return Err("Compiled Kani inventory version does not match .github/kani-version".into());
    }
    let totals = inventory
        .get("totals")
        .ok_or("Kani inventory is missing totals")?;
    let Some(Json::Number(standard)) = totals.get("standard-harnesses") else {
        return Err("Kani inventory requires an integer standard harness count".into());
    };
    let Some(Json::Number(contract)) = totals.get("contract-harnesses") else {
        return Err("Kani inventory requires an integer contract harness count".into());
    };
    // Parsing the original decimal integer text avoids floating-point rounding.
    let standard: u64 = standard
        .parse()
        .map_err(|_| "invalid standard harness count")?;
    if standard == 0 || !matches!(contract.as_str(), "0" | "-0") {
        return Err("Kani inventory must contain nonempty standard harnesses only".into());
    }
    Ok(standard)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_json_and_unicode_are_decoded_exactly() {
        let text = r#"{"kani-version":"0.67.0","totals":{"standard-harnesses":123,"contract-harnesses":0},"extra":[null,true,false,-1.23e+2,{"unicode":"\ud83d\ude80\n\\\"/"}]}"#;
        assert_eq!(inventory_total(text, "0.67.0"), Ok(123));
        assert_eq!(parse(r#""\u006bani""#), Ok(Json::String("kani".into())));
    }
    #[test]
    fn rejects_malformed_json() {
        for text in [
            "",
            "{}{}",
            "{\"x\":0,}",
            "[1,]",
            "[01]",
            "[+1]",
            "[.1]",
            "[1.]",
            "[1e]",
            "[NaN]",
            "[truefalse]",
            "{\"x\":1,\"x\":2}",
            "\"\u{1}\"",
            r#""\ud800""#,
            r#""\udc00""#,
            r#""\ud800\u0061""#,
            r#""\z""#,
        ] {
            assert!(parse(text).is_err(), "accepted {text}");
        }
        assert!(parse(&format!("{}{}", "[".repeat(130), "]".repeat(130))).is_err());
    }
    #[test]
    fn every_truncated_document_and_escape_fails_closed() {
        let document = r#"{"version":"\u006bani","metadata":[true,false,null,-1.25e+2,"\ud83d\ude80"],"totals":{"standard-harnesses":17,"contract-harnesses":0}}"#;
        assert!(parse(document).is_ok());
        for end in 0..document.len() {
            assert!(parse(&document[..end]).is_err(), "accepted prefix {end}");
        }
        for text in [
            r#""\"#,
            r#""\u"#,
            r#""\u1"#,
            r#""\u12"#,
            r#""\u123"#,
            r#""\ud83d\"#,
            r#""\ud83d\u"#,
        ] {
            assert!(parse(text).is_err(), "accepted {text}");
        }
    }
    #[test]
    fn rejects_inventory_mutants() {
        let good = r#"{"kani-version":"0.67.0","totals":{"standard-harnesses":123,"contract-harnesses":0}}"#;
        for (from, to) in [
            ("0.67.0", "0.68.0"),
            ("123", "0"),
            ("123", "-1"),
            ("123", "1.0"),
            ("123", "1e2"),
            ("123", "true"),
            ("123", "\"123\""),
            ("123", "null"),
            ("123", "18446744073709551616"),
            ("\"contract-harnesses\":0", "\"contract-harnesses\":1"),
            ("\"contract-harnesses\":0", "\"contract-harnesses\":false"),
            ("\"contract-harnesses\":0", "\"other\":0"),
            ("\"totals\"", "\"missing\""),
        ] {
            assert!(
                inventory_total(&good.replace(from, to), "0.67.0").is_err(),
                "accepted {from} => {to}"
            );
        }
        for text in [
            "null",
            "[]",
            "{}",
            r#"{"kani-version":"0.67.0","totals":[]}"#,
        ] {
            assert!(inventory_total(text, "0.67.0").is_err());
        }
    }
}
