use super::*;

use std::collections::HashMap;
use crate::block::state::ModelFace as DefFace;
use crate::block::state_chain::TexValue;

fn def_face(texture: &str, cullface: &str, uv: Option<[f32; 4]>) -> DefFace {
    DefFace {
        texture: texture.to_string(),
        cullface: cullface.to_string(),
        tintindex: -1,
        rotation: 0,
        uv,
    }
}

fn def_elem(faces: Vec<(&str, DefFace)>, from: [f32; 3], to: [f32; 3]) -> ModelElementDef {
    ModelElementDef {
        from,
        to,
        faces: faces.into_iter().map(|(n, f)| (n.to_string(), f)).collect(),
        rotation: None,
    }
}

fn chain_with(pairs: &[(&str, &str)]) -> Chain {
    let mut textures = HashMap::new();
    for (k, v) in pairs {
        textures.insert(
            k.to_string(),
            TexValue { sprite: v.to_string(), force_translucent: false },
        );
    }
    Chain { textures, elements: Vec::new(), force_translucent: false }
}

// ---- 面名与序号 ----

#[test]
fn face_names_map_to_indices_in_order() {
    for (i, n) in FACE_NAMES.iter().enumerate() {
        assert_eq!(face_index(n), Some(i as u8), "{n} 的序号应为 {i}");
    }
    assert_eq!(face_index("nope"), None);
    assert_eq!(face_index(""), None);
}

// ---- 缺省 UV ----

#[test]
fn default_uv_matches_facebakery_for_cube() {
    // 一个 0..16 的整方块，六面的缺省 UV 都应覆盖整个 0..16
    let from = [0.0, 0.0, 0.0];
    let to = [16.0, 16.0, 16.0];
    for face in 0..6u8 {
        let uv = default_uv(face, from, to);
        assert_eq!(uv, [0.0, 0.0, 16.0, 16.0], "面 {face} 的缺省 UV 应为整块");
    }
}

#[test]
fn default_uv_follows_axis_direction_per_face() {
    // 部分元素：可见各面的 u/v 取值方向
    let from = [2.0, 3.0, 5.0];
    let to = [8.0, 9.0, 11.0];

    // down：u 随 +x（2..8），v 随 -z（16-11=5 .. 16-5=11）
    assert_eq!(default_uv(0, from, to), [2.0, 5.0, 8.0, 11.0]);
    // up：u 随 +x，v 随 +z（5..11）
    assert_eq!(default_uv(1, from, to), [2.0, 5.0, 8.0, 11.0]);
    // north：u 随 -x（16-8=8 .. 16-2=14），v 随 -y（16-9=7 .. 16-3=13）
    assert_eq!(default_uv(2, from, to), [8.0, 7.0, 14.0, 13.0]);
    // south：u 随 +x，v 随 -y
    assert_eq!(default_uv(3, from, to), [2.0, 7.0, 8.0, 13.0]);
    // west：u 随 +z，v 随 -y
    assert_eq!(default_uv(4, from, to), [5.0, 7.0, 11.0, 13.0]);
    // east：u 随 -z（16-11=5 .. 16-5=11），v 随 -y
    assert_eq!(default_uv(5, from, to), [5.0, 7.0, 11.0, 13.0]);
}

// ---- 元素转换 ----

