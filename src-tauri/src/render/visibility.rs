//! 可见面判定：邻面剔除与四边形生成。
//!
//! 对照 legacy-cpp/render.cpp 的 ComputeOpaque 与 RenderSchematic 中的
//! 面遍历。这一层把解析层、模型解析与几何串起来，产出光栅化的输入。

use crate::schematic::model::resolve_models;
use crate::block::db::{BlockDb, Face, NO_TEXTURE};
use crate::render::geometry::{build_face_corners, cull_neighbor_offset, face_shade, rotate_uv_corners};
use crate::render::transform::{rotate_cull_offset, to_world};
use crate::schematic::litematic::{BlockState, Schematic};

/// 判断一个方块状态是否「完全遮光」，即能否剔除相邻方块朝向它的面。
///
/// 判定条件刻意保守——只有普通完整不透明立方体才算：
///   - 恰好一个模型，且非半透明
///   - 恰好一个元素，无旋转、无 rescale
///   - 元素范围恰为 (0,0,0)-(16,16,16)
///   - 六个面齐全，且每面贴图都存在、都不含透明通道
///
/// 台阶、楼梯、玻璃、树叶等都不满足，因此不会错误地剔掉它们背后的面。
///
/// 取 `version_idx` 而不是 `&VersionDef`：面的贴图字段是**字符串表下标**，
/// 只有经 `BlockDb::texture_pool_index` 按版本解析后才能得到贴图池下标
/// （见 `ModelFace::texture` 的说明）。持有版本引用并不足以完成这层解析，
/// 会诱使调用者退化成把字符串下标当池下标用——那正是这里此前的缺陷：
/// 树叶被误判为遮光，邻面被错误剔除，渲染出现空洞。
pub fn is_opaque(db: &BlockDb, version_idx: usize, state: &BlockState) -> bool {
    let Some(version) = db.versions.get(version_idx) else {
        return false;
    };
    let models = resolve_models(db, version, state);
    if models.len() != 1 {
        return false;
    }
    let Some(model) = db.models.get(models[0].model_idx as usize) else {
        return false;
    };
    if model.translucent || model.elements.len() != 1 {
        return false;
    }
    let e = &model.elements[0];
    if e.has_rotation || e.rescale {
        return false;
    }
    if e.from != [0.0, 0.0, 0.0] || e.to != [16.0, 16.0, 16.0] {
        return false;
    }
    // 六个面齐全（位 0~5 全置）
    if e.face_mask != 0x3F {
        return false;
    }
    // 每个面都要有贴图，且贴图不含透明通道
    for f in &e.faces {
        if f.texture == NO_TEXTURE {
            return false;
        }
        // 先按版本把字符串表下标解析成贴图池下标，再取贴图属性
        let pool = db.texture_pool_index(version_idx, f.texture);
        match db.textures.get(pool as usize) {
            Some(t) if !t.has_alpha => {}
            _ => return false,
        }
    }
    true
}

/// 由方块状态与面朝向推导出的一个可见四边形
#[derive(Debug, Clone, PartialEq)]
pub struct Quad {
    /// 四个角点，已变换到世界坐标（单位为方块）。
    /// 变换顺序为元素旋转 → 变体旋转 → 除以 16 并平移到方块位置。
    pub corners: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    /// 贴图池下标（已由字符串表下标解析而来）
    pub texture: u32,
    /// 该面的明暗系数（已含世界法线方向光）
    pub shade: f32,
    /// 该面的染色系数（RGB，各 0..1）。无染色时为全 1。
    pub tint: [f32; 3],
    /// 该四边形所属方块在区域内的坐标
    pub block_pos: [i32; 3],
}

/// 渲染时是否跳过该方块（不产生任何面）。
///
/// 比统计口径多一个 structure_void：legacy-cpp 渲染时跳过它
/// （render.cpp:316-317），而统计把它算作非空气（litematic.cpp:374）。
/// 两处口径不同是有意的，改名以示区别，避免日后被「统一」掉。
pub(crate) fn is_render_skipped(name: &str) -> bool {
    matches!(
        name,
        "minecraft:air"
            | "minecraft:cave_air"
            | "minecraft:void_air"
            | "minecraft:structure_void"
    )
}

