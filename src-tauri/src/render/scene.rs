//! 组装前端渲染所需的完整场景数据。
//!
//! 前端用 Three.js 绘制，因此这里把「解析结果」整理成可直接喂给
//! BufferGeometry 与 DataTexture 的形状：按贴图分批的顶点数据，
//! 外加每个批次要用到的贴图像素。

use crate::block::db::{BlockDb, TextureInfo};
use crate::error::{AppError, AppResult};
use crate::schematic::litematic::Schematic;
use crate::render::mesh;
use crate::render::visibility;

/// 一个批次的贴图透明类型。
///
/// 必须区分，因为三种类型要用不同的绘制方式：
/// - `Opaque`：全不透明，画的时候可以写深度，正常遮挡。
/// - `Cutout`：只有全透明与全不透明两种像素（树叶、草、藤蔓）。
///   全透明像素不能画出来，但**画出来的部分必须写深度**，
///   否则后面的方块会从叶子缝隙里透出来。
/// - `Translucent`：含真正的半透明像素（染色玻璃、水）。
///   必须做混合，且不能写深度，否则半透明的面会挡掉后面的几何。
///
/// 一开始只用一个布尔 `has_alpha`，无法区分后两者：按 Cutout 处理会让玻璃
/// 变硬、按 Translucent 处理会让树叶互相透视。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlphaMode {
    Opaque,
    Cutout,
    Translucent,
}

/// 由贴图像素判定透明类型。
///
/// 判据只看 alpha 通道：
/// - 全部 255 -> Opaque
/// - 只有 0 与 255 -> Cutout
/// - 存在 1..=254 -> Translucent
pub fn classify_alpha(rgba: &[u8]) -> AlphaMode {
    let mut saw_zero = false;
    let mut saw_partial = false;
    for px in rgba.chunks_exact(4) {
        match px[3] {
            0 => saw_zero = true,
            255 => {}
            _ => saw_partial = true,
        }
        if saw_partial {
            return AlphaMode::Translucent;
        }
    }
    if saw_zero {
        AlphaMode::Cutout
    } else {
        AlphaMode::Opaque
    }
}

/// 一个批次 = 一张贴图 = 前端一个材质
#[derive(Debug, Clone, PartialEq)]
pub struct SceneBatch {
    /// 贴图池下标
    pub texture: u32,
    /// 顶点位置，每 3 个 f32 一个顶点
    pub positions: Vec<f32>,
    /// 顶点 UV，取值 0..16，前端除以 16 归一化
    pub uvs: Vec<f32>,
    /// 每顶点颜色系数（RGB 三通道，已含明暗与染色）
    pub colors: Vec<f32>,
    /// 三角形索引
    pub indices: Vec<u32>,
    pub texture_width: u16,
    pub texture_height: u16,
    /// 贴图像素，RGBA8888，长度 = 宽 * 高 * 4
    pub texture_rgba: Vec<u8>,
    /// 该批次贴图的透明类型，决定前端如何绘制
    pub alpha_mode: AlphaMode,
}

/// 完整场景
#[derive(Debug, Clone, PartialEq)]
pub struct SceneData {
    pub batches: Vec<SceneBatch>,
    pub total_vertices: usize,
    pub total_triangles: usize,
    /// 投影外框尺寸，供前端设置初始相机
    pub enclosing: [i32; 3],
    /// 投影左下角在世界坐标中的位置，供前端居中
    pub origin: [i32; 3],
    pub mc_version_name: String,
    pub data_version: i32,
    /// 被跳过的方块种类数（该版本方块库里查不到定义的方块）
    pub unresolved_blocks: usize,
}

/// 取一张贴图的 RGBA 像素。越界或长度不足时返回 None。
pub(crate) fn texture_rgba(db: &BlockDb, idx: u32) -> Option<Vec<u8>> {
    let t: &TextureInfo = db.textures.get(idx as usize)?;
    let start = t.offset as usize;
    let end = start + t.len as usize;
    let px = db.pixels.get(start..end)?;
    let want = t.w as usize * t.h as usize * 4;
    if px.len() < want {
        return None;
    }
    Some(px[..want].to_vec())
}

/// 由已载入的投影与方块库组装场景。
///
/// 这样拆分是为了让组装逻辑可脱离文件 IO 单独测试。
pub fn build_from(sc: &Schematic, db: &BlockDb) -> AppResult<SceneData> {
    build_data(sc, db, None)
}

