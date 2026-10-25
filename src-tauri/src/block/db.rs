//! 方块库读取。
//!
//! 格式见 docs/blocks-dat-format.md，由 legacy-cpp 的写出实现
//! （packbuild.cpp）与读取实现（blockdb.cpp）对照确认。
//!
//! 每个 pack 文件只含一个版本；程序启动时加载多个 pack
//! （基础版 + 模组叠加包）后合并使用。

use crate::error::{AppError, AppResult};
use std::collections::HashMap;

const MAGIC: &[u8; 7] = b"PMBLKDB";

/// 本程序支持的方块库格式版本。
///
/// 载入时用它校验文件头；对外公开是为了让诊断工具能报告「读到的格式版本」，
/// 而无需在 `BlockDb` 里再存一份与常量重复的副本。
pub const FORMAT_VERSION: u32 = 3;

/// 各段的数量上限，用于拒绝异常或损坏的文件。
/// 取值与 legacy-cpp 的读取实现一致。
const MAX_VERSIONS: u32 = 100_000;
const MAX_TEXTURES: u32 = 2_000_000;
const MAX_MODELS: u32 = 5_000_000;
const MAX_LANGS: u32 = 100_000;
const MAX_STRINGS: u32 = 20_000_000;
/// 单张语言表的条目数上限。
///
/// 不能沿用 MAX_STRINGS：语言条目是「三条字符串下标」共 12 字节的结构，
/// 按 20_000_000 放行时，一个仅 240 MB 的构造文件就能让
/// `Vec::with_capacity(n)` 直接申请约 1.4 GB 而 OOM 中止。
/// legacy-cpp 对这一档的上限也是 5_000_000（blockdb.cpp:154）。
const MAX_LANG_ENTRIES: u32 = 5_000_000;
/// 单版本方块数上限。legacy-cpp 用的是 500000（blockdb.cpp:247），
/// 与字符串表的 2000 万不是一个量级——误用后者会让损坏文件
/// 通过校验并触发超大分配。
const MAX_BLOCKS: u32 = 500_000;

/// 贴图缺失的哨兵值
pub const NO_TEXTURE: u32 = 0xFFFF_FFFF;

/// 六个面。faceMask 的位 0~5 依次对应下表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

/// 模型中的一个面
#[derive(Debug, Clone, PartialEq)]
pub struct ModelFace {
    /// 贴图**字符串表下标**，不是贴图池下标。
    ///
    /// 两者数值空间完全无关：实测随包的方块库中字符串表 6263 项、贴图池仅
    /// 1685 项，48% 的面的取值超出贴图池长度。若误把字符串下标当池下标用，
    /// 多数面会取不到贴图（渲染空白），少数会取到完全无关的贴图。
    ///
    /// 这里不能直接存池下标：模型跨版本共享，而路径到池下标的映射按版本
    /// 不同，只能在按版本查用时解析（见 BlockDb::texture_pool_index）。
    pub texture: u32,
    pub uv: [f32; 4],
    /// 0xFF 表示无剔除面
    pub cullface: u8,
    pub tintindex: i16,
    /// 已归一化到 0~359
    pub rotation: u16,
}

/// 模型元素（一个长方体）
#[derive(Debug, Clone, PartialEq)]
pub struct ModelElement {
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub has_rotation: bool,
    pub rot_kind: u8,
    pub origin: [f32; 3],
    pub axis: u8,
    pub angle: f32,
    pub rescale: bool,
    pub rot_vec: [f32; 3],
    pub face_mask: u8,
    /// 顺序对应 face_mask 中自低位到高位置位的面
    pub faces: Vec<ModelFace>,
}

impl ModelElement {
    /// 取指定面的数据（若该面存在）
    pub fn face(&self, f: Face) -> Option<&ModelFace> {
        if self.face_mask & (1u8 << (f as u8)) == 0 {
            return None;
        }
        // faces 按位置位的顺序排列，需要数出该位之前有几个置位
        let idx = (0..(f as u8)).filter(|b| self.face_mask & (1u8 << b) != 0).count();
        self.faces.get(idx)
    }
}

