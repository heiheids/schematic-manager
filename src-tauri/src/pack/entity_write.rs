//! 实体贴图库的写入端：把客户端 jar 的 entity/ 贴图并入磁盘上的库文件。
//!
//! 容器格式的字段含义与读取端保持一致（见 block/entity_db.rs 头部），这里按
//! 「追加或替换一个版本」的方式写：读出现有库 → 去掉同数据版本的旧条目
//! → 并入新贴图 → 整体重写。被替换版本的贴图字节留在像素池里不做回收
//! ——重写偏移的代价远大于这点空间，库文件本身也才几十 MB 量级。

use crate::block::entity_db::{EntityDb, EntityTexture, EntityVersion};
use crate::error::{AppError, AppResult};
use crate::pack::jar::JarFile;
use crate::pack::build::JarSource;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 库文件魔数与格式版本，与 block/entity_db.rs 读取端一致。
const MAGIC: &[u8] = b"SMET";
const FORMAT_VERSION: u32 = 1;

/// 贴图路径前缀与目录名。库里的键形如 `entity/chest/normal`——
/// 只去掉 `assets/minecraft/textures/`，与读取端、方块贴图的
/// `block/stone` 同一套写法；若去掉到 `entity/` 之后，键就成了
/// `chest/normal`，按文档传 `entity/chest/normal` 的调用方会查不到。
const TEXTURES_PREFIX: &str = "assets/minecraft/textures/";
const ENTITY_DIR: &str = "entity/";

/// 把一个客户端 jar 的实体贴图并入 path 处的库文件，返回并入的贴图数。
///
/// 同一数据版本重复添加时替换旧条目（等价于修正错误数据），其余版本
/// 不受影响。写文件先落临时文件再改名，避免中途失败留下半个库。
pub fn add_version(
    path: &Path,
    jar: &JarFile,
    data_version: i32,
    version_name: &str,
) -> AppResult<usize> {
    let mut db = if path.is_file() {
        EntityDb::load(path)?
    } else {
        EntityDb {
            paths: Vec::new(),
            textures: Vec::new(),
            versions: Vec::new(),
            pixels: Vec::new(),
        }
    };
    db.versions.retain(|v| v.data_version != data_version);

    // 按内容去重：整张 RGBA 做 key。两个版本之间大量贴图逐字节相同，
    // 不 dedupe 的话像素池会随版本数线性膨胀。
    let mut dedupe: HashMap<(u16, u16, Vec<u8>), u32> = HashMap::new();
    for (i, t) in db.textures.iter().enumerate() {
        let px = db.pixels[t.offset as usize..(t.offset + t.len) as usize].to_vec();
        dedupe.insert((t.w, t.h, px), i as u32);
    }

    let mut by_path: HashMap<String, u32> = HashMap::new();
    let mut decoded = 0usize;
    for entry in JarSource::entries(jar) {
        let Some(stripped) = entry.strip_prefix(TEXTURES_PREFIX) else {
            continue;
        };
        if !stripped.starts_with(ENTITY_DIR) {
            continue;
        }
        // zip 里偶尔出现大写扩展名，比较时统一转小写
        let lower = stripped.to_ascii_lowercase();
        if !lower.ends_with(".png") {
            continue;
        }
        let rel = &stripped[..stripped.len() - 4];
        let (w, h, rgba) = jar
            .decode_texture(&format!("minecraft:{rel}"))
            .ok_or_else(|| AppError::Parse(format!("实体贴图 {rel} 解码失败")))?;
        let idx = match dedupe.get(&(w, h, rgba.clone())) {
            Some(&i) => i,
            None => {
                let offset = u32::try_from(db.pixels.len())
                    .map_err(|_| AppError::Parse("像素池超出 4GB 上限".into()))?;
                let len = u32::try_from(rgba.len())
                    .map_err(|_| AppError::Parse("单张贴图超出 4GB 上限".into()))?;
                let has_alpha = rgba.chunks_exact(4).any(|p| p[3] != 255);
                db.pixels.extend_from_slice(&rgba);
                db.textures.push(EntityTexture {
                    w,
                    h,
                    offset,
                    len,
                    has_alpha,
                });
                let idx = (db.textures.len() - 1) as u32;
                dedupe.insert((w, h, rgba), idx);
                idx
            }
        };
        by_path.insert(rel.to_string(), idx);
        decoded += 1;
    }
    db.versions.push(EntityVersion {
        data_version,
        name: version_name.to_string(),
        by_path,
    });
    db.versions.sort_by_key(|v| v.data_version);

    // 字符串表重写为「所有版本实际引用到的路径」的排序集合：被替换版本
    // 独有的路径随之消失，表保持紧凑；像素池不回收是空间换实现简单，
    // 字符串没有这个问题。
    let mut paths: Vec<String> = db
        .versions
        .iter()
        .flat_map(|v| v.by_path.keys().cloned())
        .collect();
    paths.sort();
    paths.dedup();
    db.paths = paths;

    write_db(path, &db)?;
    Ok(decoded)
}

