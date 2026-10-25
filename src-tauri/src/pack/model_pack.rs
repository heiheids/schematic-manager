//! 把解析后的模型定义转换成可写入资源包的形态。
//!
//! 用途：blockstate 里引用的是模型 id，模型定义里引用的是贴图变量；
//! 构建资源包时要先把这些引用解析成具体下标：
//!   - 模型：<ns>:<path> -> 模型池下标（按内容去重）
//!   - 贴图：`#var` 或路径 -> 贴图路径字符串表下标
//!   - 面的 cullface：面名字符串 -> 面序号（0..5），空串为不剔除
//!   - 面的 UV：缺省时按官方 FaceBakery 规则从元素边界推导
//!
//! 对照 legacy-cpp/packbuild.cpp 的 DefaultUv、FaceIndex 与 BuildModel 后半段。
//!
//! 纯逻辑：调用方提供「按 id 取模型链」与「按路径取贴图下标」的闭包，
//! 因此可完整测试。

use crate::block::db::{Model, ModelElement, ModelFace};
use crate::block::state::{ModelDef, ModelElementDef};
use crate::block::state_chain::{is_force_translucent, resolve_tex_ref, Chain};

/// 六个面名，下标即面序号（与 block::db::Face 一致）
pub const FACE_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// 面名对应的序号；未知面名返回 None
pub fn face_index(name: &str) -> Option<u8> {
    FACE_NAMES.iter().position(|n| *n == name).map(|i| i as u8)
}

/// 按官方规则计算缺省 UV（0..16 空间）。
///
/// 规则取自 Minecraft 的 FaceBakery，与 legacy-cpp 的 DefaultUv 一致。
/// 方向弄反会让贴图上下或左右颠倒——这类错误不影响几何，
/// 只让画面看着不对，所以按面逐一核对。
pub fn default_uv(face: u8, from: [f32; 3], to: [f32; 3]) -> [f32; 4] {
    let [x1, y1, z1] = from;
    let [x2, y2, z2] = to;
    match face {
        // down：u 随 +x，v 随 -z
        0 => [x1, 16.0 - z2, x2, 16.0 - z1],
        // up：u 随 +x，v 随 +z
        1 => [x1, z1, x2, z2],
        // north：u 随 -x，v 随 -y
        2 => [16.0 - x2, 16.0 - y2, 16.0 - x1, 16.0 - y1],
        // south：u 随 +x，v 随 -y
        3 => [x1, 16.0 - y2, x2, 16.0 - y1],
        // west：u 随 +z，v 随 -y
        4 => [z1, 16.0 - y2, z2, 16.0 - y1],
        // east：u 随 -z，v 随 -y
        _ => [16.0 - z2, 16.0 - y2, 16.0 - z1, 16.0 - y1],
    }
}

/// 转换结果
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertedModel {
    pub model: Model,
    /// 模型引用到的贴图路径（已解析、已规范），供调用方登记到贴图池
    pub used_textures: Vec<String>,
}

