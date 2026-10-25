//! zip 读取（只读）。
//!
//! 用途：Minecraft 客户端与模组 jar 都是 zip，导入资源包时需要列出条目、
//! 按名提取并解压。本机无法获取 zip 类依赖，故自行实现。
//!
//! 只实现读取所需的部分：从中央目录索引条目，按需解压单个条目。
//! 不做写入、加密、zip64（jar 不会用到；遇到时明确报错而非给出错数据）。
//!
//! 分层：中央目录解析是纯字节逻辑，`parse_central_directory` 可脱离
//! 解压单独测试；解压只出现在 `read` 内。

use crate::error::{AppError, AppResult};
use std::io::Read;

/// 中央目录结尾的签名
const EOCD_SIG: u32 = 0x0605_4b50;
const EOCD_MIN_LEN: usize = 22;
/// 中央目录条目的签名
const CEN_SIG: u32 = 0x0201_4b50;
/// 本地文件头的签名
const LOC_SIG: u32 = 0x0403_4b50;

/// 压缩方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// 不压缩
    Stored,
    /// raw deflate（zip 条目的常见形式）
    Deflate,
}

/// 一个条目在 zip 中的位置与属性
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 条目名（路径分隔符为 `/`）
    pub name: String,
    pub method: Method,
    /// 压缩后大小
    pub compressed_size: u64,
    /// 解压后大小
    pub uncompressed_size: u64,
    /// 本地文件头在文件中的偏移
    pub local_offset: u64,
    pub crc32: u32,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }
}

/// 整个 zip 的索引
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Archive {
    pub entries: Vec<Entry>,
}

