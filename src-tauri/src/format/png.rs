//! PNG 编解码。
//!
//! 解码用于：Minecraft 客户端与模组 jar 里的贴图是 PNG，而内部资源包格式存的
//! 是 RGBA8888 原始像素，导入 jar 时必须解码。
//! 编码用于：方块栏的方块渲染图要作为 PNG 交给前端显示，见 `render::icon`。
//!
//! 本机无法获取 png 类依赖（crates.io 不可达），故自行实现。只解实际会用到的
//! 子集：8 位与 4 位、非隔行。遇到不支持的形式明确报错，而不是给出错色。
//!
//! 分层：`unfilter` 与 `to_rgba` 只处理已解压的扫描线数据，不依赖 zlib，
//! 因此可脱离压缩环节单独测试——这是本文件测试的主要对象。

use crate::error::{AppError, AppResult};

/// 解码后的图像
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// RGBA8888，长度 = width * height * 4
    pub rgba: Vec<u8>,
}

/// 像素调色板
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Palette {
    /// 每项 3 字节 RGB
    pub rgb: Vec<[u8; 3]>,
    /// 每项 1 字节 alpha，长度可能与 rgb 相同或为空
    pub alpha: Vec<u8>,
}

/// 色型对应的每像素通道数
fn channels(color_type: u8) -> AppResult<usize> {
    match color_type {
        0 => Ok(1), // 灰度
        2 => Ok(3), // RGB
        3 => Ok(1), // 调色板索引
        4 => Ok(2), // 灰度 + alpha
        6 => Ok(4), // RGBA
        other => Err(AppError::Corrupt(format!("PNG 色型 {other} 不受支持"))),
    }
}

/// 一行扫描线的字节数（不含行首的滤波字节）
fn stride_of(width: u32, color_type: u8, bit_depth: u8) -> AppResult<usize> {
    let ch = channels(color_type)?;
    let bits = (width as usize)
        .checked_mul(ch)
        .and_then(|v| v.checked_mul(bit_depth as usize))
        .ok_or_else(|| AppError::Corrupt("PNG 宽度过大".into()))?;
    Ok((bits + 7) / 8)
}

/// 滤波时使用的「每像素字节数」（PNG 规范规定最小为 1）
fn filter_bpp(color_type: u8, bit_depth: u8) -> AppResult<usize> {
    let ch = channels(color_type)?;
    let bits = ch * bit_depth as usize;
    Ok(((bits + 7) / 8).max(1))
}

/// 还原 PNG 的行滤波。
///
/// `filtered` 为 IDAT 拼接并解压后的数据，每行开头有一个滤波类型字节，
/// 因此每行占 `stride + 1` 字节。四种滤波都要用到左、上、左上三个邻居，
/// 必须逐行顺序还原。
pub fn unfilter(
    filtered: &[u8],
    width: u32,
    height: u32,
    color_type: u8,
    bit_depth: u8,
) -> AppResult<Vec<u8>> {
    let stride = stride_of(width, color_type, bit_depth)?;
    let bpp = filter_bpp(color_type, bit_depth)?;
    let row_len = stride + 1;
    let expect = row_len
        .checked_mul(height as usize)
        .ok_or_else(|| AppError::Corrupt("PNG 尺寸过大".into()))?;
    if filtered.len() != expect {
        return Err(AppError::Corrupt(format!(
            "PNG 像素数据长度 {} 与预期 {expect} 不符（宽 {width} 高 {height}）",
            filtered.len()
        )));
    }

    let mut out = vec![0u8; stride * height as usize];
    for y in 0..height as usize {
        let row = &filtered[y * row_len..(y + 1) * row_len];
        let ft = row[0];
        let src = &row[1..];
        let base = y * stride;
        for x in 0..stride {
            let left = if x >= bpp { out[base + x - bpp] } else { 0 };
            let up = if y > 0 { out[base - stride + x] } else { 0 };
            let up_left = if y > 0 && x >= bpp {
                out[base - stride + x - bpp]
            } else {
                0
            };
            // Paeth 预测器：取与 p 最接近的邻居
            let paeth = |a: u8, b: u8, c: u8| -> u8 {
                let (pa, pb, pc) = (
                    (b as i32 - c as i32).abs(),
                    (a as i32 - c as i32).abs(),
                    (a as i32 + b as i32 - 2 * c as i32).abs(),
                );
                if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                }
            };
            out[base + x] = match ft {
                0 => src[x],
                1 => src[x].wrapping_add(left),
                2 => src[x].wrapping_add(up),
                3 => src[x].wrapping_add(((left as u16 + up as u16) / 2) as u8),
                4 => src[x].wrapping_add(paeth(left, up, up_left)),
                other => {
                    return Err(AppError::Corrupt(format!("PNG 滤波类型 {other} 无效")))
                }
            };
        }
    }
    Ok(out)
}

