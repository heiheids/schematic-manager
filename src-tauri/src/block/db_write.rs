//! 内部资源包（blocks.dat）的序列化。
//!
//! 是 block/db.rs 读取器的逆运算，格式来源为 legacy-cpp/packbuild.cpp 的
//! SerializePack。导入 Minecraft jar 时用它把解析结果写成 pack 文件。
//!
//! 字符串池必须先于所有段建立：各段存的是字符串下标，边写边收集会让
//! 先写出的段拿到错误下标。
//!
//! 本模块只做序列化，不涉及 zip / PNG / 文件 IO，因此可用「写出再读回」
//! 的往返测试完整验证——这也是保证与读取端一致的最直接手段。
//!
//! 一个容易搞错的点：文件头里的 langCount 是**语言表个数**（不是条目数），
//! 段内先写一个与它相等的重复计数，再逐表写条目数。

use crate::block::db::{BlockDb, LangEntry, Model, ModelElement};
use crate::error::{AppError, AppResult};

/// 小端写入辅助。
///
/// 读取端（block/db.rs 的 BinReader）是手写字节序的，这里保持一致：
/// 既统一了风格，也让本地类型检查不必为 byteorder 额外造桩。
trait WriteLe {
    fn w_u8(&mut self, v: u8) -> AppResult<()>;
    fn w_u16(&mut self, v: u16) -> AppResult<()>;
    fn w_u32(&mut self, v: u32) -> AppResult<()>;
    fn w_i16(&mut self, v: i16) -> AppResult<()>;
    fn w_i32(&mut self, v: i32) -> AppResult<()>;
    fn w_f32(&mut self, v: f32) -> AppResult<()>;
}