/// 模型
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub translucent: bool,
    pub elements: Vec<ModelElement>,
}

/// 贴图在像素池中的位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureInfo {
    pub w: u16,
    pub h: u16,
    pub offset: u32,
    pub len: u32,
    pub has_alpha: bool,
}

/// 语言表条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LangEntry {
    pub block_name: String,
    pub en: String,
    pub zh: String,
}

/// 方块的一个变体（或 multipart 的一个分支）
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    /// 条件字符串在方块属性上下文中的表达
    pub when: String,
    pub model_idx: u32,
    /// 绕 X 轴旋转的 90 度倍数
    pub rot_x_div90: u8,
    /// 绕 Y 轴旋转的 90 度倍数
    pub rot_y_div90: u8,
    pub uvlock: bool,
}

/// 方块定义
#[derive(Debug, Clone, PartialEq)]
pub struct BlockDef {
    pub name: String,
    pub variants: Vec<Variant>,
    pub multipart: Vec<Variant>,
}

/// 一个版本
#[derive(Debug, Clone)]
pub struct VersionDef {
    pub name: String,
    pub data_version: i32,
    pub lang_idx: u32,
    /// 贴图路径 -> 贴图池下标
    pub texture_by_path: HashMap<String, u32>,
    pub blocks: Vec<BlockDef>,
    pub langs: Vec<LangEntry>,
    pub is_overlay: bool,
}

/// 解析器。所有读取都做边界检查，损坏数据返回错误而不是越界。
struct BinReader<'a> {
    data: &'a [u8],
    pos: usize,
    le: bool,
}

impl<'a> BinReader<'a> {
    fn new(data: &'a [u8], little_endian: bool) -> Self {
        Self { data, pos: 0, le: little_endian }
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
        let a = [b[0], b[1]];
        Ok(if self.le { u16::from_le_bytes(a) } else { u16::from_be_bytes(a) })
    }
    fn u32(&mut self) -> AppResult<u32> {
        let b = self.take(4)?;
        let a = [b[0], b[1], b[2], b[3]];
        Ok(if self.le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) })
    }
    fn i16(&mut self) -> AppResult<i16> {
        Ok(self.u16()? as i16)
    }
    fn i32(&mut self) -> AppResult<i32> {
        Ok(self.u32()? as i32)
    }
    fn f32(&mut self) -> AppResult<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// NUL 结尾的字符串。
    ///
    /// 格式本身不存长度，因此必须限制扫描范围：损坏文件里出现
    /// 缺失终止符时，无界扫描会一直读到文件尾甚至越界。
    fn cstr(&mut self) -> AppResult<String> {
        const MAX_STR: usize = 64 * 1024;
        let start = self.pos;
        let mut end = start;
        while end < self.data.len() && self.data[end] != 0 {
            end += 1;
            if end - start > MAX_STR {
                return Err(AppError::Corrupt("字符串长度异常，可能文件已损坏".into()));
            }
        }
        if end >= self.data.len() {
            return Err(AppError::Corrupt("字符串缺少终止符，文件已截断".into()));
        }
        let s = String::from_utf8_lossy(&self.data[start..end]).into_owned();
        self.pos = end + 1;
        Ok(s)
    }

    /// 读一个 f32 三元组
    fn f32x3(&mut self) -> AppResult<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    /// 读一个 u32 并做上限校验
    fn count(&mut self, max: u32, what: &str) -> AppResult<u32> {
        let n = self.u32()?;
        if n > max {
            return Err(AppError::Corrupt(format!("{what}数量异常：{n}")));
        }
        Ok(n)
    }
}