/// 序列化成容器字节（未压缩）。
///
/// 读取端的测试用它做往返验证：构造内存库 → 序列化 → load_from_bytes，
/// 保证写入端产出的每个字段都能被读取端正确还原。
pub(crate) fn serialize(db: &EntityDb) -> AppResult<Vec<u8>> {
    let str_idx: HashMap<&str, u32> = db
        .paths
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i as u32))
        .collect();

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());

    out.extend_from_slice(&(db.paths.len() as u32).to_le_bytes());
    for s in &db.paths {
        let b = s.as_bytes();
        let n = u16::try_from(b.len())
            .map_err(|_| AppError::Parse(format!("路径过长，无法写入实体贴图库：{s}")))?;
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(b);
    }

    out.extend_from_slice(&(db.textures.len() as u32).to_le_bytes());
    for t in &db.textures {
        out.extend_from_slice(&t.w.to_le_bytes());
        out.extend_from_slice(&t.h.to_le_bytes());
        out.extend_from_slice(&t.offset.to_le_bytes());
        out.extend_from_slice(&t.len.to_le_bytes());
        out.push(t.has_alpha as u8);
    }

    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    e.write_all(&db.pixels)?;
    let comp = e.finish()?;
    out.extend_from_slice(&(db.pixels.len() as u32).to_le_bytes());
    out.extend_from_slice(&(comp.len() as u32).to_le_bytes());
    out.extend_from_slice(&comp);

    out.extend_from_slice(&(db.versions.len() as u32).to_le_bytes());
    for v in &db.versions {
        out.extend_from_slice(&(v.data_version as u32).to_le_bytes());
        let nb = v.name.as_bytes();
        let nn = u16::try_from(nb.len()).map_err(|_| {
            AppError::Parse(format!("版本名过长，无法写入实体贴图库：{}", v.name))
        })?;
        out.extend_from_slice(&nn.to_le_bytes());
        out.extend_from_slice(nb);
        out.extend_from_slice(&(v.by_path.len() as u32).to_le_bytes());
        // 路径排序后写出，保证同一输入产出同一文件
        let mut entries: Vec<(&String, &u32)> = v.by_path.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (p, tex) in entries {
            let si = str_idx
                .get(p.as_str())
                .ok_or_else(|| AppError::Parse(format!("路径 {p} 不在字符串表中")))?;
            out.extend_from_slice(&si.to_le_bytes());
            out.extend_from_slice(&tex.to_le_bytes());
        }
    }
    Ok(out)
}

/// 序列化、压缩、落盘（临时文件 + 改名）。
fn write_db(path: &Path, db: &EntityDb) -> AppResult<()> {
    let raw = serialize(db)?;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    e.write_all(&raw)?;
    let packed = e.finish()?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Parse(format!("创建目录 {} 失败：{e}", parent.display())))?;
    }
    let mut tmp: PathBuf = path.as_os_str().to_owned().into();
    tmp.as_mut_os_string().push(".tmp");
    std::fs::write(&tmp, &packed)
        .map_err(|e| AppError::Parse(format!("写入 {} 失败：{e}", tmp.display())))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| AppError::Parse(format!("替换 {} 失败：{e}", path.display())))?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/pack/entity_write.rs"]
mod tests;
