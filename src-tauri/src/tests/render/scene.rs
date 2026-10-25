use super::*;
use crate::block::db::NO_TEXTURE;
use crate::block::db::{BlockDef, Model, ModelElement, ModelFace, Variant, VersionDef};
use crate::schematic::litematic::{BlockState, Region};
use std::collections::HashMap;

/// 透明类型必须把「只有 0/255」与「含中间值」分开。
///
/// 这个区分不是分类爱好：前端对 cutout 用 alphaTest（丢弃全透明像素、
/// 其余照常写深度），对 translucent 用混合且不写深度。
/// 若把染色玻璃误判成 cutout，它的半透明像素会被 alphaTest 挖成窟窿；
/// 若把树叶误判成 translucent，叶子之间会互相透视。
#[test]
fn classify_alpha_separates_cutout_from_translucent() {
    // 全不透明
    let opaque: Vec<u8> = (0..4).flat_map(|_| [10u8, 20, 30, 255]).collect();
    assert_eq!(classify_alpha(&opaque), AlphaMode::Opaque);

    // 只有 0 与 255：树叶/草这类镂空贴图
    let cutout = vec![10, 20, 30, 255, 10, 20, 30, 0, 40, 50, 60, 255];
    assert_eq!(classify_alpha(&cutout), AlphaMode::Cutout);

    // 有 1..254 之间的值：染色玻璃/水
    let translucent = vec![10, 20, 30, 255, 10, 20, 30, 163, 10, 20, 30, 0];
    assert_eq!(classify_alpha(&translucent), AlphaMode::Translucent);

    // 全透明：没有可见像素，但也不是半透明
    let all_clear = vec![0u8; 8];
    assert_eq!(classify_alpha(&all_clear), AlphaMode::Cutout);
}

/// 只要**存在一个**半透明像素就必须判成 translucent。
///
/// 判据不能按比例写：染色玻璃里半透明像素是少数（191/256），
/// 按「多数」判断会漏判，玻璃照样被挖窟窿。
#[test]
fn a_single_translucent_pixel_is_enough() {
    let mut px = vec![255u8; 16];
    px[3] = 1; // 唯一一个半透明像素
    assert_eq!(
        classify_alpha(&px),
        AlphaMode::Translucent,
        "哪怕只有一个半透明像素也必须走混合，否则它会被 alphaTest 丢掉"
    );
}

/// 真实染色玻璃的 alpha 分布必须判成 translucent。
#[test]
fn stained_glass_like_pixels_are_translucent() {
    // 16x16 里 191 个 alpha=163、其余 65 个 alpha=255，
    // 与基础库 block/white_stained_glass 的实测分布一致
    let mut px = Vec::with_capacity(16 * 16 * 4);
    for i in 0..16 * 16 {
        let a = if i < 191 { 163u8 } else { 255u8 };
        px.extend_from_slice(&[200, 200, 200, a]);
    }
    assert_eq!(classify_alpha(&px), AlphaMode::Translucent);
}

/// 批次带上的透明类型，必须与它自己的贴图像素一致。
///
/// 断言「等于 classify_alpha(自己的像素)」而不是写死某个值：
/// 写死值只是在核对测试夹具的内容，而这里要守的是**传递关系**——
/// 组装时把透明类型算错或传丢，前端就会用错绘制方式。
#[test]
fn batch_alpha_mode_matches_its_own_pixels() {
    let db = test_db();
    let sc = test_schematic();
    let data = build_from(&sc, &db).expect("build 失败");
    assert!(!data.batches.is_empty(), "样本应产出至少一个批次");
    for b in &data.batches {
        assert_eq!(
            b.alpha_mode,
            classify_alpha(&b.texture_rgba),
            "批次的透明类型与它自己的贴图像素不一致"
        );
    }
}