/// 整个方块库
///
/// 注意：Debug 是派生实现的，格式化输出会包含整个像素池，
/// 因此不要在日志里直接打印本类型。
#[derive(Debug)]
pub struct BlockDb {
    pub strings: Vec<String>,
    pub langs: Vec<Vec<LangEntry>>,
    pub textures: Vec<TextureInfo>,
    pub models: Vec<Model>,
    pub versions: Vec<VersionDef>,
    /// zlib 解压后的 RGBA8888 像素池
    pub pixels: Vec<u8>,
}

impl BlockDb {
    /// 不含任何版本的空库。
    ///
    /// 基础库按需构建：还没补任何版本时程序照常运行，只有具体到某个
    /// 数据版本的查询才会报「不在方块库中」。
    pub fn empty() -> Self {
        BlockDb {
            strings: Vec::new(),
            langs: Vec::new(),
            textures: Vec::new(),
            models: Vec::new(),
            versions: Vec::new(),
            pixels: Vec::new(),
        }
    }

    /// 从已解压的字节解析。
    ///
    /// `is_overlay` 必须由调用方给出，不能从数据推测：叠加包与基础版本的
    /// 差别在于**资源包类型**，而资源包类型是导入时由用户选择的
    /// （legacy-cpp 的 PackInfo::kind：0=内置基础库 1=MC 版本包 2=模组包，
    /// 见 blockdb_packs.cpp:41），并不记录在 pack 文件里。
    ///
    /// 尤其不能以 dataVersion 为负来判定：打包脚本会把**缺少**
    /// data_version 的普通版本也记为 -1
    /// （legacy-cpp/tools/pack_blockdb.py:446），那样会把基础版误判成
    /// 叠加包，进而把它排除在版本选择之外。
    pub fn load_from_bytes(data: &[u8], is_overlay: bool) -> AppResult<Self> {
        let mut r = BinReader::new(data, true);

        let magic = r.take(8)?;
        if &magic[..7] != MAGIC {
            return Err(AppError::Parse("方块数据库魔数不匹配".into()));
        }
        let format_version = r.u32()?;
        if format_version != FORMAT_VERSION {
            return Err(AppError::UnsupportedVersion(format!(
                "方块库格式版本 {format_version}，本程序支持 {FORMAT_VERSION}"
            )));
        }
        let version_count = r.count(MAX_VERSIONS, "版本")?;
        let texture_count = r.count(MAX_TEXTURES, "贴图")?;
        let model_count = r.count(MAX_MODELS, "模型")?;
        let lang_count = r.count(MAX_LANGS, "语言表")?;

        // ---- 字符串表 ----
        let str_count = r.count(MAX_STRINGS, "字符串")?;
        let mut strings = Vec::with_capacity(str_count as usize);
        for _ in 0..str_count {
            strings.push(r.cstr()?);
        }
        // 段内重复计数，必须与文件头一致
        let sec = r.u32()?;
        if sec != lang_count {
            return Err(AppError::Corrupt(format!(
                "语言表段内计数 {sec} 与文件头 {lang_count} 不一致"
            )));
        }

        // ---- 语言表 ----
        let mut langs = Vec::with_capacity(lang_count as usize);
        for _ in 0..lang_count {
            let n = r.count(MAX_LANG_ENTRIES, "语言条目")?;
            let mut group = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let bi = r.u32()?;
                let ei = r.u32()?;
                let zi = r.u32()?;
                group.push(LangEntry {
                    block_name: str_at(&strings, bi),
                    en: str_at(&strings, ei),
                    zh: str_at(&strings, zi),
                });
            }
            langs.push(group);
        }

        // ---- 贴图表 ----
        let sec = r.u32()?;
        if sec != texture_count {
            return Err(AppError::Corrupt(format!(
                "贴图段内计数 {sec} 与文件头 {texture_count} 不一致"
            )));
        }
        let mut textures = Vec::with_capacity(texture_count as usize);
        for _ in 0..texture_count {
            textures.push(TextureInfo {
                w: r.u16()?,
                h: r.u16()?,
                offset: r.u32()?,
                len: r.u32()?,
                has_alpha: r.u8()? != 0,
            });
        }
        let raw_size = r.u32()?;
        let comp_size = r.u32()?;
        let comp = r.take(comp_size as usize)?;
        let pixels = decompress(comp, raw_size as usize)?;