/// 从低位深（1/2/4 位）的扫描线中取出第 i 个索引
fn sub_byte_index(row: &[u8], i: usize, bit_depth: u8) -> u8 {
    let per_byte = 8 / bit_depth as usize;
    let byte = row[i / per_byte];
    let shift = 8 - bit_depth as usize * ((i % per_byte) + 1);
    (byte >> shift) & ((1u16 << bit_depth) - 1) as u8
}

/// 把已还原的扫描线转成 RGBA8888。
///
/// 支持 8 位的灰度/RGB/调色板/灰度+alpha/RGBA，以及 4 位与 1/2 位的
/// 灰度与调色板（Minecraft 贴图基本是 8 位 RGBA，但资源包里偶有低位深）。
pub fn to_rgba(
    raw: &[u8],
    width: u32,
    height: u32,
    color_type: u8,
    bit_depth: u8,
    palette: &Palette,
    transparency: Option<&[[u8; 3]]>,
) -> AppResult<Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    let stride = stride_of(width, color_type, bit_depth)?;
    if raw.len() != stride * h {
        return Err(AppError::Corrupt("PNG 扫描线长度与尺寸不符".into()));
    }
    let mut out = vec![0u8; w * h * 4];

    for y in 0..h {
        let row = &raw[y * stride..(y + 1) * stride];
        for x in 0..w {
            let px = (y * w + x) * 4;
            match (color_type, bit_depth) {
                (0, 8) => {
                    let g = row[x];
                    out[px] = g;
                    out[px + 1] = g;
                    out[px + 2] = g;
                    out[px + 3] = 255;
                }
                (0, 4) | (0, 2) | (0, 1) => {
                    // 低位深灰度需按位深缩放到 0..255
                    let v = sub_byte_index(row, x, bit_depth);
                    let max = ((1u16 << bit_depth) - 1) as u32;
                    let g = ((v as u32 * 255) / max) as u8;
                    out[px] = g;
                    out[px + 1] = g;
                    out[px + 2] = g;
                    out[px + 3] = 255;
                }
                (2, 8) => {
                    out[px] = row[x * 3];
                    out[px + 1] = row[x * 3 + 1];
                    out[px + 2] = row[x * 3 + 2];
                    out[px + 3] = 255;
                }
                (3, 8) => {
                    let idx = row[x] as usize;
                    let c = palette
                        .rgb
                        .get(idx)
                        .ok_or_else(|| AppError::Corrupt(format!("调色板下标 {idx} 越界")))?;
                    out[px] = c[0];
                    out[px + 1] = c[1];
                    out[px + 2] = c[2];
                    out[px + 3] = palette.alpha.get(idx).copied().unwrap_or(255);
                }
                (3, 4) | (3, 2) | (3, 1) => {
                    let idx = sub_byte_index(row, x, bit_depth) as usize;
                    let c = palette
                        .rgb
                        .get(idx)
                        .ok_or_else(|| AppError::Corrupt(format!("调色板下标 {idx} 越界")))?;
                    out[px] = c[0];
                    out[px + 1] = c[1];
                    out[px + 2] = c[2];
                    out[px + 3] = palette.alpha.get(idx).copied().unwrap_or(255);
                }
                (4, 8) => {
                    let g = row[x * 2];
                    out[px] = g;
                    out[px + 1] = g;
                    out[px + 2] = g;
                    out[px + 3] = row[x * 2 + 1];
                }
                (6, 8) => {
                    out[px] = row[x * 4];
                    out[px + 1] = row[x * 4 + 1];
                    out[px + 2] = row[x * 4 + 2];
                    out[px + 3] = row[x * 4 + 3];
                }
                (ct, bd) => {
                    return Err(AppError::Corrupt(format!(
                        "PNG 色型 {ct} 位深 {bd} 的组合不受支持"
                    )))
                }
            }
        }
    }

    // tRNS 对灰度与 RGB 表示「哪一个采样值视为全透明」
    if let Some(t) = transparency {
        match (color_type, bit_depth) {
            (0, 8) => {
                let key = t[0][0];
                for i in 0..w * h {
                    if raw[i] == key {
                        out[i * 4 + 3] = 0;
                    }
                }
            }
            // 低位深灰度：tRNS 里的透明键仍是 2 字节大端，采样值放在低字节，
            // 解析时已取 body[1]（见 tRNS 分支），因此它就是 0..2^位深-1
            // 范围的**原始采样值**，不与 0..255 的缩放值比较。
            //
            // 这一段原先落在 `_ => {}` 上被静默忽略：同一张 PNG 在 8 位灰度
            // 下有透明、换成 1/2/4 位灰度就没有透明，而这种差异只表现为
            // 「贴图该透的地方不透」，很难追查。
            (0, 4) | (0, 2) | (0, 1) => {
                let key = t[0][0];
                for y in 0..h {
                    let row = &raw[y * stride..(y + 1) * stride];
                    for x in 0..w {
                        if sub_byte_index(row, x, bit_depth) == key {
                            out[(y * w + x) * 4 + 3] = 0;
                        }
                    }
                }
            }
            (2, 8) => {
                let key = t[0];
                for y in 0..h {
                    let row = &raw[y * stride..(y + 1) * stride];
                    for x in 0..w {
                        if row[x * 3] == key[0] && row[x * 3 + 1] == key[1] && row[x * 3 + 2] == key[2]
                        {
                            out[(y * w + x) * 4 + 3] = 0;
                        }
                    }
                }
            }
            // 其余组合不会带透明键：调色板的透明度走 palette.alpha
            // （解析 tRNS 时就分流了），灰度+alpha 与 RGBA 自带 alpha 通道，
            // 而 16 位等未支持的组合在进入本函数前就被拒绝了。
            _ => {}
        }
    }

    Ok(out)
}