/// 把元素转成 pack 形态。
///
/// 返回 `(元素, 用到的贴图路径, 是否有面强制半透明)`。第三个分量用于让
/// `convert_model` 判定整个模型是否半透明——面直接声明 `force_translucent`
/// 时也要反映到模型上，否则玻璃一类方块会被当成不透明而剔除错面。
///
/// `tex_of` 把贴图引用解析成「字符串池下标」：
///   - 变量 `#x` 先沿链解析成路径，再规范成 `ns:path`，最后取字符串下标
///   - 直接给路径的同样规范化后取下标
///
/// 解析失败的面直接跳过而不是写一个占位下标：占位会让模型显示错贴图，
/// 而跳过只让该面缺失，更容易被发现。
pub fn convert_element<F>(
    e: &ModelElementDef,
    chain: &Chain,
    mut tex_of: F,
) -> Option<(ModelElement, Vec<String>, bool)>
where
    F: FnMut(&str) -> Option<(u32, String)>,
{
    if e.faces.is_empty() {
        return None;
    }

    let mut mask: u8 = 0;
    // 面必须按面序存放：读取端是按 faceMask 的位序依次取面的
    let mut by_index: Vec<Option<ModelFace>> = vec![None; 6];
    let mut used = Vec::new();
    let mut translucent = false;

    for (name, f) in &e.faces {
        let Some(idx) = face_index(name) else {
            continue; // 未知面名（例如 mod 自定义）直接忽略
        };
        let Some((tex_idx, path)) = tex_of(&f.texture) else {
            continue;
        };
        // 沿变量链判断是否强制半透明。结果必须向上传递而不是丢弃：
        // 丢弃会让「该面声明了强制半透明」这件事永远到不了写出的模型上。
        if is_force_translucent(&chain.textures, &f.texture) {
            translucent = true;
        }

        let uv = f
            .uv
            .unwrap_or_else(|| default_uv(idx, e.from, e.to));
        by_index[idx as usize] = Some(ModelFace {
            texture: tex_idx,
            uv,
            cullface: match face_index(&f.cullface) {
                Some(c) => c,
                None => 0xFF, // 空串或未知面名 = 不参与剔除
            },
            // tintindex 在文件里是 i16：-1 表示无染色。
            // 越界值直接按 -1 处理而不是截断——截断会让一个合法外的值
            // 变成某个看起来正常的染色下标，反而更难发现。
            tintindex: i16::try_from(f.tintindex).unwrap_or(-1),
            rotation: f.rotation,
        });
        mask |= 1u8 << idx;
        used.push(path);
    }

    if mask == 0 {
        return None; // 所有面都没解析出来
    }

    // 按面序压紧：只保留已置位的面
    let mut faces = Vec::new();
    for slot in by_index.into_iter() {
        if let Some(f) = slot {
            faces.push(f);
        }
    }
    if faces.len() != mask.count_ones() as usize {
        // 理论上到不了这里；到达说明上面的一致性被破坏，宁可跳过
        return None;
    }

    let (has_rotation, rot_kind, origin, axis, angle, rescale, rot_vec) = match &e.rotation {
        Some(r) => {
            let multi = r.rot_vec.is_some();
            (
                true,
                if multi { 1u8 } else { 0u8 },
                r.origin,
                match r.axis.as_str() {
                    "x" => 0u8,
                    "z" => 2,
                    _ => 1, // 缺省 y
                },
                r.angle,
                r.rescale,
                r.rot_vec.unwrap_or([0.0; 3]),
            )
        }
        None => (false, 0u8, [0.0; 3], 0u8, 0.0, false, [0.0; 3]),
    };

    Some((
        ModelElement {
            from: e.from,
            to: e.to,
            has_rotation,
            rot_kind,
            origin,
            axis,
            angle,
            rescale,
            rot_vec,
            face_mask: mask,
            faces,
        },
        used,
        translucent,
    ))
}

/// 把模型链（已合并 textures 与 elements）转成 pack 形态。
///
/// 半透明标志取自 `chain.force_translucent`——它由 build_chain 沿继承链
/// 累积（链上任意一级声明过即为真），也包含每个面直接声明的情况。
/// 此前该标志靠调用方传入，而唯一的调用点硬编码 false，导致
/// `force_translucent` 这个特性实际不可达：玻璃一类方块会被当成不透明。
///
/// 返回 None 表示没有任何可用元素——这种模型写入后不可见，
/// 不如让调用方按「模型缺失」处理并计入统计。
pub fn convert_model<F>(
    def: &ModelDef,
    chain: &Chain,
    mut tex_of: F,
) -> Option<ConvertedModel>
where
    F: FnMut(&str) -> Option<(u32, String)>,
{
    // elements 以链上最近一级为准；由调用方预先放进 chain
    let elements = if chain.elements.is_empty() {
        &def.elements
    } else {
        &chain.elements
    };
    if elements.is_empty() {
        return None;
    }

    let mut out_elements = Vec::new();
    let mut used = Vec::new();
    // 链级标志是起点；下面每个元素还会补充「该面直接声明」的情况
    let mut translucent = chain.force_translucent;
    for e in elements {
        if let Some((el, mut u, face_force)) = convert_element(e, chain, &mut tex_of) {
            translucent |= face_force;
            out_elements.push(el);
            used.append(&mut u);
        }
    }
    if out_elements.is_empty() {
        return None;
    }

    used.sort();
    used.dedup();
    Some(ConvertedModel {
        model: Model { translucent, elements: out_elements },
        used_textures: used,
    })
}

/// 收集一个模型链里全部面的贴图路径（去重、排序）。
///
/// 存在的理由：模型面里存的是**字符串表下标**，而该下标来自模型自己引用的
/// 路径，形成顺序依赖。因此分两遍：先用本函数收集全部路径并登记字符串池，
/// 再调用 convert_model（此时路径到下标已完整可查），避免边查边写的借用冲突。
pub fn collect_texture_paths(chain: &Chain, elements: &[crate::block::state::ModelElementDef], cur_ns: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in elements {
        for (_, f) in &e.faces {
            if let Some(p) = resolve_face_texture_path(chain, &f.texture, cur_ns) {
                out.push(p);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 把面的贴图引用解析成规范路径（只解析，不查表）。
///
/// 与 `resolve_face_texture` 的区别：这里不要求路径已登记到字符串池，
/// 因此调用方可以在拿到路径后再登记，避免「先有表还是先有路径」的循环。
pub fn resolve_face_texture_path(chain: &Chain, raw: &str, cur_ns: &str) -> Option<String> {
    let path = resolve_tex_ref(&chain.textures, raw)?;
    crate::pack::index::qualify(&path, cur_ns)
}

#[cfg(test)]
#[path = "../tests/pack/model_pack.rs"]
mod tests;
