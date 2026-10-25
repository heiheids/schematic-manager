//! 把可见四边形整理成前端可直接使用的网格数据。
//!
//! 渲染交给前端的 Three.js，因此本模块只负责几何整理：
//! 按贴图分批、展开顶点、生成三角形索引。
//! 按贴图分批是为了让前端「一批一个材质」，避免自己再维护图集。

use crate::render::visibility::Quad;

/// 一个批次对应一张贴图，前端据此创建一个材质。
#[derive(Debug, Clone, PartialEq)]
pub struct TextureBatch {
    /// 贴图池下标
    pub texture: u32,
    /// 顶点位置，每 3 个 f32 为一个顶点
    pub positions: Vec<f32>,
    /// 顶点 UV，每 2 个 f32 为一个顶点
    pub uvs: Vec<f32>,
    /// 每顶点的颜色系数（RGB 三通道，各 0..1）。
    /// 已把「面明暗」与「方块染色」乘在一起，前端直接作为顶点色使用。
    pub colors: Vec<f32>,
    /// 三角形索引，每 3 个构成一个三角形
    pub indices: Vec<u32>,
}

impl TextureBatch {
    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// 整个投影的网格数据
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshData {
    pub batches: Vec<TextureBatch>,
    /// 顶点总数，供前端预估内存
    pub total_vertices: usize,
    pub total_triangles: usize,
}

/// 把四边形列表按贴图分批展开成三角形网格。
///
/// 每个四边形拆成两个三角形。UV 直接沿用模型空间的 0..16 取值，
/// 由前端除以 16 归一化——这样 Rust 侧不必关心贴图分辨率。
pub fn build(quads: &[Quad]) -> MeshData {
    let mut batches: Vec<TextureBatch> = Vec::new();
    // 贴图下标 -> 批次下标。四边形可达上百万而批次可达上千，
    // 逐个四边形线性扫批次会退化成平方复杂度。
    let mut by_texture: std::collections::HashMap<u32, usize> =
        std::collections::HashMap::new();

    for q in quads {
        // 贴图为哨兵值时跳过：没有可用的贴图，画出来也只是占位色块
        if q.texture == crate::block::db::NO_TEXTURE {
            continue;
        }
        let idx = match by_texture.get(&q.texture) {
            Some(i) => *i,
            None => {
                batches.push(TextureBatch {
                    texture: q.texture,
                    positions: Vec::new(),
                    uvs: Vec::new(),
                    colors: Vec::new(),
                    indices: Vec::new(),
                });
                let i = batches.len() - 1;
                by_texture.insert(q.texture, i);
                i
            }
        };
        let batch = &mut batches[idx];

        let base = batch.vertex_count() as u32;
        for k in 0..4 {
            let p = &q.corners[k];
            batch.positions.extend_from_slice(p);
            batch.uvs.extend_from_slice(&q.uv[k]);
            // 明暗与染色合成逐顶点颜色：贴图是灰度时染色才看得出效果，
            // 而明暗也要一起带上，否则旋转过的面会失去立体感
            for ch in 0..3 {
                batch.colors.push(q.shade * q.tint[ch]);
            }
        }
        // 四边形按 0-2-1、0-3-2 拆成两个三角形。
        //
        // 顺序是**反**的（不是直觉上的 0-1-2、0-2-3），这一点不能改：
        // 角点顺序由 Minecraft 模型规范固定（见 geometry::build_face_corners，
        // 它决定 UV 的方向），而这些角点按 0-1-2 组成的三角形，其几何法线
        // 指向方块**内部**。前端材质用 THREE.FrontSide，只有逆时针（法线朝
        // 观察者）的面才画，于是六个面从外面看全是背面被剔除，画面变成
        // 「看得见对面墙的内侧」——也就是看穿方块。
        //
        // legacy-cpp 的软件光栅器不做背面剔除（render.cpp 的 drawTri 只判
        // 面积是否为 0），因此同样的角点顺序在那里没问题，移植到 three.js
        // 才暴露出来。
        //
        // 交换后两个顶点即可翻转绕序，且不改动顶点本身，UV 与明暗的对应
        // 关系保持不变。
        batch.indices.extend_from_slice(&[base, base + 2, base + 1]);
        batch.indices.extend_from_slice(&[base, base + 3, base + 2]);
    }

    // 结果稳定：按贴图下标排序，便于测试与缓存比对
    batches.sort_by_key(|b| b.texture);

    let total_vertices = batches.iter().map(|b| b.vertex_count()).sum();
    let total_triangles = batches.iter().map(|b| b.triangle_count()).sum();
    MeshData { batches, total_vertices, total_triangles }
}

#[cfg(test)]
#[path = "../tests/render/mesh.rs"]
mod tests;