/// 读取大端 u32
fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// 解码 PNG 字节
pub fn decode(data: &[u8]) -> AppResult<Image> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if data.len() < 8 || data[..8] != SIG {
        return Err(AppError::Corrupt("不是 PNG：文件签名不符".into()));
    }

    let mut pos = 8usize;
    let (mut width, mut height) = (0u32, 0u32);
    let (mut color_type, mut bit_depth) = (0u8, 0u8);
    let mut idat: Vec<u8> = Vec::new();
    let mut palette = Palette::default();
    let mut transparency: Option<Vec<[u8; 3]>> = None;
    let mut saw_header = false;

    while pos + 8 <= data.len() {
        let len = be32(&data[pos..]) as usize;
        let ctype = &data[pos + 4..pos + 8];
        let body_start = pos + 8;
        let body_end = body_start
            .checked_add(len)
            .ok_or_else(|| AppError::Corrupt("PNG 块长度溢出".into()))?;
        if body_end + 4 > data.len() {
            return Err(AppError::Corrupt("PNG 块数据不完整".into()));
        }
        let body = &data[body_start..body_end];

        match ctype {
            b"IHDR" => {
                if body.len() < 13 {
                    return Err(AppError::Corrupt("IHDR 长度不足".into()));
                }
                width = be32(body);
                height = be32(&body[4..]);
                bit_depth = body[8];
                color_type = body[9];
                if width == 0 || height == 0 {
                    return Err(AppError::Corrupt("PNG 尺寸为 0".into()));
                }
                if body[12] != 0 {
                    return Err(AppError::UnsupportedVersion(
                        "暂不支持隔行（Adam7）PNG".into(),
                    ));
                }
                if !matches!(bit_depth, 1 | 2 | 4 | 8) {
                    return Err(AppError::UnsupportedVersion(format!(
                        "暂不支持 {bit_depth} 位深"
                    )));
                }
                channels(color_type)?; // 提前校验色型
                saw_header = true;
            }
            b"PLTE" => {
                palette.rgb = body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            }
            b"tRNS" => {
                match color_type {
                    // 调色板透明度逐项给出
                    3 => palette.alpha = body.to_vec(),
                    // 灰度/RGB 的透明键，取前 2 / 6 字节
                    0 if body.len() >= 2 => {
                        transparency = Some(vec![[body[1], 0, 0]]);
                    }
                    2 if body.len() >= 6 => {
                        transparency = Some(vec![[body[1], body[3], body[5]]]);
                    }
                    _ => {}
                }
            }
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {} // 其余块（gAMA、pHYs 等）与像素无关
        }
        pos = body_end + 4;
    }

    if !saw_header {
        return Err(AppError::Corrupt("PNG 缺少 IHDR".into()));
    }
    if idat.is_empty() {
        return Err(AppError::Corrupt("PNG 缺少 IDAT".into()));
    }

    let raw = crate::format::nbt::inflate_zlib(&idat)?;
    let pixels = unfilter(&raw, width, height, color_type, bit_depth)?;
    let rgba = to_rgba(
        &pixels,
        width,
        height,
        color_type,
        bit_depth,
        &palette,
        transparency.as_deref(),
    )?;
    Ok(Image { width, height, rgba })
}