/// 只按指定区域组装场景。区域外不加入邻接方块，因此区域边界面会保留。
pub fn build_from_region(sc: &Schematic, db: &BlockDb, region_name: &str) -> AppResult<SceneData> {
    let region = sc
        .regions
        .iter()
        .find(|r| r.name == region_name)
        .cloned()
        .ok_or_else(|| AppError::Parse(format!("投影中不存在区域：{region_name}")))?;
    let enclosing = region.size;
    let regional = Schematic {
        root: sc.root.clone(),
        regions: vec![region],
        path: sc.path.clone(),
        meta_modified: sc.meta_modified,
        blocks_modified: sc.blocks_modified,
    };
    build_data(&regional, db, Some(enclosing))
}

fn build_data(sc: &Schematic, db: &BlockDb, enclosing: Option<[i32; 3]>) -> AppResult<SceneData> {
    let summary = sc.summary();
    let version_idx = db
        .pick_version(summary.data_version)
        .ok_or_else(|| AppError::Parse("方块库中没有任何可用版本".into()))?;
    let version = &db.versions[version_idx];

    let quads = visibility::collect_quads(sc, db, version_idx);
    let m = mesh::build(&quads);

    // 统计方块库中查不到定义的方块**种类**，供界面提示。
    // 用集合去重：同一方块会出现在多个区域的调色板中，
    // 逐项累加得到的是「(区域, 方块) 组合数」而不是种类数。
    let mut unresolved: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for r in &sc.regions {
        for b in &r.palette {
            if b.is_air() {
                continue;
            }
            if crate::schematic::model::find_block(db, version, &b.name).is_none() {
                unresolved.insert(b.name.as_str());
            }
        }
    }
    let unresolved = unresolved.len();

    let mut batches = Vec::with_capacity(m.batches.len());
    for b in m.batches {
        let Some(t) = db.textures.get(b.texture as usize) else { continue };
        let Some(rgba) = texture_rgba(db, b.texture) else {
            // 贴图数据缺失时跳过该批次：前端拿不到像素就只能画占位色块，
            // 反而掩盖真实问题，不如不画。
            continue;
        };
        let alpha_mode = classify_alpha(&rgba);
        batches.push(SceneBatch {
            texture: b.texture,
            positions: b.positions,
            uvs: b.uvs,
            colors: b.colors,
            indices: b.indices,
            texture_width: t.w,
            texture_height: t.h,
            texture_rgba: rgba,
            alpha_mode,
        });
    }

    let total_vertices = batches.iter().map(|b| b.positions.len() / 3).sum();
    let total_triangles = batches.iter().map(|b| b.indices.len() / 3).sum();

    // 投影在世界坐标中的起点：取所有区域的最小位置
    let mut origin = [i32::MAX; 3];
    for r in &sc.regions {
        for a in 0..3 {
            origin[a] = origin[a].min(r.pos[a]);
        }
    }
    if sc.regions.is_empty() {
        origin = [0, 0, 0];
    }

    Ok(SceneData {
        batches,
        total_vertices,
        total_triangles,
        enclosing: enclosing.unwrap_or(summary.enclosing),
        origin,
        mc_version_name: summary.mc_version_name,
        data_version: summary.data_version,
        unresolved_blocks: unresolved,
    })
}

/// 从文件载入投影并组装场景，方块库由调用方提供。
///
/// 拆成独立入口是为了让调用方复用已载入的方块库：
/// 它解压后约 5.6 MB、含 4 万余条方块定义，每次预览都重新解析会明显变慢。
pub fn build_with_db(schematic_path: &str, db: &BlockDb) -> AppResult<SceneData> {
    if db.versions.is_empty() {
        return Err(AppError::Parse("方块库为空".into()));
    }
    let sc = Schematic::load(schematic_path)?;
    build_from(&sc, db)
}

/// 从文件载入投影并只组装指定区域，方块库由调用方提供。
pub fn build_region_with_db(
    schematic_path: &str,
    db: &BlockDb,
    region_name: &str,
) -> AppResult<SceneData> {
    if db.versions.is_empty() {
        return Err(AppError::Parse("方块库为空".into()));
    }
    let sc = Schematic::load(schematic_path)?;
    build_from_region(&sc, db, region_name)
}

#[cfg(test)]
#[path = "../tests/render/scene.rs"]
mod tests;
