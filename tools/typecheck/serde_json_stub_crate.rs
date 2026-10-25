//! serde_json 的最小可用替身，编译成独立 crate 供离线检查使用。
//!
//! 与 flate2 桩不同，这里不能只做「签名齐备但行为为空」——blockstate 的
//! 解析逻辑是整条导入链路里最微妙的一段（嵌套 OR/AND、数组形式、
//! 键顺序），必须能在本地真正测试。因此这里实现一个最小但正确的
//! JSON 解析器，覆盖被检查代码实际用到的 API 表面。
//!
//! 这是测试设施而非产品代码：产品用真实的 serde_json（CI 可用）。

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Map<String, Value>),
}

/// 对象类型。用 BTreeMap 以保证遍历顺序稳定（真实 serde_json 默认不保证，
/// 但被测代码已自行排序，这里稳定只会让测试更可复现）。
pub type Map<K, V> = BTreeMap<K, V>;

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }
    /// 非负整数：小数部分非零时返回 None，与 serde_json 行为一致
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Number(n) if *n >= 0.0 && n.fract() == 0.0 => Some(*n as u64),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Number(n) if n.fract() == 0.0 => Some(*n as i64),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_object(&self) -> Option<&Map<String, Value>> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }
    pub fn get<I: AsRef<str>>(&self, key: I) -> Option<&Value> {
        match self {
            Value::Object(m) => m.get(key.as_ref()),
            _ => None,
        }
    }
    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }
    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }
    pub fn is_string(&self) -> bool {
        matches!(self, Value::String(_))
    }
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Number(_))
    }
    pub fn is_boolean(&self) -> bool {
        matches!(self, Value::Bool(_))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    msg: String,
    line: usize,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (第 {} 行)", self.msg, self.line)
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, i: 0 }
    }

    fn line(&self) -> usize {
        self.b[..self.i.min(self.b.len())].iter().filter(|c| **c == b'\n').count() + 1
    }

    fn err<T>(&self, msg: &str) -> Result<T, Error> {
        Err(Error { msg: msg.to_string(), line: self.line() })
    }

    fn skip_ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\r' | b'\n') {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> Result<(), Error> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            self.err(&format!("期望字符 {}", c as char))
        }
    }

    fn parse_value(&mut self) -> Result<Value, Error> {
        self.skip_ws();
        match self.peek() {
            None => self.err("内容意外结束"),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => Ok(Value::String(self.parse_string()?)),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(_) => self.parse_number(),
        }
    }

    fn literal(&mut self, word: &str, v: Value) -> Result<Value, Error> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            self.err(&format!("无效的字面量，期望 {word}"))
        }
    }

    fn parse_object(&mut self) -> Result<Value, Error> {
        self.expect(b'{')?;
        let mut m: Map<String, Value> = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Value::Object(m));
        }
        loop {
            self.skip_ws();
            let k = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            let v = self.parse_value()?;
            m.insert(k, v);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                }
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::Object(m));
                }
                _ => return self.err("对象中期望 , 或 }"),
            }
        }
    }

    fn parse_array(&mut self) -> Result<Value, Error> {
        self.expect(b'[')?;
        let mut a = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Value::Array(a));
        }
        loop {
            a.push(self.parse_value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                }
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::Array(a));
                }
                _ => return self.err("数组中期望 , 或 ]"),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, Error> {
        self.expect(b'"')?;
        let mut s = String::new();
        loop {
            let c = match self.peek() {
                Some(c) => c,
                None => return self.err("字符串未闭合"),
            };
            self.i += 1;
            match c {
                b'"' => return Ok(s),
                b'\\' => {
                    let e = match self.peek() {
                        Some(e) => e,
                        None => return self.err("转义序列未完成"),
                    };
                    self.i += 1;
                    match e {
                        b'"' => s.push('"'),
                        b'\\' => s.push('\\'),
                        b'/' => s.push('/'),
                        b'b' => s.push('\u{8}'),
                        b'f' => s.push('\u{c}'),
                        b'n' => s.push('\n'),
                        b'r' => s.push('\r'),
                        b't' => s.push('\t'),
                        b'u' => {
                            let cp = self.hex4()?;
                            // 代理对：高位后必须紧跟低位转义
                            if (0xD800..0xDC00).contains(&cp) {
                                if self.peek() == Some(b'\\') {
                                    self.i += 1;
                                    self.expect(b'u')?;
                                    let lo = self.hex4()?;
                                    let c = 0x10000
                                        + ((cp - 0xD800) << 10)
                                        + (lo - 0xDC00);
                                    s.push(
                                        char::from_u32(c).unwrap_or('\u{FFFD}'),
                                    );
                                } else {
                                    s.push('\u{FFFD}');
                                }
                            } else {
                                s.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                            }
                        }
                        _ => return self.err("未知的转义字符"),
                    }
                }
                // UTF-8 多字节：直接按字节累积，最后整体校验
                _ => {
                    let start = self.i - 1;
                    let len = if c < 0x80 {
                        1
                    } else if c >> 5 == 0b110 {
                        2
                    } else if c >> 4 == 0b1110 {
                        3
                    } else {
                        4
                    };
                    self.i = start + len;
                    if self.i > self.b.len() {
                        return self.err("字符串中的 UTF-8 序列不完整");
                    }
                    match std::str::from_utf8(&self.b[start..self.i]) {
                        Ok(t) => s.push_str(t),
                        Err(_) => return self.err("字符串中的 UTF-8 无效"),
                    }
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, Error> {
        if self.i + 4 > self.b.len() {
            return self.err("\\u 转义不完整");
        }
        let s = match std::str::from_utf8(&self.b[self.i..self.i + 4]) {
            Ok(s) => s,
            Err(_) => return self.err("\\u 转义不是有效 UTF-8"),
        };
        let v = u32::from_str_radix(s, 16).map_err(|_| Error {
            msg: "\\u 转义不是十六进制".into(),
            line: self.line(),
        })?;
        self.i += 4;
        Ok(v)
    }

    fn parse_number(&mut self) -> Result<Value, Error> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-') {
                self.i += 1;
            } else {
                break;
            }
        }
        let s = match std::str::from_utf8(&self.b[start..self.i]) {
            Ok(s) => s,
            Err(_) => return self.err("数字不是有效 UTF-8"),
        };
        match s.parse::<f64>() {
            Ok(n) => Ok(Value::Number(n)),
            Err(_) => self.err(&format!("无效的数字 {s:?}")),
        }
    }
}