/// 造一个方块库：单个版本，一个完整不透明立方体方块
fn test_db() -> BlockDb {
    // cullface 填对应面序号（0..5），与真实模型一致；
    // 填 0xFF 表示不参与剔除，那样相邻方块之间不会互相隐藏面。
    let faces: Vec<ModelFace> = (0..6u8)
        .map(|i| ModelFace {
            // 0 是字符串表下标而非贴图池下标：
            // 文件里该字段指向贴图路径，必须经版本映射才能得到池下标。
            // 测试库故意让两者不同（strings[0] 对应池下标 1），
            // 否则"当成池下标直接用"这个缺陷在测试中不会暴露。
            texture: 0,
            uv: [0.0, 0.0, 16.0, 16.0],
            cullface: i,
            tintindex: -1,
            rotation: 0,
        })
        .collect();
    let model = Model {
        translucent: false,
        elements: vec![ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [16.0, 16.0, 16.0],
            has_rotation: false,
            rot_kind: 0,
            origin: [0.0; 3],
            axis: 0,
            angle: 0.0,
            rescale: false,
            rot_vec: [0.0; 3],
            face_mask: 0x3F,
            faces,
        }],
    };
    let version = VersionDef {
        name: "1.20.1".into(),
        data_version: 3465,
        lang_idx: 0,
        texture_by_path: HashMap::from([("block/test".to_string(), 1u32)]),
        blocks: vec![BlockDef {
            name: "minecraft:stone".into(),
            variants: vec![Variant {
                when: String::new(),
                model_idx: 0,
                rot_x_div90: 0,
                rot_y_div90: 0,
                uvlock: false,
            }],
            multipart: Vec::new(),
        }],
        langs: Vec::new(),
        is_overlay: false,
    };
    BlockDb {
        strings: vec!["block/test".to_string()],
        langs: Vec::new(),
        // 池下标 1 才是该方块要用的贴图；池下标 0 是干扰项，
        // 若把字符串下标当池下标用就会取到它
        textures: vec![
            TextureInfo { w: 2, h: 2, offset: 0, len: 16, has_alpha: false },
            TextureInfo { w: 2, h: 2, offset: 0, len: 16, has_alpha: false },
        ],
        models: vec![model],
        versions: vec![version],
        pixels: (0..16).collect(),
    }
}

/// 一个 2x1x1 的投影，全部为石头（非空气，会互相剔除部分面）
fn test_schematic() -> Schematic {
    Schematic {
        root: crate::format::nbt::Value::Compound(vec![
            // 缺这一项时 summary 的 data_version 为 0，选版本会落到别的版本
            ("MinecraftDataVersion".into(), crate::format::nbt::Value::Int(3465)),
            (
            "Metadata".into(),
            crate::format::nbt::Value::Compound(vec![
                ("Name".into(), crate::format::nbt::Value::String("测试".into())),
                ("EnclosingSize".into(), crate::format::nbt::Value::Compound(vec![
                    ("x".into(), crate::format::nbt::Value::Int(2)),
                    ("y".into(), crate::format::nbt::Value::Int(1)),
                    ("z".into(), crate::format::nbt::Value::Int(1)),
                ])),
            ]),
            ),
        ]),
        regions: vec![Region {
            name: "r".into(),
            pos: [0, 0, 0],
            size: [2, 1, 1],
            volume: 2,
            bits: 2,
            palette: vec![BlockState::new("minecraft:stone")],
            indices: vec![0, 0],
            ..Default::default()
        }],
        path: String::new(),
        meta_modified: false,
        blocks_modified: false,
    }
}

#[test]
fn builds_batches_with_texture_pixels() {
    let sc = test_schematic();
    let db = test_db();
    let scene = build_from(&sc, &db).expect("组装失败");

    assert!(!scene.is_empty(), "应产出至少一个批次");
    assert_eq!(scene.batches.len(), 1);
    let b = &scene.batches[0];
    // 面的贴图字段是字符串表下标 0，经版本映射后应得到贴图池下标 1。
    // 若直接把字符串下标当池下标用，这里会得到 0 并失败——
    // 这正是要防的缺陷：取到的是完全无关的贴图。
    assert_eq!(b.texture, 1, "字符串表下标必须解析成贴图池下标");
    assert_eq!(b.texture_width, 2);
    assert_eq!(b.texture_height, 2);
    assert_eq!(b.texture_rgba.len(), 2 * 2 * 4, "贴图像素长度应为 宽*高*4");
    assert_eq!(b.texture_rgba, (0..16).collect::<Vec<u8>>());
    assert_eq!(scene.total_vertices, b.positions.len() / 3);
    assert_eq!(scene.total_triangles, b.indices.len() / 3);
}