#[test]
fn explicit_uv_is_kept() {
    let e = def_elem(
        vec![("north", def_face("#all", "", Some([1.0, 2.0, 3.0, 4.0])))],
        [0.0; 3],
        [16.0; 3],
    );
    let ch = chain_with(&[("all", "minecraft:block/stone")]);
    let pool = HashMap::from([("minecraft:block/stone".to_string(), 7u32)]);
    let (el, used, _) = convert_element(&e, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .expect("应转换成功");
    assert_eq!(el.faces[0].uv, [1.0, 2.0, 3.0, 4.0], "显式 UV 不应被覆盖");
    assert_eq!(used, vec!["minecraft:block/stone"]);
}

#[test]
fn missing_uv_falls_back_to_default() {
    let e = def_elem(
        vec![("up", def_face("#all", "", None))],
        [1.0, 2.0, 3.0],
        [5.0, 6.0, 7.0],
    );
    let ch = chain_with(&[("all", "minecraft:block/x")]);
    let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
    let (el, _, _) = convert_element(&e, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .unwrap();
    assert_eq!(el.faces[0].uv, default_uv(1, [1.0, 2.0, 3.0], [5.0, 6.0, 7.0]));
}

#[test]
fn cullface_name_becomes_index_and_empty_is_no_cull() {
    let e = def_elem(
        vec![
            ("north", def_face("#all", "north", None)),
            ("south", def_face("#all", "", None)),
        ],
        [0.0; 3],
        [16.0; 3],
    );
    let ch = chain_with(&[("all", "minecraft:block/x")]);
    let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
    let (el, _, _) = convert_element(&e, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .unwrap();
    // 面按序存放：north(2) 在前、south(3) 在后
    assert_eq!(el.face_mask, 0b00001100);
    assert_eq!(el.faces.len(), 2);
    assert_eq!(el.faces[0].cullface, 2, "north 应映射为 2");
    assert_eq!(el.faces[1].cullface, 0xFF, "空 cullface 表示不剔除");
}

#[test]
fn faces_are_ordered_by_face_index_not_input_order() {
    // 输入顺序打乱，输出必须仍按面序排列——读取端按 faceMask 的位序取面
    let e = def_elem(
        vec![
            ("east", def_face("#all", "", None)),
            ("down", def_face("#all", "", None)),
            ("up", def_face("#all", "", None)),
        ],
        [0.0; 3],
        [16.0; 3],
    );
    let ch = chain_with(&[("all", "minecraft:block/x")]);
    let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
    let (el, _, _) = convert_element(&e, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .unwrap();
    // down=0、up=1、east=5 -> mask 位 0、1、5
    assert_eq!(el.face_mask, 0b0010_0011);
    assert_eq!(el.faces.len(), 3, "面数应与置位数一致");
}

#[test]
fn unresolvable_texture_makes_face_skipped() {
    // 引用不存在的变量：该面跳过，其余面保留
    let e = def_elem(
        vec![
            ("north", def_face("#missing", "", None)),
            ("up", def_face("#all", "", None)),
        ],
        [0.0; 3],
        [16.0; 3],
    );
    let ch = chain_with(&[("all", "minecraft:block/x")]);
    let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
    let (el, used, _) = convert_element(&e, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .unwrap();
    assert_eq!(el.faces.len(), 1, "无法解析的面应被跳过");
    assert_eq!(el.face_mask, 0b000010, "只剩 up 面");
    assert_eq!(used, vec!["minecraft:block/x"]);
}

#[test]
fn element_with_all_faces_unresolvable_is_dropped() {
    let e = def_elem(
        vec![("north", def_face("#missing", "", None))],
        [0.0; 3],
        [16.0; 3],
    );
    let ch = chain_with(&[]);
    let pool: HashMap<String, u32> = HashMap::new();
    assert!(convert_element(&e, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .is_none());
}

#[test]
fn unknown_face_names_are_ignored() {
    let e = def_elem(
        vec![
            ("custom_face", def_face("#all", "", None)),
            ("up", def_face("#all", "", None)),
        ],
        [0.0; 3],
        [16.0; 3],
    );
    let ch = chain_with(&[("all", "minecraft:block/x")]);
    let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
    let (el, _, _) = convert_element(&e, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .unwrap();
    assert_eq!(el.faces.len(), 1);
    assert_eq!(el.face_mask, 0b000010, "只应保留 up");
}

#[test]
fn element_rotation_is_carried_over() {
    let mut e = def_elem(
        vec![("up", def_face("#all", "", None))],
        [0.0; 3],
        [16.0; 3],
    );
    e.rotation = Some(crate::block::state::ElementRotation {
        origin: [8.0, 8.0, 8.0],
        axis: "x".to_string(),
        angle: 45.0,
        rescale: true,
        rot_vec: None,
    });
    let ch = chain_with(&[("all", "minecraft:block/x")]);
    let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
    let (el, _, _) = convert_element(&e, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .unwrap();
    assert!(el.has_rotation);
    assert_eq!(el.rot_kind, 0, "单轴旋转 rot_kind 应为 0");
    assert_eq!(el.axis, 0, "axis=x 应映射为 0");
    assert_eq!(el.angle, 45.0);
    assert!(el.rescale);
    assert_eq!(el.origin, [8.0, 8.0, 8.0]);
}

#[test]
fn three_axis_rotation_sets_kind_and_vector() {
    let mut e = def_elem(
        vec![("up", def_face("#all", "", None))],
        [0.0; 3],
        [16.0; 3],
    );
    e.rotation = Some(crate::block::state::ElementRotation {
        origin: [0.0; 3],
        axis: "y".to_string(),
        angle: 0.0,
        rescale: false,
        rot_vec: Some([90.0, 0.0, 45.0]),
    });
    let ch = chain_with(&[("all", "minecraft:block/x")]);
    let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
    let (el, _, _) = convert_element(&e, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .unwrap();
    assert_eq!(el.rot_kind, 1, "多轴旋转 rot_kind 应为 1");
    assert_eq!(el.rot_vec, [90.0, 0.0, 45.0]);
}

// ---- 模型转换 ----

#[test]
fn convert_model_collects_textures_and_dedups() {
    let def = ModelDef {
        parent: String::new(),
        textures: vec![("all".into(), "#base".into(), false)],
        elements: vec![
            def_elem(vec![("up", def_face("#all", "", None))], [0.0; 3], [16.0; 3]),
            def_elem(vec![("down", def_face("#all", "", None))], [0.0; 3], [16.0; 3]),
        ],
        ambientocclusion: true,
    };
    // #all -> #base -> 实际路径
    let ch = chain_with(&[("all", "#base"), ("base", "minecraft:block/x")]);
    let pool = HashMap::from([("minecraft:block/x".to_string(), 3u32)]);
    let cm = convert_model(&def, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .unwrap();
    assert_eq!(cm.model.elements.len(), 2);
    assert_eq!(
        cm.used_textures,
        vec!["minecraft:block/x"],
        "两个面用同一贴图时应去重为一条"
    );
}

#[test]
fn convert_model_without_elements_is_none() {
    let def = ModelDef {
        parent: "block/cube".into(),
        textures: vec![],
        elements: vec![],
        ambientocclusion: true,
    };
    let ch = chain_with(&[]);
    let pool: HashMap<String, u32> = HashMap::new();
    assert!(
        convert_model(&def, &ch, |raw| {
            resolve_face_texture(&ch, raw, "minecraft", &pool)
        })
        .is_none(),
        "没有元素的模型不应产出（写入后不可见）"
    );
}

#[test]
fn chain_elements_take_precedence_over_def() {
    // chain 里有 elements 时以它为准（父链已合并）
    let def = ModelDef {
        parent: String::new(),
        textures: vec![],
        elements: vec![def_elem(
            vec![("up", def_face("#missing", "", None))],
            [0.0; 3],
            [16.0; 3],
        )],
        ambientocclusion: true,
    };
    let mut ch = chain_with(&[("all", "minecraft:block/x")]);
    ch.elements = vec![def_elem(
        vec![("up", def_face("#all", "", None))],
        [0.0; 3],
        [16.0; 3],
    )];
    let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
    let cm = convert_model(&def, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .unwrap();
    assert_eq!(cm.model.elements.len(), 1, "应使用 chain 里的 elements");
}

/// 链级的 force_translucent 必须传到写出的模型上。
///
/// 这条覆盖的是一个曾经断掉的链路：标志由 build_chain 累积，但
/// convert_model 原先从调用方参数取值，而唯一的产品调用点硬编码
/// false，于是「玻璃不当成不透明」这个特性实际不可达。
#[test]
fn chain_translucent_flag_reaches_the_model() {
    let def = ModelDef {
        parent: String::new(),
        textures: vec![],
        elements: vec![def_elem(
            vec![("up", def_face("#all", "", None))],
            [0.0; 3],
            [16.0; 3],
        )],
        ambientocclusion: true,
    };
    let mut ch = chain_with(&[("all", "minecraft:block/x")]);
    ch.force_translucent = true;
    let pool = HashMap::from([("minecraft:block/x".to_string(), 0u32)]);
    let cm = convert_model(&def, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .unwrap();
    assert!(
        cm.model.translucent,
        "链上声明了强制半透明，写出的模型也必须是半透明"
    );
}

/// 面直接声明 force_translucent 时也要反映到模型上。
///
/// 这是 legacy-cpp 的第二条来源（packbuild.cpp:335 在每个面上检查
/// tv.forceTranslucent 并置 out.translucent）。只处理链级标志会漏掉它。
#[test]
fn face_level_translucent_flag_reaches_the_model() {
    let def = ModelDef {
        parent: String::new(),
        textures: vec![],
        elements: vec![def_elem(
            vec![("up", def_face("#all", "", None))],
            [0.0; 3],
            [16.0; 3],
        )],
        ambientocclusion: true,
    };
    // 链上变量自身带 force_translucent
    let mut ch = chain_with(&[("all", "#glass")]);
    ch.textures.insert(
        "glass".to_string(),
        crate::block::state_chain::TexValue {
            sprite: "minecraft:block/glass".to_string(),
            force_translucent: true,
        },
    );
    let pool = HashMap::from([("minecraft:block/glass".to_string(), 0u32)]);
    let cm = convert_model(&def, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .unwrap();
    assert!(
        cm.model.translucent,
        "沿变量链查到强制半透明时，模型也必须是半透明"
    );
}

/// 没有声明时不应误判为半透明。
///
/// 半透明会让渲染走另一条混合路径，误判会改变所有普通方块的观感。
#[test]
fn plain_model_is_not_translucent() {
    let def = ModelDef {
        parent: String::new(),
        textures: vec![],
        elements: vec![def_elem(
            vec![("up", def_face("#all", "", None))],
            [0.0; 3],
            [16.0; 3],
        )],
        ambientocclusion: true,
    };
    let ch = chain_with(&[("all", "minecraft:block/stone")]);
    let pool = HashMap::from([("minecraft:block/stone".to_string(), 0u32)]);
    let cm = convert_model(&def, &ch, |raw| {
        resolve_face_texture(&ch, raw, "minecraft", &pool)
    })
    .unwrap();
    assert!(!cm.model.translucent, "普通模型不应被判为半透明");
}

// ---- 贴图引用解析 ----

#[test]
fn resolve_face_texture_qualifies_and_looks_up_id() {
    let ch = chain_with(&[("all", "block/stone")]);
    let pool = HashMap::from([("minecraft:block/stone".to_string(), 9u32)]);
    let (idx, path) =
        resolve_face_texture(&ch, "#all", "minecraft", &pool).expect("应解析成功");
    assert_eq!(idx, 9);
    assert_eq!(path, "minecraft:block/stone");
}

#[test]
fn resolve_face_texture_keeps_explicit_namespace() {
    let ch = chain_with(&[("all", "mymod:block/x")]);
    let pool = HashMap::from([("mymod:block/x".to_string(), 2u32)]);
    let (idx, path) =
        resolve_face_texture(&ch, "#all", "minecraft", &pool).expect("应解析成功");
    assert_eq!(idx, 2);
    assert_eq!(path, "mymod:block/x", "已有命名空间不应被覆盖");
}

#[test]
fn resolve_face_texture_accepts_direct_path() {
    let ch = chain_with(&[]);
    let pool = HashMap::from([("mymod:block/x".to_string(), 4u32)]);
    let (idx, _) =
        resolve_face_texture(&ch, "mymod:block/x", "mymod", &pool).expect("应解析成功");
    assert_eq!(idx, 4);
}

#[test]
fn unresolvable_or_unregistered_texture_returns_none() {
    let ch = chain_with(&[]);
    let pool: HashMap<String, u32> = HashMap::new();
    // 变量缺失
    assert!(resolve_face_texture(&ch, "#nope", "minecraft", &pool).is_none());
    // 路径未登记到字符串池
    assert!(resolve_face_texture(&ch, "block/x", "minecraft", &pool).is_none());
}

// ---- resolve_face_texture_path：产品实际走的那条路径 ----
//
// 上面那组测试针对 resolve_face_texture，它多一层字符串池查表；
// 而 pack::build 与 collect_texture_paths 走的是不带池的
// resolve_face_texture_path。此前它只有间接覆盖，这里直接测。

#[test]
fn resolve_path_qualifies_bare_path_with_current_namespace() {
    let ch = chain_with(&[("all", "block/stone")]);
    assert_eq!(
        resolve_face_texture_path(&ch, "#all", "minecraft").as_deref(),
        Some("minecraft:block/stone"),
        "缺命名空间的路径应按当前命名空间补全"
    );
}

#[test]
fn resolve_path_keeps_explicit_namespace() {
    let ch = chain_with(&[("all", "mymod:block/x")]);
    assert_eq!(
        resolve_face_texture_path(&ch, "#all", "minecraft").as_deref(),
        Some("mymod:block/x"),
        "已有命名空间不应被当前命名空间覆盖"
    );
}

#[test]
fn resolve_path_accepts_direct_path_without_variable() {
    let ch = chain_with(&[]);
    assert_eq!(
        resolve_face_texture_path(&ch, "block/x", "minecraft").as_deref(),
        Some("minecraft:block/x"),
        "直接给路径时不需要变量表"
    );
}

/// 不需要字符串池：这是它与 resolve_face_texture 的关键区别，
/// 也是调用方能在拿到路径后再登记、从而打破循环的原因。
#[test]
fn resolve_path_does_not_require_pool_registration() {
    let ch = chain_with(&[("all", "block/never_registered")]);
    assert!(
        resolve_face_texture_path(&ch, "#all", "minecraft").is_some(),
        "未登记到字符串池的路径也应能解析出规范路径"
    );
}

#[test]
fn resolve_path_returns_none_for_missing_variable() {
    let ch = chain_with(&[]);
    assert!(
        resolve_face_texture_path(&ch, "#nope", "minecraft").is_none(),
        "变量缺失时应返回 None"
    );
}

/// 成环时必须终止并返回 None，而不是死循环或给出中间态。
#[test]
fn resolve_path_returns_none_for_cycle() {
    let mut ch = chain_with(&[]);
    ch.textures.insert(
        "a".into(),
        TexValue { sprite: "#b".into(), force_translucent: false },
    );
    ch.textures.insert(
        "b".into(),
        TexValue { sprite: "#a".into(), force_translucent: false },
    );
    assert!(
        resolve_face_texture_path(&ch, "#a", "minecraft").is_none(),
        "成环时应返回 None 而不是死循环"
    );
}

/// collect_texture_paths 是产品路径上的收集函数，直接测它。
#[test]
fn collect_texture_paths_dedups_and_sorts() {
    let ch = chain_with(&[("a", "block/z"), ("b", "block/a")]);
    let elements = vec![
        def_elem(
            vec![
                ("up", def_face("#a", "", None)),
                ("down", def_face("#b", "", None)),
                ("north", def_face("#a", "", None)),
            ],
            [0.0; 3],
            [16.0; 3],
        ),
    ];
    let paths = collect_texture_paths(&ch, &elements, "minecraft");
    assert_eq!(
        paths,
        vec!["minecraft:block/a".to_string(), "minecraft:block/z".to_string()],
        "应去重并按字典序排序"
    );
}

/// 解析面的贴图引用到「字符串池下标 + 规范路径」。
///
/// 返回 None 表示无法解析：变量缺失、成环、或路径未登记。
///
/// 解析部分（变量链、命名空间规范化）**委托给 `resolve_face_texture_path`**，
/// 只额外做一次字符串池查表。两份实现各写一遍时，测试覆盖的是测试专用的
/// 那一份，产品实际走的那份反而没有直接覆盖——这里保持单一实现。
pub fn resolve_face_texture(
    chain: &Chain,
    raw: &str,
    cur_ns: &str,
    string_pool: &HashMap<String, u32>,
) -> Option<(u32, String)> {
    let qualified = resolve_face_texture_path(chain, raw, cur_ns)?;
    let idx = string_pool.get(&qualified).copied()?;
    Some((idx, qualified))
}