/// 收集整个投影的可见四边形。
///
/// 不透明判定按调色板索引缓存：同一区域内每种方块状态只计算一次，
/// 避免逐方块重复解析模型。
/// 收集可见四边形。
///
/// 收版本**下标**而非引用：面贴图在文件里是字符串表下标，必须按版本解析成
/// 贴图池下标，而这一步需要回到 db.versions 查该版本的路径映射。
pub fn collect_quads(sc: &Schematic, db: &BlockDb, ver_idx: usize) -> Vec<Quad> {
    let Some(version) = db.versions.get(ver_idx) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    for r in &sc.regions {
        // -1 未计算，0 非不透明，1 不透明
        let mut opaque_cache: Vec<i8> = vec![-1; r.palette.len()];

        for (i, pi) in r.indices.iter().enumerate() {
            let Some(state) = r.palette.get(*pi as usize) else { continue };
            if is_render_skipped(&state.name) {
                continue;
            }
            // 区域在投影中的位置必须计入，否则多区域投影的各区域会
            // 全部重叠在原点。
            // 注意：legacy-cpp 只用 PosFromIndex、从不加 r.pos
            // （render.cpp:323-326 是全仓库唯一一处取方块坐标的地方），
            // 因此多区域投影在旧版下会重叠。这是有意修正，单区域行为不变。
            //
            // 坐标用 checked_add：pos 与 local 都来自文件内容（Size 分量
            // 最大 i32::MAX），相加可能溢出——debug 构建会 panic，
            // release 会回绕成负坐标而把方块画到错误位置。
            // 溢出说明该方块落在可表示范围之外，跳过它而不是给出错误坐标。
            let local = r.pos_from_index(i);
            let (Some(bx), Some(by), Some(bz)) = (
                r.pos[0].checked_add(local[0]),
                r.pos[1].checked_add(local[1]),
                r.pos[2].checked_add(local[2]),
            ) else {
                continue;
            };
            let bp = [bx, by, bz];

            let models = resolve_models(db, version, state);
            for rm in &models {
                let Some(model) = db.models.get(rm.model_idx as usize) else { continue };
                for elem in &model.elements {
                    for face_idx in 0..6u8 {
                        let face = match face_idx {
                            0 => Face::Down,
                            1 => Face::Up,
                            2 => Face::North,
                            3 => Face::South,
                            4 => Face::West,
                            _ => Face::East,
                        };
                        if elem.face_mask & (1u8 << face_idx) == 0 {
                            continue;
                        }
                        let Some(mf) = elem.face(face) else { continue };

                        // 邻面剔除。cullface 为 0xFF 表示不参与剔除。
                        if mf.cullface != 0xFF {
                            // 剔除方向必须随变体旋转：方块转向后，它朝向某个
                            // 邻居的那一面也变了，否则会剔除错误的邻居。
                            let off = rotate_cull_offset(
                                cull_neighbor_offset(cullface_to_face(mf.cullface)),
                                rm.rot_x_div90,
                                rm.rot_y_div90,
                            );
                            let n = [bp[0] + off[0], bp[1] + off[1], bp[2] + off[2]];
                            if r.inside(n[0], n[1], n[2]) {
                                // 区域外视为空气，不剔除：否则结构的外表面会缺失
                                let ni = r.index(n[0], n[1], n[2]);
                                if let Some(npi) = r.indices.get(ni) {
                                    let idx = *npi as usize;
                                    if idx < opaque_cache.len() {
                                        if opaque_cache[idx] < 0 {
                                            // 注意：Rust 只为 u8 实现了 From<bool>，
                                            // i8::from(bool) 不存在，必须显式取值
                                            let opaque = r
                                                .palette
                                                .get(idx)
                                                .map_or(false, |s| is_opaque(db, ver_idx, s));
                                            opaque_cache[idx] = if opaque { 1 } else { 0 };
                                        }
                                        if opaque_cache[idx] == 1 {
                                            continue; // 被完全遮挡
                                        }
                                    }
                                }
                            }
                        }

                        // 面的 rotation 字段只影响贴图方向，不影响几何。
                        // 旋转的是「每个顶点取 uv 矩形的哪个角」，采样区域不变——
                        // 见 geometry::rotate_uv_corners 的说明（活塞臂就是被
                        // 绕贴图中心旋转害错的）。
                        let mut corners =
                            build_face_corners(face, elem.from, elem.to, mf.uv);
                        corners.uv = rotate_uv_corners(face, corners.uv, mf.rotation);
                        // 角点此刻仍在模型空间：既不随元素旋转、也不随变体
                        // 旋转、也没有平移到方块位置，尺度是方块坐标的 16 倍。
                        // 不在这里变换，所有方块的所有面会重叠在原点。
                        let world = std::array::from_fn(|k| {
                            to_world(
                                corners.pos[k],
                                elem,
                                rm.rot_x_div90,
                                rm.rot_y_div90,
                                bp,
                            )
                        });

                        out.push(Quad {
                            corners: world,
                            uv: corners.uv,
                            // 文件里的贴图字段是字符串表下标，必须按版本
                            // 解析成贴图池下标后才能给下游取像素
                            texture: db.texture_pool_index(ver_idx, mf.texture),
                            shade: shade_for(face, &world),
                            // 染色按方块名决定：同一个 tintindex 在草方块与
                            // 红石线上含义不同（见 tint 模块）
                            tint: crate::render::tint::tint_color(&state.name, &state.props, mf.tintindex),
                            block_pos: bp,
                        });
                    }
                }
            }
        }
    }
    out
}