impl WriteLe for Vec<u8> {
    fn w_u8(&mut self, v: u8) -> AppResult<()> {
        self.push(v);
        Ok(())
    }
    fn w_u16(&mut self, v: u16) -> AppResult<()> {
        self.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn w_u32(&mut self, v: u32) -> AppResult<()> {
        self.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn w_i16(&mut self, v: i16) -> AppResult<()> {
        self.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn w_i32(&mut self, v: i32) -> AppResult<()> {
        self.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn w_f32(&mut self, v: f32) -> AppResult<()> {
        self.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
}
use std::collections::HashMap;

const MAGIC: &[u8; 8] = b"PMBLKDB\0";
const FORMAT_VERSION: u32 = 3;

/// 字符串池：保持插入顺序，同时记录下标
#[derive(Debug, Default)]
struct StringPool {
    items: Vec<String>,
    index: HashMap<String, u32>,
}

impl StringPool {
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(i) = self.index.get(s) {
            return *i;
        }
        let i = self.items.len() as u32;
        self.items.push(s.to_string());
        self.index.insert(s.to_string(), i);
        i
    }
}

/// 把方块库序列化成 pack 字节。
///
/// 要求恰好一个版本与至多一个语言表：pack 文件的设计就是单版本，
/// 多版本会让读取端只取到第一个、静默丢弃其余，因此这里直接报错。
pub fn serialize(db: &BlockDb) -> AppResult<Vec<u8>> {
    let version = match db.versions.len() {
        1 => &db.versions[0],
        0 => return Err(AppError::Parse("方块库没有任何版本，无法序列化".into())),
        n => {
            return Err(AppError::Parse(format!(
                "pack 文件只容纳一个版本，当前有 {n} 个"
            )))
        }
    };
    if db.langs.len() > 1 {
        return Err(AppError::Parse(format!(
            "pack 文件只容纳一个语言表，当前有 {} 个",
            db.langs.len()
        )));
    }

    // ---- 先建字符串池 ----
    // 池以 db.strings 为起点：模型面里的 texture 字段存的就是该表的下标，
    // 保留原有下标才能保证写出后仍然可解析。
    let mut pool = StringPool::default();
    for s in &db.strings {
        pool.intern(s);
    }

    let name_idx = pool.intern(&version.name);
    let block_name_idx: Vec<u32> = version.blocks.iter().map(|b| pool.intern(&b.name)).collect();
    let variant_when_idx: Vec<Vec<u32>> = version
        .blocks
        .iter()
        .map(|b| b.variants.iter().map(|v| pool.intern(&v.when)).collect())
        .collect();
    let multipart_when_idx: Vec<Vec<u32>> = version
        .blocks
        .iter()
        .map(|b| b.multipart.iter().map(|v| pool.intern(&v.when)).collect())
        .collect();
    // 贴图路径映射的下标需一并登记，否则读取端取不到路径字符串
    let mut tex_entries: Vec<(u32, u32)> = Vec::with_capacity(version.texture_by_path.len());
    for (path, idx) in &version.texture_by_path {
        tex_entries.push((pool.intern(path), *idx));
    }
    // 顺序固定，保证同一输入产出同一文件
    tex_entries.sort_by_key(|(p, _)| *p);

    let empty: Vec<LangEntry> = Vec::new();
    let langs: &Vec<LangEntry> = db.langs.first().unwrap_or(&empty);
    let lang_idx: Vec<[u32; 3]> = langs
        .iter()
        .map(|e| {
            [
                pool.intern(&e.block_name),
                pool.intern(&e.en),
                pool.intern(&e.zh),
            ]
        })
        .collect();

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(MAGIC);
    out.w_u32(FORMAT_VERSION)?;
    out.w_u32(1)?; // 版本数
    out.w_u32(db.textures.len() as u32)?;
    out.w_u32(db.models.len() as u32)?;
    // 语言表**个数**：无语言表时也要写 1（读取端按此计数逐表读取）
    out.w_u32(1)?;

    // ---- 字符串池 ----
    out.w_u32(pool.items.len() as u32)?;
    for s in &pool.items {
        out.extend_from_slice(s.as_bytes());
        out.w_u8(0)?;
    }

    // ---- 语言表 ----
    out.w_u32(1)?; // 段内重复计数，须与文件头的个数一致
    out.w_u32(langs.len() as u32)?; // 该表的条目数
    for ids in &lang_idx {
        out.w_u32(ids[0])?;
        out.w_u32(ids[1])?;
        out.w_u32(ids[2])?;
    }

    // ---- 贴图表 ----
    out.w_u32(db.textures.len() as u32)?;
    for t in &db.textures {
        out.w_u16(t.w)?;
        out.w_u16(t.h)?;
        out.w_u32(t.offset)?;
        out.w_u32(t.len)?;
        out.w_u8(u8::from(t.has_alpha))?;
    }
    // 像素池：zlib 压缩，先写原始长度再写压缩长度与内容
    let comp = compress_zlib(&db.pixels)?;
    out.w_u32(db.pixels.len() as u32)?;
    out.w_u32(comp.len() as u32)?;
    out.extend_from_slice(&comp);

    // ---- 模型表 ----
    out.w_u32(db.models.len() as u32)?;
    for m in &db.models {
        write_model(&mut out, m)?;
    }

    // ---- 版本段 ----
    out.w_u32(1)?;
    out.w_u32(name_idx)?;
    out.w_i32(version.data_version)?;
    out.w_u32(0)?; // 语言表下标
    out.w_u32(tex_entries.len() as u32)?;
    for (path_idx, pool_idx) in &tex_entries {
        out.w_u32(*path_idx)?;
        out.w_u32(*pool_idx)?;
    }
    out.w_u32(version.blocks.len() as u32)?;
    for (i, b) in version.blocks.iter().enumerate() {
        out.w_u32(block_name_idx[i])?;
        // 变体与 multipart 的条数写成 u16，必须先校验上限。
        // `as u16` 会静默截断：65536 条会写成 0，而写入端仍照实写 65536 条，
        // 读取端按 0 条读取后，会把变体的字节当成下一个方块的字段继续消费，
        // 整个版本段从此错位——不报错，只是数据全错。
        // 同文件的 write_model 对元素数已有相同的守卫，这里保持一致。
        if b.variants.len() > u16::MAX as usize {
            return Err(AppError::Corrupt(format!(
                "方块 {} 的变体数为 {}，超过上限 {}",
                b.name,
                b.variants.len(),
                u16::MAX
            )));
        }
        out.w_u16(b.variants.len() as u16)?;
        for (k, v) in b.variants.iter().enumerate() {
            write_variant(&mut out, v, variant_when_idx[i][k])?;
        }
        if b.multipart.len() > u16::MAX as usize {
            return Err(AppError::Corrupt(format!(
                "方块 {} 的 multipart 数为 {}，超过上限 {}",
                b.name,
                b.multipart.len(),
                u16::MAX
            )));
        }
        out.w_u16(b.multipart.len() as u16)?;
        for (k, v) in b.multipart.iter().enumerate() {
            write_variant(&mut out, v, multipart_when_idx[i][k])?;
        }
    }

    Ok(out)
}

fn write_variant(
    out: &mut Vec<u8>,
    v: &crate::block::db::Variant,
    when_idx: u32,
) -> AppResult<()> {
    out.w_u32(when_idx)?;
    out.w_u32(v.model_idx)?;
    out.w_u8(v.rot_x_div90)?;
    out.w_u8(v.rot_y_div90)?;
    out.w_u8(u8::from(v.uvlock))?;
    Ok(())
}

fn write_model(out: &mut Vec<u8>, m: &Model) -> AppResult<()> {
    out.w_u8(u8::from(m.translucent))?;
    if m.elements.len() > u16::MAX as usize {
        return Err(AppError::Parse(format!(
            "模型元素数 {} 超出格式上限",
            m.elements.len()
        )));
    }
    out.w_u16(m.elements.len() as u16)?;
    for e in &m.elements {
        write_element(out, e)?;
    }
    Ok(())
}

fn write_element(out: &mut Vec<u8>, e: &ModelElement) -> AppResult<()> {
    for k in 0..3 {
        out.w_f32(e.from[k])?;
    }
    for k in 0..3 {
        out.w_f32(e.to[k])?;
    }
    out.w_u8(u8::from(e.has_rotation))?;
    if e.has_rotation {
        out.w_u8(e.rot_kind)?;
        for k in 0..3 {
            out.w_f32(e.origin[k])?;
        }
        out.w_u8(e.axis)?;
        out.w_f32(e.angle)?;
        out.w_u8(u8::from(e.rescale))?;
        for k in 0..3 {
            out.w_f32(e.rot_vec[k])?;
        }
    }
    out.w_u8(e.face_mask)?;

    // 面的顺序必须与 faceMask 的位序一致：读取端按位依次取面
    let mut fi = 0usize;
    for bit in 0..6u8 {
        if e.face_mask & (1u8 << bit) == 0 {
            continue;
        }
        let f = e.faces.get(fi).ok_or_else(|| {
            AppError::Parse(format!(
                "元素的面数与 faceMask 不符：位 {bit} 已置位但只有 {} 个面",
                e.faces.len()
            ))
        })?;
        fi += 1;
        // 贴图字段是字符串表下标，原样写出即可
        out.w_u32(f.texture)?;
        for k in 0..4 {
            out.w_f32(f.uv[k])?;
        }
        out.w_u8(f.cullface)?;
        out.w_i16(f.tintindex)?;
        out.w_u16(f.rotation)?;
    }
    if fi != e.faces.len() {
        return Err(AppError::Parse(format!(
            "元素的面数与 faceMask 不符：{} 个面但只写出 {fi} 个",
            e.faces.len()
        )));
    }
    Ok(())
}

/// zlib 压缩（PNG/资源包使用的容器格式）
fn compress_zlib(data: &[u8]) -> AppResult<Vec<u8>> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    e.write_all(data)?;
    Ok(e.finish()?)
}

#[cfg(test)]
#[path = "../tests/block/db_write.rs"]
mod tests;