/// CRC32 查表（PNG 各块用的就是这一种）。
fn crc_table() -> &'static [u32; 256] {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    })
}

fn crc32(data: &[u8]) -> u32 {
    let t = crc_table();
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// 写一个 PNG 块：长度 + 类型 + 数据 + 覆盖「类型 + 数据」的 CRC32
fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// 编码 8 位 RGBA PNG。
///
/// 只写必需的那几块：不做调色板、不做隔行、逐行滤波固定用 0（不滤波）。
/// 图标只有几十像素见方，滤波换来的一点体积收益抵不上实现与验证的成本。
/// 解压用的同样是 flate2 的 zlib，与解码路径同一套实现。
pub fn encode(width: u32, height: u32, rgba: &[u8]) -> AppResult<Vec<u8>> {
    if width == 0 || height == 0 {
        return Err(AppError::Corrupt("PNG 尺寸为 0".into()));
    }
    let want = width as usize * height as usize * 4;
    if rgba.len() < want {
        return Err(AppError::Corrupt(format!(
            "编码 PNG 需要 {want} 字节像素，实际只有 {}",
            rgba.len()
        )));
    }

    // 每条扫描线前面一个滤波类型字节
    let stride = width as usize * 4;
    let mut raw = Vec::with_capacity(height as usize * (1 + stride));
    for y in 0..height as usize {
        raw.push(0);
        raw.extend_from_slice(&rgba[y * stride..y * stride + stride]);
    }

    use std::io::Write;
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&raw)
        .map_err(|e| AppError::Corrupt(format!("PNG 压缩失败: {e}")))?;
    let compressed = enc
        .finish()
        .map_err(|e| AppError::Corrupt(format!("PNG 压缩失败: {e}")))?;

    let mut out = Vec::with_capacity(compressed.len() + 64);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    // 位深 8、色型 6（RGBA）、压缩/滤波/隔行方式均为 0
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    write_chunk(&mut out, b"IHDR", &ihdr);
    write_chunk(&mut out, b"IDAT", &compressed);
    write_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

#[cfg(test)]
#[path = "../tests/format/png.rs"]
mod tests;