#[test]
fn enclosing_and_origin_are_reported() {
    let sc = test_schematic();
    let db = test_db();
    let scene = build_from(&sc, &db).unwrap();
    assert_eq!(scene.enclosing, [2, 1, 1]);
    assert_eq!(scene.origin, [0, 0, 0]);
    assert_eq!(scene.data_version, 3465);
}

#[test]
fn region_scene_uses_only_the_selected_region() {
    let mut sc = test_schematic();
    sc.regions[0].name = "selected".into();
    sc.regions[0].pos = [7, -3, 2];
    sc.regions[0].size = [1, 1, 1];
    sc.regions[0].volume = 1;
    sc.regions[0].indices = vec![0];
    sc.regions.push(Region {
        name: "other".into(),
        pos: [-30, -20, -10],
        size: [4, 4, 4],
        volume: 64,
        bits: 2,
        palette: vec![BlockState::new("mod:unknown")],
        indices: vec![0; 64],
        ..Default::default()
    });
    let db = test_db();

    let full = build_from(&sc, &db).unwrap();
    assert_eq!(full.unresolved_blocks, 1);
    let scene = build_from_region(&sc, &db, "selected").unwrap();
    assert_eq!(scene.origin, [7, -3, 2]);
    assert_eq!(scene.enclosing, [1, 1, 1], "单区外框应取区域尺寸而非全投影元数据");
    assert_eq!(scene.unresolved_blocks, 0, "其他区域的未知方块不应计入单区统计");
    assert_eq!(scene.total_triangles / 2, 6, "单方块区域的六个边界面都应保留");
}

#[test]
fn unknown_region_name_is_an_error() {
    let sc = test_schematic();
    let db = test_db();
    let err = build_from_region(&sc, &db, "missing").unwrap_err().to_string();
    assert!(err.contains("missing"), "错误应指出未找到的区域：{err}");
    assert!(err.contains("区域"), "错误应说明失败对象：{err}");
}

#[test]
fn adjacent_blocks_hide_shared_faces() {
    // 两个相邻的完整立方体：贴合的那两个面应被剔除，
    // 只留下 2 个方块 × 6 面 - 2 个共享面 × 2 = 10 个面
    let sc = test_schematic();
    let db = test_db();
    let scene = build_from(&sc, &db).unwrap();
    let faces = scene.total_triangles / 2;
    assert_eq!(faces, 10, "相邻面应被剔除，实际 {faces} 个面");
}

#[test]
fn unknown_block_is_counted_as_unresolved() {
    let mut sc = test_schematic();
    sc.regions[0].palette = vec![BlockState::new("mod:unknown")];
    let db = test_db();
    let scene = build_from(&sc, &db).unwrap();
    assert_eq!(scene.unresolved_blocks, 1, "查不到定义的方块应被统计");
    assert!(scene.is_empty(), "没有模型就不应产出批次");
}

#[test]
fn missing_texture_pixels_skips_batch() {
    let mut db = test_db();
    // 声明长度为 16 但像素池只有 4 字节，数据不足
    db.textures[0].len = 16;
    db.pixels = vec![0u8; 4];
    let sc = test_schematic();
    let scene = build_from(&sc, &db).unwrap();
    assert!(scene.is_empty(), "贴图数据不足时应跳过该批次而不是画错色");
}

#[test]
fn air_only_schematic_yields_empty_scene() {
    let mut sc = test_schematic();
    sc.regions[0].palette = vec![BlockState::new("minecraft:air")];
    let db = test_db();
    let scene = build_from(&sc, &db).unwrap();
    assert!(scene.is_empty());
    assert_eq!(scene.total_triangles, 0);
}

#[test]
fn no_available_version_is_an_error() {
    let mut db = test_db();
    db.versions.clear();
    let sc = test_schematic();
    let e = build_from(&sc, &db).unwrap_err().to_string();
    assert!(e.contains("版本"), "错误信息应说明缺少可用版本：{e}");
}

#[test]
fn sentinel_texture_produces_no_batch() {
    let mut db = test_db();
    for f in db.models[0].elements[0].faces.iter_mut() {
        f.texture = NO_TEXTURE;
    }
    let sc = test_schematic();
    let scene = build_from(&sc, &db).unwrap();
    assert!(scene.is_empty(), "哨兵贴图不应产生批次");
}

impl SceneData {
    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }
}