/// 求一个面的明暗系数。
///
/// 只按该面在**世界空间**的朝向取固定系数，这正是原版的做法：
/// Minecraft 的方块光照不看真实光源方向，而是按面朝向给一个固定明暗
/// （顶面最亮、底面最暗、四个侧面居中且各不相同），因此同一朝向的面
/// 无论在哪都同样亮。
///
/// 之前这里还额外乘过一次 legacy-cpp 的「方向光」lambert，那是**错的**：
/// legacy 那套系数会让西面（0.886）比顶面（0.620）更亮、南面比顶面更亮，
/// 与游戏里「顶面最亮」的观感相反。用户要求「参考游戏内渲染方式」，
/// 因此改为完全按世界朝向取系数。
///
/// 「按世界朝向」这一点不能省：模型空间的面号在方块被变体旋转后
/// 已不对应实际朝向。例如活塞 `facing=up` 时绕 X 转 270°，模型的
/// 「顶面」在世界里朝下；若仍按模型面号取系数，会把朝下的面照成最亮。
fn shade_for(face: Face, corners: &[[f32; 3]; 4]) -> f32 {
    match face_normal(corners) {
        // 用法线所属的主轴方向反推它是哪个面，再取该面的系数
        Some(n) => face_shade(dominant_face(n)),
        // 退化面（面积为 0，本身不可见）退回按模型面号取
        None => face_shade(face),
    }
}

/// 由单位法线求它最接近的那个面。
///
/// 法线是轴向的（模型的面永远平行于坐标平面，旋转也只是 90° 的倍数），
/// 因此取绝对值最大的分量即可；平局时按 X→Y→Z 的固定顺序取值，
/// 保证结果确定。
fn dominant_face(n: [f32; 3]) -> Face {
    let ax = n[0].abs();
    let ay = n[1].abs();
    let az = n[2].abs();
    if ay >= ax && ay >= az {
        if n[1] >= 0.0 {
            Face::Up
        } else {
            Face::Down
        }
    } else if ax >= az {
        if n[0] >= 0.0 {
            Face::East
        } else {
            Face::West
        }
    } else if n[2] >= 0.0 {
        Face::South
    } else {
        Face::North
    }
}

/// 由四个角点求面的单位法线（**朝外**）。
///
/// 注意取三角形的顺序必须是 `0-2-1`，与 mesh::build 实际输出的绕序一致。
/// `build_face_corners` 给出的角点顺序 (0,1,2) 按右手法则算出来是**朝内**的
/// （这正是 mesh 要把索引改成 0-2-1 / 0-3-2 的原因，见 render/mesh.rs 的说明）。
/// 起初这里按 (0,1,2) 算，得到的法线全部朝内，于是「顶面」被算成朝下：
/// 明暗全部反过来。测试 shading_follows_world_facing_and_keeps_up_brightest
/// 正是抓到了这一点。
///
/// 退化三角形（面积为 0）返回 None，由调用方退回按面号取系数——
/// 这类面本身不可见，不值得为它编一个方向。
fn face_normal(corners: &[[f32; 3]; 4]) -> Option<[f32; 3]> {
    let a = corners[0];
    let b = corners[2];
    let c = corners[1];
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 1e-9 {
        return None;
    }
    Some([n[0] / len, n[1] / len, n[2] / len])
}

fn cullface_to_face(v: u8) -> Face {    match v {
        0 => Face::Down,
        1 => Face::Up,
        2 => Face::North,
        3 => Face::South,
        4 => Face::West,
        _ => Face::East,
    }
}

#[cfg(test)]
#[path = "../tests/render/visibility.rs"]
mod tests;