/// 解析 JSON 字节。
///
/// 真实 serde_json 的签名是 `from_slice<T: Deserialize>(&[u8]) -> Result<T>`，
/// 桩里为了便于使用而固定返回 Value；但**必须保留泛型参数**，
/// 否则调用写成 `from_slice::<Value>(..)` 时会与真实 API 不一致，
/// 本地检查通过而 CI 失败。
pub fn from_slice<T: FromValue>(b: &[u8]) -> Result<T, Error> {
    let v = parse_slice(b)?;
    T::from_value(v)
}

/// 允许把解析结果当作 Value 或其他包装类型
pub trait FromValue: Sized {
    fn from_value(v: Value) -> Result<Self, Error>;
}

impl FromValue for Value {
    fn from_value(v: Value) -> Result<Self, Error> {
        Ok(v)
    }
}

/// 不做类型转换的解析（内部用）
pub fn parse_slice(b: &[u8]) -> Result<Value, Error> {
    let mut p = Parser::new(b);
    let v = p.parse_value()?;
    p.skip_ws();
    if p.i != b.len() {
        return p.err("末尾存在多余内容");
    }
    Ok(v)
}

/// 解析 JSON 字符串
pub fn from_str<T: FromValue>(s: &str) -> Result<T, Error> {
    from_slice(s.as_bytes())
}

/// 便于构造值（被测代码的测试会用到）
pub fn json(v: Value) -> Value {
    v
}

/// 把值序列化成 JSON。
///
/// 真实 serde_json 的签名是 `to_value<T: Serialize>(&T) -> Result<Value>`，
/// 由派生宏生成的实现真正执行序列化（含 `rename_all` 的字段改名）。
/// 桩里没有派生宏，因此这里对任意类型都返回一个**空对象**——
/// 它足以让 `to_value(..)` 通过类型检查，也让基于它的测试能在本地编译，
/// 但**不会**验证重命名行为；那部分只在 CI 的真实 serde 下生效。
pub fn to_value<T: ?Sized>(_v: &T) -> Result<Value, Error> {
    Ok(Value::Object(Map::new()))
}

/// 把值序列化成 JSON 文本。桩不真正序列化，只返回空对象文本。
pub fn to_string<T: ?Sized>(_v: &T) -> Result<String, Error> {
    Ok("{}".to_string())
}

/// 同上，带缩进版本。
pub fn to_string_pretty<T: ?Sized>(_v: &T) -> Result<String, Error> {
    Ok("{}".to_string())
}

/// 构造 JSON 值。桩里统一返回 Null，只保证类型能过。
#[macro_export]
macro_rules! json {
    ($($t:tt)*) => { $crate::Value::Null };
}
