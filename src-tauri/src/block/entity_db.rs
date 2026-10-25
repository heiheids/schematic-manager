//! 实体贴图库：各版本 `assets/minecraft/textures/entity/**` 的离线副本。
//!
//! 与方块库（block::db）分开存放，因为两者的用法不同：方块贴图要被拼进图集、
//! 由模型按 uv 采样；实体贴图是按名字整张取用（贴到实体几何上）。
//!
//! 数据由 pack/entity_write.rs 从客户端 jar 构建（启动补版本界面触发）。
//! 容器格式由读写两端共同遵守；这里是读取端，所有读取都做边界检查，
//! 损坏数据返回错误而不是越界或 panic。

use crate::error::{AppError, AppResult};
use std::collections::HashMap;
use std::path::Path;

/// 魔数。与方块库的 `SMDB` 区分开，避免把两个包弄混后读出一堆无意义的数字。
const MAGIC: &[u8] = b"SMET";
const FORMAT_VERSION: u32 = 1;

/// 上限。这些数字只用于挡住损坏文件导致的超大分配，
/// 取的是「正常数据量级的若干倍」，不是业务约束。
const MAX_VERSIONS: u32 = 10_000;
const MAX_TEXTURES: u32 = 200_000;
const MAX_STRINGS: u32 = 200_000;
const MAX_ENTRIES: u32 = 200_000;
const MAX_NAME: usize = 4096;

/// 一张实体贴图在像素池中的位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityTexture {
    pub w: u16,
    pub h: u16,
    pub offset: u32,
    pub len: u32,
    pub has_alpha: bool,
}

/// 某个版本可用的实体贴图：路径 -> 贴图池下标
#[derive(Debug, Clone, Default)]
pub struct EntityVersion {
    pub data_version: i32,
    pub name: String,
    pub by_path: HashMap<String, u32>,
}

/// 实体贴图库
///
/// Debug 是派生实现，会打印整个像素池，因此不要在日志里直接打印本类型。
#[derive(Debug)]
pub struct EntityDb {
    pub paths: Vec<String>,
    pub textures: Vec<EntityTexture>,
    pub versions: Vec<EntityVersion>,
    /// zlib 解压后的 RGBA8888 像素池
    pub pixels: Vec<u8>,
}

impl EntityDb {
    /// 从已解压的字节解析
    pub fn load_from_bytes(data: &[u8]) -> AppResult<Self> {
        let mut r = Reader::new(data);
        if r.take(MAGIC.len())? != MAGIC {
            return Err(AppError::Parse("实体贴图库魔数不匹配".into()));
        }
        let fv = r.u32()?;
        if fv != FORMAT_VERSION {
            return Err(AppError::UnsupportedVersion(format!(
                "实体贴图库格式版本 {fv}，本程序支持 {FORMAT_VERSION}"
            )));
        }

        let str_count = r.count(MAX_STRINGS, "字符串")?;
        let mut paths = Vec::with_capacity(str_count as usize);
        for _ in 0..str_count {
            paths.push(r.short_str()?);
        }

        let tex_count = r.count(MAX_TEXTURES, "贴图")?;
        let mut textures = Vec::with_capacity(tex_count as usize);
        for _ in 0..tex_count {
            textures.push(EntityTexture {
                w: r.u16()?,
                h: r.u16()?,
                offset: r.u32()?,
                len: r.u32()?,
                has_alpha: r.u8()? != 0,
            });
        }

        let raw_size = r.u32()? as usize;
        let comp_size = r.u32()? as usize;
        let comp = r.take(comp_size)?;
        let pixels = decompress(comp, raw_size)?;

        // 贴图区间的合法性在解析阶段一次校验完。
        //
        // 放到取用时再判会让「下标越界」变成运行时的 0 像素，界面上表现为
        // 一张全黑贴图，看不出是数据坏了；这里直接拒绝整个文件。
        for (i, t) in textures.iter().enumerate() {
            let end = (t.offset as usize)
                .checked_add(t.len as usize)
                .ok_or_else(|| AppError::Corrupt(format!("贴图 {i} 的区间溢出")))?;
            if end > pixels.len() {
                return Err(AppError::Corrupt(format!(
                    "贴图 {i} 区间 {}..{} 超出像素池长度 {}",
                    t.offset,
                    end,
                    pixels.len()
                )));
            }
            let expect = t.w as usize * t.h as usize * 4;
            if t.len as usize != expect {
                return Err(AppError::Corrupt(format!(
                    "贴图 {i} 长度 {} 与尺寸 {}x{} 不符（应为 {expect}）",
                    t.len, t.w, t.h
                )));
            }
        }

        let ver_count = r.count(MAX_VERSIONS, "版本")?;
        let mut versions = Vec::with_capacity(ver_count as usize);
        for _ in 0..ver_count {
            let data_version = r.u32()? as i32;
            let name_len = r.u16()? as usize;
            if name_len > MAX_NAME {
                return Err(AppError::Corrupt(format!("版本名过长：{name_len}")));
            }
            let name = String::from_utf8_lossy(r.take(name_len)?).into_owned();
            let n = r.count(MAX_ENTRIES, "贴图条目")?;
            let mut by_path = HashMap::with_capacity(n.min(8192) as usize);
            for _ in 0..n {
                let path_idx = r.u32()? as usize;
                let pool_idx = r.u32()?;
                let path = paths.get(path_idx).ok_or_else(|| {
                    AppError::Corrupt(format!("贴图路径下标越界：{path_idx}"))
                })?;
                if pool_idx as usize >= textures.len() {
                    return Err(AppError::Corrupt(format!(
                        "贴图池下标越界：{pool_idx}（共 {} 张）",
                        textures.len()
                    )));
                }
                by_path.insert(path.clone(), pool_idx);
            }
            versions.push(EntityVersion { data_version, name, by_path });
        }

        Ok(Self { paths, textures, versions, pixels })
    }

