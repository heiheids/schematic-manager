use super::*;
use crate::block::db::{Model, ModelElement, ModelFace, TextureInfo, VersionDef};

/// 造一个「完整不透明立方体」模型
fn full_cube(texture: u32, has_alpha: bool) -> (Model, TextureInfo) {
    let faces: Vec<ModelFace> = (0..6)
        .map(|_| ModelFace {
            texture,
            uv: [0.0, 0.0, 16.0, 16.0],
            cullface: 0xFF,
            tintindex: -1,
            rotation: 0,
        })
        .collect();
    (
        Model {
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
        },
        TextureInfo { w: 16, h: 16, offset: 0, len: 1024, has_alpha },
    )
}

fn build(texture_alpha: bool, mutate: impl FnOnce(&mut Model)) -> (BlockDb, VersionDef) {
    let (mut m, ti) = full_cube(0, texture_alpha);
    mutate(&mut m);
    let version = VersionDef {
        name: "1.20.1".into(),
        data_version: 3465,
        lang_idx: 0,
        // 真实数据里面的贴图字段是字符串表下标，须经此映射得到池下标
        texture_by_path: std::collections::HashMap::from([(
            "block/stone".to_string(),
            0u32,
        )]),
        blocks: vec![crate::block::db::BlockDef {
            name: "minecraft:stone".into(),
            variants: vec![crate::block::db::Variant {
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
    let db = BlockDb {
        strings: vec!["block/stone".to_string()],
        langs: Vec::new(),
        textures: vec![ti],
        models: vec![m],
        versions: vec![version.clone()],
        pixels: vec![0u8; 1024],
    };
    (db, version)
}

/// 明暗必须按**世界空间**的面朝向定，顶面永远最亮、底面永远最暗。
///
/// 这条守住两件事：
/// 1. 朝向顺序正确。曾经在面系数之上又乘了一个 legacy 的方向光
///    lambert，结果西面（0.780）比顶面（0.620）更亮、南面也比顶面亮，
///    与游戏里「顶面最亮」完全相反。
/// 2. 用的是**世界**朝向而不是模型面号。方块被变体旋转后，模型面号
///    已不对应实际朝向；活塞 facing=up 时模型的顶面在世界里朝下，
///    若按面号取系数就会把朝下的面照成最亮。
#[test]
fn shading_follows_world_facing_and_keeps_up_brightest() {
    use crate::render::geometry::build_face_corners;

    // 未旋转的完整立方块
    let from = [0.0f32, 0.0, 0.0];
    let to = [16.0f32, 16.0, 16.0];
    let uv = [0.0f32, 0.0, 16.0, 16.0];
    let all = [
        Face::Down,
        Face::Up,
        Face::North,
        Face::South,
        Face::West,
        Face::East,
    ];
    let mut shades: Vec<(Face, f32)> = Vec::new();
    for f in all {
        let c = build_face_corners(f, from, to, uv);
        shades.push((f, shade_for(f, &c.pos)));
    }
    let by = |f: Face| shades.iter().find(|(x, _)| *x == f).unwrap().1;

    // 顶面最亮、底面最暗
    for (f, s) in &shades {
        if *f != Face::Up {
            assert!(
                by(Face::Up) > *s,
                "顶面应比其他面都亮，但 {f:?} 是 {s}、顶面是 {}",
                by(Face::Up)
            );
        }
        if *f != Face::Down {
            assert!(
                by(Face::Down) < *s,
                "底面应比其他面都暗，但 {f:?} 是 {s}、底面是 {}",
                by(Face::Down)
            );
        }
    }

    // 旋转后仍按世界朝向取系数：把每个面用 rotate_variant 转 90°，
    // 再核对它的明暗等于「它转到的新朝向」在未旋转时的明暗。
    // 这样即使面号没变，只要朝向变了，明暗也必须跟着变。
    for (f, base) in &shades {
        for rx in 0..4u8 {
            for ry in 0..4u8 {
                let c = build_face_corners(*f, from, to, uv);
                let rotated: [[f32; 3]; 4] = std::array::from_fn(|k| {
                    crate::render::transform::rotate_variant(c.pos[k], rx, ry)
                });
                let got = shade_for(*f, &rotated);
                // 该面旋转后的实际朝向
                let want_face = dominant_face(face_normal(&rotated).expect("非退化面"));
                let want = by(want_face);
                assert!(
                    (got - want).abs() < 1e-6,
                    "rx={rx} ry={ry} 时 {f:?} 的明暗为 {got}，\
                     但它此刻朝向 {want_face:?}（应为 {want}）——\
                     说明明暗是按模型面号而非世界朝向取的"
                );
                let _ = base;
            }
        }
    }
}

/// dominant_face 对六个轴向法线都要给出对应面。
#[test]
fn dominant_face_maps_axis_normals() {
    assert_eq!(dominant_face([0.0, 1.0, 0.0]), Face::Up);
    assert_eq!(dominant_face([0.0, -1.0, 0.0]), Face::Down);
    assert_eq!(dominant_face([0.0, 0.0, -1.0]), Face::North);
    assert_eq!(dominant_face([0.0, 0.0, 1.0]), Face::South);
    assert_eq!(dominant_face([-1.0, 0.0, 0.0]), Face::West);
    assert_eq!(dominant_face([1.0, 0.0, 0.0]), Face::East);
}

/// 收集到的四边形必须已经落到该方块的世界坐标上。
///
/// 这条测试防的是「角点停留在模型空间」：若未做旋转与平移，
/// 所有方块的所有面会重叠在原点且尺度放大 16 倍，
/// 而只检查数量的测试完全发现不了。
#[test]
fn quads_are_transformed_to_block_world_position() {
    let (db, _v) = build(false, |_| {});
    let sc = Schematic {
        root: crate::format::nbt::Value::Compound(vec![]),
        regions: vec![crate::schematic::litematic::Region {
            name: "r".into(),
            pos: [3, 5, 7],
            size: [1, 1, 1],
            volume: 1,
            bits: 2,
            palette: vec![BlockState::new("minecraft:stone")],
            indices: vec![0],
            ..Default::default()
        }],
        path: String::new(),
        meta_modified: false,
        blocks_modified: false,
    };
    let quads = collect_quads(&sc, &db, 0);
    assert!(!quads.is_empty());

    // 区域位于 (3,5,7)，方块尺寸为 1，因此所有角点都应落在
    // [3,4] × [5,6] × [7,8] 之内。若未平移，坐标会落在 0..16。
    for q in &quads {
        for c in &q.corners {
            for (axis, v) in c.iter().enumerate() {
                let lo = [3.0, 5.0, 7.0][axis];
                let hi = lo + 1.0;
                assert!(
                    *v >= lo - 1e-4 && *v <= hi + 1e-4,
                    "角点 {c:?} 超出方块范围；轴 {axis} 应在 {lo}..{hi}，\
                     说明角点没有被变换到世界坐标"
                );
            }
        }
    }
}

/// 坐标相加溢出时必须跳过该方块，而不是 panic 或给出回绕的坐标。
///
/// pos 与 local 都来自文件内容：Size 分量最大 i32::MAX，pos 也可接近
/// 该值。原先直接相加，debug 构建（cargo test）会 panic
/// `attempt to add with overflow`，release 构建会回绕成负坐标，
/// 把方块画到完全错误的位置。
#[test]
fn overflowing_block_position_is_skipped_not_wrapped() {
    let (db, _v) = build(false, |_| {});
    let sc = Schematic {
        root: crate::format::nbt::Value::Compound(vec![]),
        regions: vec![crate::schematic::litematic::Region {
            name: "r".into(),
            // 原点取 i32::MAX：再加任何非零偏移都会溢出
            pos: [i32::MAX, i32::MAX, i32::MAX],
            // 尺寸 2 使 local 可以为 1，从而触发相加溢出
            size: [2, 2, 2],
            volume: 8,
            bits: 2,
            palette: vec![BlockState::new("minecraft:stone")],
            indices: vec![0; 8],
            ..Default::default()
        }],
        path: String::new(),
        meta_modified: false,
        blocks_modified: false,
    };
    // 不应 panic；可以输出为空（全部跳过），但绝不能带出错坐标
    let quads = collect_quads(&sc, &db, 0);
    for q in &quads {
        for c in &q.corners {
            for v in c {
                assert!(
                    v.is_finite(),
                    "角点坐标必须是有限值，实际 {c:?}"
                );
                assert!(
                    *v >= i32::MAX as f32 - 2.0,
                    "不应出现回绕后的负坐标，实际 {c:?}"
                );
            }
        }
    }
}

/// 面的贴图字段必须按版本解析成贴图池下标。
///
/// 两者数值空间无关：文件里的取值指向字符串表。若直接当池下标用，
/// 多数面会取不到贴图，少数会取到完全无关的贴图。
#[test]
fn face_texture_is_resolved_through_version_map() {
    // full_cube 的贴图字段为 1（字符串表下标），strings[1] 指向池下标 5
    let (mut m, ti) = full_cube(1, false);
    for f in m.elements[0].faces.iter_mut() {
        f.texture = 1;
    }
    let version = VersionDef {
        name: "1.20.1".into(),
        data_version: 3465,
        lang_idx: 0,
        texture_by_path: std::collections::HashMap::from([(
            "block/stone".to_string(),
            5u32,
        )]),
        blocks: vec![crate::block::db::BlockDef {
            name: "minecraft:stone".into(),
            variants: vec![crate::block::db::Variant {
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
    let mut textures = vec![ti.clone(); 6];
    textures[5] = ti;
    let db = BlockDb {
        // 下标 1 是路径，下标 0 留空以便越界时报错
        strings: vec![String::new(), "block/stone".to_string()],
        langs: Vec::new(),
        textures,
        models: vec![m],
        versions: vec![version],
        pixels: vec![0u8; 6 * 1024],
    };
    let sc = Schematic {
        root: crate::format::nbt::Value::Compound(vec![]),
        regions: vec![crate::schematic::litematic::Region {
            name: "r".into(),
            pos: [0, 0, 0],
            size: [1, 1, 1],
            volume: 1,
            bits: 2,
            palette: vec![BlockState::new("minecraft:stone")],
            indices: vec![0],
            ..Default::default()
        }],
        path: String::new(),
        meta_modified: false,
        blocks_modified: false,
    };
    let quads = collect_quads(&sc, &db, 0);
    assert!(!quads.is_empty());
    for q in &quads {
        assert_eq!(
            q.texture, 5,
            "贴图应为解析后的池下标 5，而不是字符串表下标 1\
             （若得到 1 说明未做解析，会取到完全无关的贴图）"
        );
    }
}

#[test]
fn plain_full_cube_is_opaque() {
    let (db, _v) = build(false, |_| {});
    assert!(is_opaque(&db, 0, &BlockState::new("minecraft:stone")));
}

/// 不透明判定必须**按版本解析**贴图字段，不能把字符串表下标当池下标用。
///
/// 面的贴图字段是字符串表下标（见 ModelFace::texture 的说明），两者数值
/// 空间无关：随包库里字符串表 6263 项、贴图池只有 1685 项，约一半的取值
/// 超出池长度。误用会让半透明方块被判成「完全遮光」，进而剔除邻面、
/// 渲染出洞。
///
/// 这里刻意构造「字符串下标 1 → 池下标 5」的错位，并让两个位置的
/// has_alpha 相反：只有走对解析路径才能得到「含透明 → 不遮光」的结论。
#[test]
fn opacity_resolves_texture_through_version_not_as_pool_index() {
    // 池下标 5 的贴图含透明（树叶/玻璃），池下标 1 的不含。
    // 面的字段是字符串表下标 1，字符串表 [1] = "block/leaves" 映射到池下标 5。
    let (mut m, _) = full_cube(1, true);
    for f in m.elements[0].faces.iter_mut() {
        f.texture = 1;
    }
    let version = VersionDef {
        name: "1.20.1".into(),
        data_version: 3465,
        lang_idx: 0,
        texture_by_path: std::collections::HashMap::from([(
            "block/leaves".to_string(),
            5u32,
        )]),
        blocks: vec![crate::block::db::BlockDef {
            name: "minecraft:oak_leaves".into(),
            variants: vec![crate::block::db::Variant {
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
    // 池下标 1 不透明，池下标 5 含透明
    let opaque_tex = TextureInfo { w: 16, h: 16, offset: 0, len: 1024, has_alpha: false };
    let alpha_tex = TextureInfo { w: 16, h: 16, offset: 0, len: 1024, has_alpha: true };
    let mut textures = vec![opaque_tex; 6];
    textures[1] = opaque_tex;
    textures[5] = alpha_tex;
    let db = BlockDb {
        strings: vec![String::new(), "block/leaves".to_string()],
        langs: Vec::new(),
        textures,
        models: vec![m],
        versions: vec![version.clone()],
        pixels: vec![0u8; 6 * 1024],
    };

    assert!(
        !is_opaque(&db, 0, &BlockState::new("minecraft:oak_leaves")),
        "贴图字段是字符串表下标，必须按版本解析到含透明的池下标 5；\
         直接拿 1 当池下标会读到不透明贴图，从而错误地判定遮光并剔除邻面"
    );
}

#[test]
fn textured_transparency_is_not_opaque() {
    // 贴图含透明通道（玻璃、树叶）不能剔除邻居
    let (db, _v) = build(true, |_| {});
    assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));
}

#[test]
fn partial_and_rotated_elements_are_not_opaque() {
    // 非完整立方体（台阶）
    let (db, _v) = build(false, |m| m.elements[0].to[1] = 8.0);
    assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));

    // 带旋转
    let (db, _v) = build(false, |m| m.elements[0].has_rotation = true);
    assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));

    // 带 rescale
    let (db, _v) = build(false, |m| m.elements[0].rescale = true);
    assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));
}

#[test]
fn missing_faces_are_not_opaque() {
    // 缺一个面（例如只有五个面的方块）不能剔除邻居
    let (db, _v) = build(false, |m| m.elements[0].face_mask = 0x1F);
    assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));
}

#[test]
fn translucent_model_is_not_opaque() {
    let (db, _v) = build(false, |m| m.translucent = true);
    assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));
}

#[test]
fn missing_texture_is_not_opaque() {
    let (db, _v) = build(false, |m| {
        for f in m.elements[0].faces.iter_mut() {
            f.texture = NO_TEXTURE;
        }
    });
    assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:stone")));
}

#[test]
fn unknown_block_is_not_opaque() {
    let (db, _v) = build(false, |_| {});
    assert!(!is_opaque(&db, 0, &BlockState::new("mod:unknown")));
    assert!(!is_opaque(&db, 0, &BlockState::new("minecraft:air")));
}

#[test]
fn air_blocks_produce_no_quads() {
    // 版本由 db 自己持有，这里只传下标；测试库只有一个版本
    let (db, _v) = build(false, |_| {});
    let sc = Schematic {
        root: crate::format::nbt::Value::Compound(vec![]),
        regions: vec![crate::schematic::litematic::Region {
            name: "r".into(),
            pos: [0, 0, 0],
            size: [1, 1, 1],
            volume: 1,
            bits: 2,
            palette: vec![BlockState::new("minecraft:air")],
            indices: vec![0],
            ..Default::default()
        }],
        path: String::new(),
        meta_modified: false,
        blocks_modified: false,
    };
    assert!(collect_quads(&sc, &db, 0).is_empty(), "空气不应产生任何面");
}