        // ---- 模型表 ----
        let sec = r.u32()?;
        if sec != model_count {
            return Err(AppError::Corrupt(format!(
                "模型段内计数 {sec} 与文件头 {model_count} 不一致"
            )));
        }
        let mut models = Vec::with_capacity(model_count.min(65536) as usize);
        for _ in 0..model_count {
            let translucent = r.u8()? != 0;
            let elem_count = r.u16()? as usize;
            let mut elements = Vec::with_capacity(elem_count.min(256));
            for _ in 0..elem_count {
                let from = r.f32x3()?;
                let to = r.f32x3()?;
                let has_rotation = r.u8()? != 0;
                let mut rot_kind = 0;
                let mut origin = [0.0f32; 3];
                let mut axis = 0;
                let mut angle = 0.0f32;
                let mut rescale = false;
                let mut rot_vec = [0.0f32; 3];
                if has_rotation {
                    rot_kind = r.u8()?;
                    origin = r.f32x3()?;
                    axis = r.u8()?;
                    angle = r.f32()?;
                    rescale = r.u8()? != 0;
                    rot_vec = r.f32x3()?;
                }
                let face_mask = r.u8()?;
                let mut faces = Vec::new();
                for bit in 0..6u8 {
                    if face_mask & (1u8 << bit) == 0 {
                        continue;
                    }
                    let texture = r.u32()?;
                    let uv = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                    let cullface = r.u8()?;
                    let tintindex = r.i16()?;
                    let rotation = r.u16()?;
                    faces.push(ModelFace { texture, uv, cullface, tintindex, rotation });
                }
                elements.push(ModelElement {
                    from,
                    to,
                    has_rotation,
                    rot_kind,
                    origin,
                    axis,
                    angle,
                    rescale,
                    rot_vec,
                    face_mask,
                    faces,
                });
            }
            models.push(Model { translucent, elements });
        }

        // ---- 版本段 ----
        let sec = r.u32()?;
        if sec > MAX_VERSIONS {
            return Err(AppError::Corrupt(format!("版本段内计数异常：{sec}")));
        }
        if sec != version_count {
            return Err(AppError::Corrupt(format!(
                "版本段内计数 {sec} 与文件头 {version_count} 不一致"
            )));
        }
        let mut versions = Vec::with_capacity(sec as usize);
        for _ in 0..sec {
            let name_idx = r.u32()?;
            let data_version = r.i32()?;
            let lang_idx = r.u32()?;
            let tex_path_count = r.count(MAX_TEXTURES, "版本贴图路径")?;
            let mut texture_by_path = HashMap::with_capacity(tex_path_count.min(65536) as usize);
            for _ in 0..tex_path_count {
                let path_idx = r.u32()?;
                let pool_idx = r.u32()?;
                texture_by_path.insert(str_at(&strings, path_idx), pool_idx);
            }
            let block_count = r.count(MAX_BLOCKS, "方块")?;
            let mut blocks = Vec::with_capacity(block_count.min(65536) as usize);
            for _ in 0..block_count {
                let block_name = str_at(&strings, r.u32()?);
                let variant_count = r.u16()? as usize;
                let mut variants = Vec::with_capacity(variant_count.min(256));
                for _ in 0..variant_count {
                    variants.push(read_variant(&mut r, &strings)?);
                }
                let mp_count = r.u16()? as usize;
                let mut multipart = Vec::with_capacity(mp_count.min(256));
                for _ in 0..mp_count {
                    multipart.push(read_variant(&mut r, &strings)?);
                }
                blocks.push(BlockDef {
                    name: block_name,
                    variants,
                    multipart,
                });
            }
            // 语言表下标必须能在本包的语言表池里取到。
            // 用 `unwrap_or_default()` 静默给一张空表的话，该版本的所有方块
            // 都会显示不出中文名，而用户看不出是数据坏了还是本来就没有名字。
            // lang_idx 是文件里的字段，属不可信输入，因此这里显式报错。
            let langs_for_version = langs.get(lang_idx as usize).cloned().ok_or_else(|| {
                AppError::Corrupt(format!(
                    "版本的语言表下标 {lang_idx} 超出了包内语言表数量 {}",
                    langs.len()
                ))
            })?;
            versions.push(VersionDef {
                name: str_at(&strings, name_idx),
                data_version,
                lang_idx,
                texture_by_path,
                blocks,
                langs: langs_for_version,
                // 由载入方给出，不推测。dataVersion 为负只表示打包时
                // 缺少版本号，与是否为叠加包无关。
                is_overlay,
            });
        }

