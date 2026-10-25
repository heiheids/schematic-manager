//! Minecraft NBT（大端序）读写 + gzip 容器。
//!
//! 语义与 legacy-cpp/nbt.cpp 保持一致：
//!   - 全部整数/浮点均为大端序
//!   - Compound 使用有序 Vec 而非 HashMap，保证序列化后字段顺序不变
//!     （投影文件的字节级往返依赖这一点）
//!   - gzip 容器自动识别：若前两字节为 1f 8b 则先解压

use crate::error::{AppError, AppResult};
use std::io::{Read, Write};

pub const TAG_END: u8 = 0;
pub const TAG_BYTE: u8 = 1;
pub const TAG_SHORT: u8 = 2;
pub const TAG_INT: u8 = 3;
pub const TAG_LONG: u8 = 4;
pub const TAG_FLOAT: u8 = 5;
pub const TAG_DOUBLE: u8 = 6;
pub const TAG_BYTE_ARRAY: u8 = 7;
pub const TAG_STRING: u8 = 8;
pub const TAG_LIST: u8 = 9;
pub const TAG_COMPOUND: u8 = 10;
pub const TAG_INT_ARRAY: u8 = 11;
pub const TAG_LONG_ARRAY: u8 = 12;

/// NBT 取值。字段顺序敏感的 Compound 用有序 Vec 表示。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    /// 列表。第二项为**文件声明的元素类型**：
    /// 空列表在序列化时无法从元素推断类型，必须原样保留，
    /// 否则「元素类型 Int、长度 0」的列表往返后会变成「元素类型 End」。
    List(Vec<Value>, u8),
    Compound(Vec<(String, Value)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Value {
    pub fn type_id(&self) -> u8 {
        match self {
            Value::Byte(_) => TAG_BYTE,
            Value::Short(_) => TAG_SHORT,
            Value::Int(_) => TAG_INT,
            Value::Long(_) => TAG_LONG,
            Value::Float(_) => TAG_FLOAT,
            Value::Double(_) => TAG_DOUBLE,
            Value::ByteArray(_) => TAG_BYTE_ARRAY,
            Value::String(_) => TAG_STRING,
            Value::List(..) => TAG_LIST,
            Value::Compound(_) => TAG_COMPOUND,
            Value::IntArray(_) => TAG_INT_ARRAY,
            Value::LongArray(_) => TAG_LONG_ARRAY,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Compound(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// 存在则替换（保持原位置），否则追加到末尾。
    pub fn set(&mut self, key: &str, value: Value) {
        if let Value::Compound(fields) = self {
            if let Some(slot) = fields.iter_mut().find(|(k, _)| k == key) {
                slot.1 = value;
                return;
            }
            fields.push((key.to_string(), value));
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Byte(v) => Some(*v as i64),
            Value::Short(v) => Some(*v as i64),
            Value::Int(v) => Some(*v as i64),
            Value::Long(v) => Some(*v),
            _ => None,
        }
    }

    /// 取双精度值。
    ///
    /// 实体的 `Pos` 是 Double 列表，但同一个字段在别处也可能写成长/整型
    /// （例如某些导出工具），因此这里把整数类型一并接受，避免坐标被读成 0。
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Double(v) => Some(*v),
            Value::Float(v) => Some(*v as f64),
            _ => self.as_i64().map(|v| v as f64),
        }
    }

    pub fn as_str(&self) -> Option<&str> {        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(v, _) => Some(v),
            _ => None,
        }
    }

    pub fn as_longs(&self) -> Option<&[i64]> {
        match self {
            Value::LongArray(v) => Some(v),
            _ => None,
        }
    }

    /// 读取 {x,y,z} 三元组，缺失或超出 i32 范围时返回 None。
    ///
    /// 用 try_from 而不是 `as i32`：NBT 里的 Position 可能是 TAG_Long，
    /// 直接截断会把一个超范围的值悄悄变成某个看起来正常的坐标
    /// （2^32 变成 0、2^32+5 变成 5），后续算出的方块位置与渲染结果
    /// 都是错的，且从数值上看不出异常。
    pub fn as_xyz(&self) -> Option<[i32; 3]> {
        let get = |k: &str| {
            self.get(k)
                .and_then(|v| v.as_i64())
                .and_then(|v| i32::try_from(v).ok())
        };
        Some([get("x")?, get("y")?, get("z")?])
    }
}

/// 解码 Java modified UTF-8。
///
/// Minecraft 的 NBT 字符串由 DataOutputStream.writeUTF 写出，用的是
/// modified UTF-8，有两类字节序列不是合法 UTF-8：NUL 写作 C0 80，
/// 补充平面字符（如 emoji）写作 CESU-8 代理对（每个 6 字节）。
/// 用 String::from_utf8_lossy 会把它们替换成 U+FFFD 并改变长度，
/// 再序列化就永久改写了文件内容。
fn decode_modified_utf8(bytes: &[u8]) -> AppResult<String> {
    let bad = || AppError::Corrupt("NBT 字符串不是合法的 modified UTF-8".into());
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b & 0x80 == 0 {
            out.push(b as char);
            i += 1;
            continue;
        }
        let (cp, len) = if b & 0xF0 == 0xE0 {
            if i + 2 >= bytes.len() { return Err(bad()); }
            ((((b & 0x0F) as u32) << 12)
                | (((bytes[i + 1] & 0x3F) as u32) << 6)
                | ((bytes[i + 2] & 0x3F) as u32), 3)
        } else if b & 0xE0 == 0xC0 {
            if i + 1 >= bytes.len() { return Err(bad()); }
            ((((b & 0x1F) as u32) << 6) | ((bytes[i + 1] & 0x3F) as u32), 2)
        } else {
            return Err(bad());
        };
        if (0xD800..0xDC00).contains(&cp) {
            let j = i + len;
            if j + 2 >= bytes.len() || bytes[j] & 0xF0 != 0xE0 { return Err(bad()); }
            let lo = (((bytes[j] & 0x0F) as u32) << 12)
                | (((bytes[j + 1] & 0x3F) as u32) << 6)
                | ((bytes[j + 2] & 0x3F) as u32);
            if !(0xDC00..0xE000).contains(&lo) { return Err(bad()); }
            let combined = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
            out.push(char::from_u32(combined).ok_or_else(bad)?);
            i = j + 3;
            continue;
        }
        if (0xDC00..0xE000).contains(&cp) { return Err(bad()); }
        out.push(char::from_u32(cp).ok_or_else(bad)?);
        i += len;
    }
    Ok(out)
}