    /// 从文件解析（自动识别 zlib / gzip / 未压缩）
    pub fn load(path: &Path) -> AppResult<Self> {
        let raw = std::fs::read(path).map_err(|e| {
            AppError::Parse(format!("读取实体贴图库 {} 失败：{e}", path.display()))
        })?;
        // 文件头 4 字节是魔数时说明未压缩，否则按容器解压
        let data = if raw.len() >= 4 && &raw[..4] == MAGIC {
            raw
        } else {
            decompress(&raw, 0)?
        };
        Self::load_from_bytes(&data)
    }

    /// 按 data_version 找版本；同一个 data_version 有多个版本名时取第一个。
    ///
    /// 用 data_version 而不是版本名：文件里只记 dataVersion，而版本名会随
    /// 快照/补丁变化（同一个 dataVersion 可能对应多个名字）。
    pub fn version_for(&self, data_version: i32) -> Option<&EntityVersion> {
        self.versions.iter().find(|v| v.data_version == data_version)
    }

    /// 按版本与路径取像素。路径形如 `entity/chest/normal`。
    pub fn pixels_of(&self, ver: &EntityVersion, path: &str) -> Option<(&EntityTexture, &[u8])> {
        let idx = *ver.by_path.get(path)? as usize;
        let t = self.textures.get(idx)?;
        let s = t.offset as usize;
        let e = s.checked_add(t.len as usize)?;
        Some((t, self.pixels.get(s..e)?))
    }
}

/// 与方块库同样的解压策略：zlib 优先，其次 gzip，都不行则原样返回。
///
/// `expect` 为 0 表示长度未知，此时不做长度校验；非 0 时长度必须一致。
fn decompress(bytes: &[u8], expect: usize) -> AppResult<Vec<u8>> {
    use std::io::Read;
    let try_read = |mut r: Box<dyn Read>| -> Option<Vec<u8>> {
        let mut out = Vec::new();
        r.read_to_end(&mut out).ok().map(|_| out)
    };
    let mut got = try_read(Box::new(flate2::read::ZlibDecoder::new(bytes)));
    if got.is_none() {
        got = try_read(Box::new(flate2::read::GzDecoder::new(bytes)));
    }
    let out = match got {
        Some(v) => v,
        None => bytes.to_vec(),
    };
    if expect != 0 && out.len() != expect {
        return Err(AppError::Corrupt(format!(
            "解压后长度 {} 与头部声明 {} 不一致",
            out.len(),
            expect
        )));
    }
    Ok(out)
}

/// 边界检查读取器
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
            .ok_or_else(|| AppError::Corrupt("读取长度溢出".into()))?;
        if end > self.data.len() {
            return Err(AppError::Corrupt(format!(
                "数据不足：位置 {} 需要 {} 字节，总长 {}",
                self.pos,
                n,
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

    fn u16(&mut self) -> AppResult<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> AppResult<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// u16 长度前缀的 UTF-8 字符串
    fn short_str(&mut self) -> AppResult<String> {
        let n = self.u16()? as usize;
        if n > MAX_NAME {
            return Err(AppError::Corrupt(format!("字符串过长：{n}")));
        }
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    fn count(&mut self, max: u32, what: &str) -> AppResult<u32> {
        let n = self.u32()?;
        if n > max {
            return Err(AppError::Corrupt(format!("{what}数量异常：{n}")));
        }
        Ok(n)
    }
}

#[cfg(test)]
#[path = "../tests/block/entity_db.rs"]
mod tests;