impl Archive {
    /// 按名精确查找
    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

fn u16le(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn u32le(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// 找中央目录结尾记录（EOCD）。
///
/// 从尾部向前搜索：EOCD 之后可能还有注释，长度不定，因此不能只看最后 22 字节。
///
/// 注释的字节是任意的（zip 规范允许任何内容），因此注释里完全可能出现
/// `PK\x05\x06`。只认「从尾部遇到的第一个签名」会采信这种假记录，把总条数、
/// 目录大小、目录偏移全读错，结果是**打开成功却一条条目都找不到**。
/// 真正的 EOCD 一定满足「起点 + 22 + 注释长度 == 文件长度」（legacy-cpp 的
/// FindEOCD 也是这么挑的，见 legacy-cpp/src/zip.cpp:202-207），
/// 因此优先取自洽的那条，找不到时再退回第一个候选，兼容注释长度字段不可信
/// 的文件（例如某些生成器把注释补在长度字段之外）。
fn find_eocd(data: &[u8]) -> AppResult<usize> {
    if data.len() < EOCD_MIN_LEN {
        return Err(AppError::Corrupt("不是 zip：文件过短".into()));
    }
    // 注释最长 65535 字节，因此 EOCD 起点不早于 len - 22 - 65535
    let lower = data.len().saturating_sub(EOCD_MIN_LEN + 65535);
    let mut loose: Option<usize> = None;
    let mut i = data.len() - EOCD_MIN_LEN;
    loop {
        if u32le(data, i) == EOCD_SIG {
            let clen = u16le(data, i + 20) as usize;
            // 尾部严格吻合：最可信，直接采信
            if i + EOCD_MIN_LEN + clen == data.len() {
                return Ok(i);
            }
            // 不相符时留作候选，继续往前找更吻合的
            if loose.is_none() {
                loose = Some(i);
            }
        }
        if i == lower {
            return loose.ok_or_else(|| {
                AppError::Corrupt("不是 zip：找不到中央目录结尾记录".into())
            });
        }
        i -= 1;
    }
}

/// 解析中央目录，得到条目索引。
///
/// 只读中央目录而不读本地文件头：本地头中的大小字段在「使用数据描述符」
/// 时可能为 0，而中央目录里的值总是可靠的。
pub fn parse_central_directory(data: &[u8]) -> AppResult<Archive> {
    let eocd = find_eocd(data)?;
    let total = u16le(data, eocd + 10) as usize;
    let cd_size = u32le(data, eocd + 12) as usize;
    let cd_off = u32le(data, eocd + 16) as usize;

    if cd_off == 0xFFFF_FFFF || cd_size == 0xFFFF_FFFF || total == 0xFFFF {
        return Err(AppError::UnsupportedVersion(
            "暂不支持 zip64 格式".into(),
        ));
    }
    if cd_off + cd_size > data.len() {
        return Err(AppError::Corrupt("zip 中央目录超出文件范围".into()));
    }

    let mut entries = Vec::with_capacity(total.min(65536));
    let mut p = cd_off;
    let end = cd_off + cd_size;
    for _ in 0..total {
        if p + 46 > end {
            return Err(AppError::Corrupt("zip 中央目录条目不完整".into()));
        }
        if u32le(data, p) != CEN_SIG {
            return Err(AppError::Corrupt(format!(
                "zip 中央目录条目签名不符（偏移 {p}）"
            )));
        }
        let method = match u16le(data, p + 10) {
            0 => Method::Stored,
            8 => Method::Deflate,
            other => {
                return Err(AppError::UnsupportedVersion(format!(
                    "暂不支持 zip 压缩方式 {other}"
                )))
            }
        };
        let crc32 = u32le(data, p + 16);
        let compressed_size = u32le(data, p + 20) as u64;
        let uncompressed_size = u32le(data, p + 24) as u64;
        let name_len = u16le(data, p + 28) as usize;
        let extra_len = u16le(data, p + 30) as usize;
        let comment_len = u16le(data, p + 32) as usize;
        let local_offset = u32le(data, p + 42) as u64;

        if p + 46 + name_len > end {
            return Err(AppError::Corrupt("zip 条目名超出中央目录范围".into()));
        }
        let raw_name = &data[p + 46..p + 46 + name_len];
        // 文件名按 UTF-8 解码；jar 内的条目名都是 ASCII，坏字节按替换字符处理
        let name = String::from_utf8_lossy(raw_name).into_owned();

        entries.push(Entry {
            name,
            method,
            compressed_size,
            uncompressed_size,
            local_offset,
            crc32,
        });

        p += 46 + name_len + extra_len + comment_len;
    }

    Ok(Archive { entries })
}

/// 从字节解析 zip 索引
pub fn open(data: &[u8]) -> AppResult<Archive> {
    parse_central_directory(data)
}

/// CRC-32（IEEE 802.3，与 zip 使用的一致）
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for b in data {
        crc = table[((crc ^ *b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// 解压 raw deflate 数据，最多读取 `declared` + 1 字节。
///
/// 为什么需要上限：`declared` 来自文件（中央目录里的 uncompressed_size），
/// 是不可信输入，解压展开比同样由文件决定。一个声明 1 KB、实际能展开
/// 数 GB 的条目若直接 `read_to_end`，会先把内存吃光，之后再报「长度不符」
/// 已经晚了——内存已经分配掉了。
///
/// 多读 1 字节（而不是恰好 `declared`）是为了区分两种情况：
///   - 读满 `declared + 1` 字节 => 实际比声明长，调用方比对长度时会判为损坏
///   - 读到的少于 `declared + 1` => 实际就是那么多，交给调用方比对
/// 若恰好读 `declared` 就停，就无法区分「正好相等」与「还有更多」。
fn inflate_limited(raw: &[u8], declared: usize) -> AppResult<Vec<u8>> {
    let limit = declared.saturating_add(1) as u64;
    let mut d = flate2::read::DeflateDecoder::new(raw).take(limit);
    let mut out = Vec::with_capacity(declared.min(64 * 1024 * 1024));
    // 解压本身出错必须向上传播：静默返回半截数据会让人误以为是长度不符
    d.read_to_end(&mut out)?;
    Ok(out)
}

/// 取出某个条目的原始内容。
///
/// 先读本地文件头以取得数据区的真实起点：本地头中的名称与扩展字段长度
/// 可能与中央目录不同，用中央目录的长度去推算数据位置会错位。
pub fn read(data: &[u8], e: &Entry) -> AppResult<Vec<u8>> {
    let off = e.local_offset as usize;
    if off + 30 > data.len() {
        return Err(AppError::Corrupt(format!("条目 {} 的本地头超出范围", e.name)));
    }
    if u32le(data, off) != LOC_SIG {
        return Err(AppError::Corrupt(format!("条目 {} 的本地头签名不符", e.name)));
    }
    let name_len = u16le(data, off + 26) as usize;
    let extra_len = u16le(data, off + 28) as usize;
    let ds = off + 30 + name_len + extra_len;
    let de = ds
        .checked_add(e.compressed_size as usize)
        .ok_or_else(|| AppError::Corrupt("条目大小溢出".into()))?;
    if de > data.len() {
        return Err(AppError::Corrupt(format!("条目 {} 的数据超出范围", e.name)));
    }
    let raw = &data[ds..de];

    let out = match e.method {
        Method::Stored => {
            if raw.len() as u64 != e.uncompressed_size {
                return Err(AppError::Corrupt(format!(
                    "条目 {} 的存储长度 {} 与记录的 {} 不符",
                    e.name,
                    raw.len(),
                    e.uncompressed_size
                )));
            }
            raw.to_vec()
        }
        Method::Deflate => inflate_limited(raw, e.uncompressed_size as usize)?,
    };

    if out.len() as u64 != e.uncompressed_size {
        return Err(AppError::Corrupt(format!(
            "条目 {} 解压后长度 {} 与记录的 {} 不符",
            e.name,
            out.len(),
            e.uncompressed_size
        )));
    }
    // 校验 CRC：中央目录里的值是可靠的，能据此发现数据损坏
    let got = crc32(&out);
    if got != e.crc32 {
        return Err(AppError::Corrupt(format!(
            "条目 {} 校验和不符（记录 {:08X}，实际 {:08X}）",
            e.name, e.crc32, got
        )));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "../tests/format/zip.rs"]
mod tests;