        // 贴图偏移必须落在像素池范围内，否则渲染时会越界读取
        for (i, t) in textures.iter().enumerate() {
            let end = t.offset as usize + t.len as usize;
            if end > pixels.len() {
                return Err(AppError::Corrupt(format!(
                    "贴图 {i} 的范围 {}+{} 超出像素池长度 {}",
                    t.offset,
                    t.len,
                    pixels.len()
                )));
            }
        }

        Ok(Self { strings, langs, textures, models, versions, pixels })
    }

    /// 从 pack 文件载入。
    ///
    /// 库文件（.dat.z）用 **zlib** 容器压缩（与 legacy-cpp 的
    /// `uncompress` 一致），而 `.litematic` 用的是 gzip。两者容器不同，
    /// 只按一种解压会读不出库文件。
    /// 因此这里按魔数分派：gzip 用 gunzip、zlib 用 inflate_zlib，
    /// 都不是时按未压缩处理。
    ///
    /// 参数含义见 load_from_bytes。
    pub fn load(path: &str, is_overlay: bool) -> AppResult<Self> {
        let bytes = std::fs::read(path)?;
        let raw = Self::decompress(&bytes)?;
        Self::load_from_bytes(&raw, is_overlay)
    }

    /// 按容器魔数解压：gzip（1f 8b）或 zlib（78 xx），都不是则原样返回。
    fn decompress(bytes: &[u8]) -> AppResult<Vec<u8>> {
        if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
            return crate::format::nbt::gunzip(bytes);
        }
        // zlib 头：CMF 低 4 位为 8（deflate），且 (CMF*256+FLG) 能被 31 整除
        if bytes.len() >= 2 && bytes[0] & 0x0f == 8 {
            let hdr = bytes[0] as u16 * 256 + bytes[1] as u16;
            if hdr % 31 == 0 {
                return crate::format::nbt::inflate_zlib(bytes);
            }
        }
        Ok(bytes.to_vec())
    }

    /// 把模型的贴图字符串表下标解析成指定版本的贴图池下标。
    ///
    /// 模型跨版本共享，而路径到池下标的映射按版本不同，解析必须发生在
    /// 知道版本之后。找不到对应贴图时返回 NO_TEXTURE，下游据此跳过该面，
    /// 而不是拿一个错误的索引去取完全无关的贴图。
    ///
    /// 查找顺序与 legacy-cpp 的 FindTexture 一致：
    /// 本版本 -> 挂在该版本上的叠加包 -> 全局。
    /// 少了叠加包这一层，模组导入的贴图永远用不上——模组的方块定义
    /// 引用的贴图只存在于模组包里。
    pub fn texture_pool_index(&self, version_idx: usize, tex_str_idx: u32) -> u32 {
        if tex_str_idx == NO_TEXTURE {
            return NO_TEXTURE;
        }
        let Some(path) = self.strings.get(tex_str_idx as usize) else {
            return NO_TEXTURE;
        };
        if let Some(i) = self
            .versions
            .get(version_idx)
            .and_then(|v| v.texture_by_path.get(path))
        {
            return *i;
        }
        // 叠加包：按挂载顺序查找，先挂上的优先
        for ov in self.overlays_of(version_idx) {
            if let Some(i) = self.versions[ov].texture_by_path.get(path) {
                return *i;
            }
        }
        // 全局回退：某个贴图只被其他版本登记过时仍可命中
        for v in &self.versions {
            if let Some(i) = v.texture_by_path.get(path) {
                return *i;
            }
        }
        NO_TEXTURE
    }

    /// 挂载在某个基础版本上的叠加包下标，按挂载顺序（先挂的在前）。
    ///
    /// 叠加包挂到 dataVersion 最接近的基础版本上。这个关系在载入后确定，
    /// 因为它取决于「库中实际存在哪些基础版本」，而单个包文件里没有
    /// 这个信息。每次调用重新计算：库是只读的，且版本数不大。
    pub fn overlays_of(&self, version_idx: usize) -> Vec<usize> {
        let Some(base) = self.versions.get(version_idx) else {
            return Vec::new();
        };
        if base.is_overlay {
            // 叠加包自身不承载其他叠加包
            return Vec::new();
        }
        self.versions
            .iter()
            .enumerate()
            .filter(|(_, v)| v.is_overlay)
            .filter(|(_, v)| self.nearest_base(v.data_version) == Some(version_idx))
            .map(|(i, _)| i)
            .collect()
    }

    /// dataVersion 最接近的基础版本下标（不含叠加包）
    fn nearest_base(&self, data_version: i32) -> Option<usize> {
        self.versions
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_overlay)
            .min_by_key(|(_, v)| (v.data_version as i64 - data_version as i64).abs())
            .map(|(i, _)| i)
    }

    /// 按 dataVersion 选最接近的版本（下标）。
    ///
    /// 差值按 i64 计算：两个 i32 相减可能溢出，极端取值下
    /// （dataVersion 经截断后接近 i32 边界）会让距离算成负数或 panic。
    /// legacy-cpp 同样显式提升为 64 位（blockdb.cpp:366）。
    ///
    /// 叠加包不参与选择（与 legacy-cpp 的 PickVersion 一致）：它们不是
    /// 某个游戏版本的定义，而是叠加在某个基础版本之上的一层。
    pub fn pick_version(&self, data_version: i32) -> Option<usize> {
        self.nearest_base(data_version)
    }

    /// 把另一个方块库并入自身。
    ///
    /// 用途：用户导入的资源包是独立的 pack 文件，要合并进基础库后
    /// 才能参与检索与渲染。
    ///
    /// 合并必须平移所有池下标：字符串、语言表、贴图、模型、像素缓冲
    /// 各自被追加，而**已有的下标是相对各自池的**。不平移会让叠加包的
    /// 方块指向基础库里毫不相干的模型与贴图——不崩溃，只是画面全错。
    ///
    /// 各层的平移量：
    ///   - 模型面里的 `texture`（字符串表下标） += 字符串池原长度
    ///   - 版本里 `texture_by_path` 的值（贴图池下标） += 贴图池原长度
    ///   - 版本的 `lang_idx`（语言表下标） += 语言表原长度
    ///   - 变体的 `model_idx`（模型池下标） += 模型池原长度
    ///   - 贴图记录的 `offset`（像素缓冲偏移） += 像素缓冲原长度
    pub fn merge_from(&mut self, other: &BlockDb, is_overlay: bool) -> AppResult<()> {
        if other.versions.is_empty() {
            return Err(AppError::Parse("资源包里没有版本数据".into()));
        }
        let str_base = self.strings.len() as u32;
        let lang_base = self.langs.len() as u32;
        let tex_base = self.textures.len() as u32;
        let model_base = self.models.len() as u32;
        let pix_base = self.pixels.len() as u32;

        // 字符串池：语言表与模型面都引用它
        self.strings.extend(other.strings.iter().cloned());

        // 语言表：LangEntry 里存的是字符串本身，直接追加
        self.langs.extend(other.langs.iter().cloned());

        // 贴图池：像素偏移要加上现有缓冲长度
        for t in &other.textures {
            let mut t = t.clone();
            t.offset = t.offset.saturating_add(pix_base);
            self.textures.push(t);
        }
        self.pixels.extend_from_slice(&other.pixels);

        // 模型池：面里的贴图是字符串表下标，必须平移
        for m in &other.models {
            let mut m = m.clone();
            for e in &mut m.elements {
                for f in &mut e.faces {
                    if f.texture != NO_TEXTURE {
                        f.texture = f.texture.saturating_add(str_base);
                    }
                }
            }
            self.models.push(m);
        }

        // 版本
        for v in &other.versions {
            let mut v = v.clone();
            // 语言表要重新取出：读取端就是按 lang_idx 从语言表池里取的，
            // 只平移下标而不重挂，按 lang_idx 取名的调用方会拿不到中文名。
            //
            // 必须用**平移前**的原始下标去 other.langs 里取，不能先加法再
            // 反向减回来：那样一旦 lang_idx 极大，saturating_add 会饱和到
            // u32::MAX，减回去得到的就不是原值，取到的表也就错了。
            let local_idx = v.lang_idx as usize;
            v.langs = other.langs.get(local_idx).cloned().ok_or_else(|| {
                // 越界说明包里的 lang_idx 与语言表数量对不上。这里报错而不是
                // `unwrap_or_default()`：静默给一张空表会让该版本的方块全都
                // 显示不出中文名，而用户看不出是数据坏了还是本来就没有名字。
                AppError::Corrupt(format!(
                    "版本 {} 的语言表下标 {local_idx} 超出该包的语言表数量 {}",
                    v.name,
                    other.langs.len()
                ))
            })?;
            // 加上基数后不会溢出：两侧都受 MAX_LANGS（100_000）限制，
            // 上界是 200_000，远小于 u32::MAX。因此这里用普通加法，
            // 不为不可能发生的边界添加保护逻辑。
            v.lang_idx += lang_base;
            v.is_overlay = is_overlay;
            for idx in v.texture_by_path.values_mut() {
                *idx = idx.saturating_add(tex_base);
            }
            for b in &mut v.blocks {
                for var in b.variants.iter_mut().chain(b.multipart.iter_mut()) {
                    var.model_idx = var.model_idx.saturating_add(model_base);
                }
            }
            self.versions.push(v);
        }
        Ok(())
    }

}