/// 编码为 Java modified UTF-8，与 decode_modified_utf8 互为逆运算。
fn encode_modified_utf8(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for ch in s.chars() {
        let cp = ch as u32;
        if cp == 0 {
            out.extend_from_slice(&[0xC0, 0x80]);
        } else if cp < 0x80 {
            out.push(cp as u8);
        } else if cp < 0x800 {
            out.push(0xC0 | (cp >> 6) as u8);
            out.push(0x80 | (cp & 0x3F) as u8);
        } else if cp < 0x10000 {
            out.push(0xE0 | (cp >> 12) as u8);
            out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
            out.push(0x80 | (cp & 0x3F) as u8);
        } else {
            let v = cp - 0x10000;
            for c in [0xD800 + (v >> 10), 0xDC00 + (v & 0x3FF)] {
                out.push(0xE0 | (c >> 12) as u8);
                out.push(0x80 | ((c >> 6) & 0x3F) as u8);
                out.push(0x80 | (c & 0x3F) as u8);
            }
        }
    }
    out
}

/// NBT 字符串长度字段是 u16，因此单个字符串最多 65535 字节（Java 的
/// DataOutput.writeUTF 也是这个上限）。
///
/// 超限必须报错而不是截断：`len as u16` 会回绕成一个比实际写入量小的值，
/// 后续所有字段的解析位置随之错位，写出的文件不可读。而写入路径的终点是
/// 覆盖用户的 .litematic，因此这里宁可失败也不能产出损坏的文件。
pub const MAX_STRING_BYTES: usize = u16::MAX as usize;

/// 写入一个字符串：u16 字节长度 + modified UTF-8 字节。
///
/// 键名与字符串值都必须走这里。此前键名直接用 `as_bytes()`（原始 UTF-8），
/// 而读取端对两者统一按 modified UTF-8 解码，导致键名含补充平面字符时
/// 「写得出、读不回」——区域名里有一个 emoji 就能让整个文件无法打开。
fn write_string(out: &mut Vec<u8>, s: &str) -> AppResult<()> {
    let b = encode_modified_utf8(s);
    if b.len() > MAX_STRING_BYTES {
        return Err(AppError::Corrupt(format!(
            "字符串过长：编码后 {} 字节，上限 {MAX_STRING_BYTES} 字节",
            b.len()
        )));
    }
    out.extend_from_slice(&(b.len() as u16).to_be_bytes());
    out.extend_from_slice(&b);
    Ok(())
}

// ---------------------------------------------------------------- 读取

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> AppResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| AppError::Corrupt("长度溢出".into()))?;
        if end > self.data.len() {
            return Err(AppError::Corrupt(format!(
                "数据不足：需要 {} 字节，位置 {}，总长 {}",
                n,
                self.pos,
                self.data.len()
            )));
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> AppResult<u8> {
        Ok(self.take(1)?[0])
    }
    fn i8(&mut self) -> AppResult<i8> {
        Ok(self.u8()? as i8)
    }
    fn i16(&mut self) -> AppResult<i16> {
        let b = self.take(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }
    fn u16(&mut self) -> AppResult<u16> {
        Ok(self.i16()? as u16)
    }
    fn i32(&mut self) -> AppResult<i32> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i64(&mut self) -> AppResult<i64> {
        let b = self.take(8)?;
        Ok(i64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }
    fn f32(&mut self) -> AppResult<f32> {
        Ok(f32::from_bits(self.i32()? as u32))
    }
    fn f64(&mut self) -> AppResult<f64> {
        Ok(f64::from_bits(self.i64()? as u64))
    }

    /// NBT 字符串：u16 字节长度 + modified UTF-8 字节
    fn string(&mut self) -> AppResult<String> {
        let len = self.u16()? as usize;
        let b = self.take(len)?;
        decode_modified_utf8(b)
    }

    fn payload(&mut self, tag: u8, depth: u32) -> AppResult<Value> {
        if depth > 512 {
            return Err(AppError::Corrupt("NBT 嵌套过深".into()));
        }
        Ok(match tag {
            TAG_BYTE => Value::Byte(self.i8()?),
            TAG_SHORT => Value::Short(self.i16()?),
            TAG_INT => Value::Int(self.i32()?),
            TAG_LONG => Value::Long(self.i64()?),
            TAG_FLOAT => Value::Float(self.f32()?),
            TAG_DOUBLE => Value::Double(self.f64()?),
            TAG_BYTE_ARRAY => {
                let n = self.i32()?;
                if n < 0 {
                    return Err(AppError::Corrupt("ByteArray 长度为负".into()));
                }
                Value::ByteArray(self.take(n as usize)?.to_vec())
            }
            TAG_STRING => Value::String(self.string()?),
            TAG_LIST => {
                let elem = self.u8()?;
                let n = self.i32()?;
                if n < 0 {
                    return Err(AppError::Corrupt("List 长度为负".into()));
                }
                let mut items = Vec::with_capacity(n.min(4096) as usize);
                for _ in 0..n {
                    items.push(self.payload(elem, depth + 1)?);
                }
                Value::List(items, elem)
            }
            TAG_COMPOUND => {
                let mut fields = Vec::new();
                loop {
                    let t = self.u8()?;
                    if t == TAG_END {
                        break;
                    }
                    let name = self.string()?;
                    let v = self.payload(t, depth + 1)?;
                    fields.push((name, v));
                }
                Value::Compound(fields)
            }
            TAG_INT_ARRAY => {
                let n = self.i32()?;
                if n < 0 {
                    return Err(AppError::Corrupt("IntArray 长度为负".into()));
                }
                let mut v = Vec::with_capacity(n.min(1 << 20) as usize);
                for _ in 0..n {
                    v.push(self.i32()?);
                }
                Value::IntArray(v)
            }
            TAG_LONG_ARRAY => {
                let n = self.i32()?;
                if n < 0 {
                    return Err(AppError::Corrupt("LongArray 长度为负".into()));
                }
                let mut v = Vec::with_capacity(n.min(1 << 20) as usize);
                for _ in 0..n {
                    v.push(self.i64()?);
                }
                Value::LongArray(v)
            }
            other => {
                return Err(AppError::Corrupt(format!(
                    "未知的 NBT 标签类型 {other}"
                )))
            }
        })
    }
}

/// 解析以「具名根标签」开头的 NBT（.litematic 即此形式）。
pub fn parse(data: &[u8]) -> AppResult<Value> {
    let mut r = Reader::new(data);
    let tag = r.u8()?;
    if tag == TAG_END {
        return Err(AppError::Corrupt("根标签为 TAG_End".into()));
    }
    let _name = r.string()?;
    r.payload(tag, 0)
}

// ---------------------------------------------------------------- 写入

fn write_payload(out: &mut Vec<u8>, v: &Value) -> AppResult<()> {
    match v {
        Value::Byte(x) => out.push(*x as u8),
        Value::Short(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::Int(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::Long(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::Float(x) => out.extend_from_slice(&x.to_bits().to_be_bytes()),
        Value::Double(x) => out.extend_from_slice(&x.to_bits().to_be_bytes()),
        Value::ByteArray(b) => {
            out.extend_from_slice(&(b.len() as i32).to_be_bytes());
            out.extend_from_slice(b);
        }
        Value::String(s) => write_string(out, s)?,
        Value::List(items, elem) => {
            // 原样写回解析时读到的元素类型。
            // 不能改用 items.first() 推断：空列表没有元素，会退化成 End，
            // 使「元素类型 Int、长度 0」的列表被改写。
            out.push(*elem);
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for it in items {
                write_payload(out, it)?;
            }
        }
        Value::Compound(fields) => {
            for (k, val) in fields {
                out.push(val.type_id());
                // 键名也必须是 modified UTF-8：读取端对所有字符串统一解码，
                // 用原始 UTF-8 写 emoji 键名会写得出、读不回
                write_string(out, k)?;
                write_payload(out, val)?;
            }
            out.push(TAG_END);
        }
        Value::IntArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        Value::LongArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
    }
    Ok(())
}

/// 序列化为具名根标签形式的 NBT 字节。
pub fn serialize(root: &Value, root_name: &str) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    out.push(root.type_id());
    write_string(&mut out, root_name)?;
    write_payload(&mut out, root)?;
    Ok(out)
}

// ---------------------------------------------------------------- gzip

/// zlib（RFC1950）解压。
///
/// 与 gzip 的区别不只是文件头：PNG 的 IDAT 用的就是 zlib 容器，
/// 而 gzip 容器多一层头与 CRC。两者混用会得到解析失败。
pub fn inflate_zlib(data: &[u8]) -> AppResult<Vec<u8>> {
    let mut d = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::new();
    d.read_to_end(&mut out)?;
    Ok(out)
}

pub fn gunzip(data: &[u8]) -> AppResult<Vec<u8>> {
    let mut d = flate2::read::GzDecoder::new(data);
    let mut out = Vec::new();
    d.read_to_end(&mut out)?;
    Ok(out)
}

pub fn gzip(data: &[u8], level: u32) -> AppResult<Vec<u8>> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(level));
    e.write_all(data)?;
    Ok(e.finish()?)
}

/// 自动识别输入是否已 gzip 压缩，然后解析。
pub fn parse_maybe_gzip(file: &[u8]) -> AppResult<Value> {
    let raw = if file.len() >= 2 && file[0] == 0x1f && file[1] == 0x8b {
        gunzip(file)?
    } else {
        file.to_vec()
    };
    parse(&raw)
}

/// 序列化并 gzip 压缩。
pub fn serialize_gzip(root: &Value, root_name: &str, level: u32) -> AppResult<Vec<u8>> {
    gzip(&serialize(root, root_name)?, level)
}

#[cfg(test)]
#[path = "../tests/format/nbt.rs"]
mod tests;