fn read_variant(r: &mut BinReader<'_>, strings: &[String]) -> AppResult<Variant> {
    Ok(Variant {
        when: str_at(strings, r.u32()?),
        model_idx: r.u32()?,
        rot_x_div90: r.u8()?,
        rot_y_div90: r.u8()?,
        uvlock: r.u8()? != 0,
    })
}

fn str_at(strings: &[String], idx: u32) -> String {
    strings.get(idx as usize).cloned().unwrap_or_default()
}

/// 解压 zlib 数据，并校验解压结果恰好等于声明的长度。
///
/// `expect` 来自文件里记录的原始长度，是**不可信输入**：解压展开比同样由
/// 文件决定。若先解压再比对，一个声明很小、实际能展开数 GB 的段会直接把
/// 内存吃光——原先的 `.min(64 MiB)` 只限制预分配容量，不限制增长。
///
/// 这里用 take 限流：最多读 expect + 1 字节，多出的 1 字节用于区分
/// 「恰好符合声明」与「超出声明」。
fn decompress(data: &[u8], expect: usize) -> AppResult<Vec<u8>> {
    use std::io::Read;
    let limit = expect.saturating_add(1) as u64;
    let mut d = flate2::read::ZlibDecoder::new(data).take(limit);
    let mut out = Vec::with_capacity(expect.min(64 * 1024 * 1024));
    d.read_to_end(&mut out)?;
    if out.len() != expect {
        return Err(AppError::Corrupt(format!(
            "解压后长度 {} 与声明的 {} 不符",
            out.len(),
            expect
        )));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "../tests/block/db.rs"]
mod tests;
