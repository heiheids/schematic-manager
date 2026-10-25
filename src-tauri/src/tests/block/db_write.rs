use super::*;
use crate::block::db::{BlockDef, ModelFace, TextureInfo, Variant, VersionDef};

fn face(texture: u32) -> ModelFace {
    ModelFace {
        texture,
        uv: [0.0, 0.0, 16.0, 16.0],
        cullface: 0xFF,
        tintindex: -1,
        rotation: 0,
    }
}

fn element(faces: Vec<ModelFace>, mask: u8) -> ModelElement {
    ModelElement {
        from: [0.0, 0.0, 0.0],
        to: [16.0, 16.0, 16.0],
        has_rotation: false,
        rot_kind: 0,
        origin: [0.0; 3],
        axis: 0,
        angle: 0.0,
        rescale: false,
        rot_vec: [0.0; 3],
        face_mask: mask,
        faces,
    }
}

/// 造一个内容较全的方块库：字符串表、方块、变体、multipart、模型、贴图
fn sample_db() -> BlockDb {
    let strings = vec![
        "1.20.1".to_string(),
        "minecraft:stone".to_string(),
        "block/stone".to_string(),
        "facing=north".to_string(),
        "Stone".to_string(),
        "石头".to_string(),
    ];
    let model = Model {
        translucent: false,
        elements: vec![
            element(vec![face(2), face(2)], 0b000011),
            element(vec![face(2)], 0b100000),
        ],
    };
    let version = VersionDef {
        name: "1.20.1".into(),
        data_version: 3465,
        lang_idx: 0,
        texture_by_path: HashMap::from([("block/stone".to_string(), 0u32)]),
        blocks: vec![BlockDef {
            name: "minecraft:stone".into(),
            variants: vec![Variant {
                when: String::new(),
                model_idx: 0,
                rot_x_div90: 1,
                rot_y_div90: 2,
                uvlock: true,
            }],
            multipart: vec![Variant {
                when: "facing=north".into(),
                model_idx: 0,
                rot_x_div90: 0,
                rot_y_div90: 0,
                uvlock: false,
            }],
        }],
        langs: Vec::new(),
        is_overlay: false,
    };
    BlockDb {
        strings,
        langs: vec![vec![LangEntry {
            block_name: "minecraft:stone".into(),
            en: "Stone".into(),
            zh: "石头".into(),
        }]],
        // 贴图与像素默认留空，由需要它们的用例自行填充。
        // 本地 flate2 桩早已改为产出合法的 stored 块，像素池的往返在本地
        // 也能验证（见 texture_section_round_trips_fields_and_pixels）。
        textures: Vec::new(),
        models: vec![model],
        versions: vec![version],
        pixels: Vec::new(),
    }
}

#[test]
fn round_trip_preserves_versions_and_blocks() {
    let db = sample_db();
    let bytes = serialize(&db).expect("序列化失败");
    let back = BlockDb::load_from_bytes(&bytes, false).expect("读回失败");

    assert_eq!(back.versions.len(), 1);
    let v = &back.versions[0];
    assert_eq!(v.name, "1.20.1");
    assert_eq!(v.data_version, 3465);
    assert_eq!(v.blocks.len(), 1);

    let b = &v.blocks[0];
    assert_eq!(b.name, "minecraft:stone");
    assert_eq!(b.variants.len(), 1);
    assert_eq!(b.variants[0].when, "");
    assert_eq!(b.variants[0].model_idx, 0);
    assert_eq!(b.variants[0].rot_x_div90, 1);
    assert_eq!(b.variants[0].rot_y_div90, 2);
    assert!(b.variants[0].uvlock);
    assert_eq!(b.multipart.len(), 1);
    assert_eq!(b.multipart[0].when, "facing=north");
}

#[test]
fn round_trip_preserves_strings_and_texture_map() {
    let db = sample_db();
    let bytes = serialize(&db).unwrap();
    let back = BlockDb::load_from_bytes(&bytes, false).unwrap();

    // 原字符串表的下标必须保持不变：模型面里存的就是这些下标
    for (i, s) in db.strings.iter().enumerate() {
        assert_eq!(&back.strings[i], s, "字符串 {i} 发生了变化");
    }
    assert_eq!(
        back.versions[0].texture_by_path.get("block/stone"),
        Some(&0),
        "贴图路径到池下标的映射应保留"
    );
}

#[test]
fn round_trip_preserves_language_table() {
    let db = sample_db();
    let back = BlockDb::load_from_bytes(&serialize(&db).unwrap(), false).unwrap();
    assert_eq!(back.langs.len(), 1, "语言表个数应为 1");
    assert_eq!(back.langs[0].len(), 1);
    let e = &back.langs[0][0];
    assert_eq!(e.block_name, "minecraft:stone");
    assert_eq!(e.en, "Stone");
    assert_eq!(e.zh, "石头");
    // 版本上的语言表也要挂对
    assert_eq!(back.versions[0].langs.len(), 1);
    assert_eq!(back.versions[0].langs[0].zh, "石头");
}

/// 贴图段的往返：字段与像素内容都要逐字节保留。
///
/// 这条测试此前是**恒真**的：夹具的 textures 与 pixels 都是空的，
/// 两边长度都是 0，贴图段写不写、读不读都能通过；注释还写着「由 CI 的
/// cargo test 覆盖」，而 CI 跑的仍是同一个空夹具。现在填入真实的贴图
/// 记录与像素，逐字段与逐字节比对。
///
/// 已验证有效性：把写入端的像素截短一个字节后这条会失败
/// （读取端报「贴图 1 的范围超出像素池长度」），而旧写法照样通过。
#[test]
fn texture_section_round_trips_fields_and_pixels() {
    let mut db = sample_db();
    db.textures = vec![
        TextureInfo {
            w: 16,
            h: 16,
            offset: 0,
            len: 256,
            has_alpha: true,
        },
        TextureInfo {
            w: 1,
            h: 1,
            offset: 256,
            len: 4,
            has_alpha: false,
        },
    ];
    // 像素按位置取不同值：这样「偏移写错」或「长度截断」都能暴露，
    // 全填同一个值则错位也看不出来
    db.pixels = (0..260).map(|i| (i % 251) as u8).collect();

    let back = BlockDb::load_from_bytes(&serialize(&db).unwrap(), false).unwrap();

    assert_eq!(back.textures.len(), 2, "贴图记录数应保留");
    for (i, t) in db.textures.iter().enumerate() {
        assert_eq!(back.textures[i].w, t.w, "第 {i} 条贴图的宽");
        assert_eq!(back.textures[i].h, t.h, "第 {i} 条贴图的高");
        assert_eq!(back.textures[i].offset, t.offset, "第 {i} 条贴图的偏移");
        assert_eq!(back.textures[i].len, t.len, "第 {i} 条贴图的长度");
        assert_eq!(
            back.textures[i].has_alpha, t.has_alpha,
            "第 {i} 条贴图的 alpha 标记"
        );
    }
    assert_eq!(back.pixels, db.pixels, "像素池应逐字节一致");

    // 贴图切片也要能取到正确的像素：只比对整个池会漏掉偏移算错的情况
    assert_eq!(
        back.texture_pixels(0).map(|p| p.to_vec()),
        Some(db.pixels[0..256].to_vec()),
        "第 0 条贴图的像素切片"
    );
    assert_eq!(
        back.texture_pixels(1).map(|p| p.to_vec()),
        Some(db.pixels[256..260].to_vec()),
        "第 1 条贴图的像素切片"
    );
}

#[test]
fn round_trip_preserves_models_and_faces() {
    let db = sample_db();
    let back = BlockDb::load_from_bytes(&serialize(&db).unwrap(), false).unwrap();
    assert_eq!(back.models.len(), 1);
    let m = &back.models[0];
    assert!(!m.translucent);
    assert_eq!(m.elements.len(), 2, "元素数应保留");

    let e0 = &m.elements[0];
    assert_eq!(e0.from, [0.0, 0.0, 0.0]);
    assert_eq!(e0.to, [16.0, 16.0, 16.0]);
    assert_eq!(e0.face_mask, 0b000011);
    assert_eq!(e0.faces.len(), 2, "面数应与 faceMask 一致");
    assert_eq!(e0.faces[0].texture, 2, "贴图字符串下标应保留");
    assert_eq!(e0.faces[0].uv, [0.0, 0.0, 16.0, 16.0]);

    let e1 = &m.elements[1];
    assert_eq!(e1.face_mask, 0b100000);
    assert_eq!(e1.faces.len(), 1);
}

#[test]
fn serialization_is_deterministic() {
    let db = sample_db();
    let a = serialize(&db).unwrap();
    let b = serialize(&db).unwrap();
    assert_eq!(a, b, "同一输入必须产出完全相同的字节");
}

#[test]
fn rejects_multiple_versions() {
    let mut db = sample_db();
    let extra = db.versions[0].clone();
    db.versions.push(extra);
    let e = serialize(&db).unwrap_err().to_string();
    assert!(e.contains("只容纳一个版本"), "应说明版本数限制：{e}");
}

#[test]
fn rejects_empty_db() {
    let mut db = sample_db();
    db.versions.clear();
    let e = serialize(&db).unwrap_err().to_string();
    assert!(e.contains("没有任何版本"), "应说明缺少版本：{e}");
}

#[test]
fn rejects_multiple_lang_tables() {
    let mut db = sample_db();
    let extra = db.langs[0].clone();
    db.langs.push(extra);
    let e = serialize(&db).unwrap_err().to_string();
    assert!(e.contains("只容纳一个语言表"), "应说明语言表限制：{e}");
}

/// 变体数超过 u16 时必须报错，而不是截断成 0。
///
/// 截断的后果不是 panic 而是**级联静默错位**：写入端声明 0 条却照实
/// 写 65536 条，读取端按 0 条读取后，把变体的字节当成下一个方块的
/// 字段继续消费，整个版本段从此错位，且全程不报错。
#[test]
fn rejects_variant_count_over_u16() {
    let mut db = sample_db();
    let v = db.versions[0].blocks[0].variants[0].clone();
    db.versions[0].blocks[0].variants = vec![v; u16::MAX as usize + 1];
    let e = serialize(&db).unwrap_err().to_string();
    assert!(
        e.contains("变体数") && e.contains("超过上限"),
        "应说明变体数超限：{e}"
    );
}

/// multipart 数同理。
#[test]
fn rejects_multipart_count_over_u16() {
    let mut db = sample_db();
    let v = db.versions[0].blocks[0].multipart[0].clone();
    db.versions[0].blocks[0].multipart = vec![v; u16::MAX as usize + 1];
    let e = serialize(&db).unwrap_err().to_string();
    assert!(
        e.contains("multipart") && e.contains("超过上限"),
        "应说明 multipart 数超限：{e}"
    );
}

/// 恰好等于上限时不应被误判为超限。
#[test]
fn accepts_variant_count_at_u16_limit() {
    let mut db = sample_db();
    let v = db.versions[0].blocks[0].variants[0].clone();
    db.versions[0].blocks[0].variants = vec![v; u16::MAX as usize];
    let r = serialize(&db);
    assert!(
        r.is_ok(),
        "恰好 65535 条变体应可写出，实际：{:?}",
        r.err().map(|e| e.to_string())
    );
}

#[test]
fn face_count_mismatch_is_reported() {
    // faceMask 置了两位但只给一个面：必须报错而不是写出错位数据
    let mut db = sample_db();
    db.models[0].elements[0].faces.truncate(1);
    let e = serialize(&db).unwrap_err().to_string();
    assert!(e.contains("面数"), "应指出面数与 faceMask 不符：{e}");
}

#[test]
fn extra_faces_are_reported() {
    let mut db = sample_db();
    // 多给一个面：多余的面不会被写出，必须报错以免静默丢弃
    db.models[0].elements[0].faces.push(face(2));
    let e = serialize(&db).unwrap_err().to_string();
    assert!(e.contains("面数"), "应指出面数不符：{e}");
}

#[test]
fn empty_lang_table_is_written_as_one_table_with_zero_entries() {
    let mut db = sample_db();
    db.langs.clear();
    let back = BlockDb::load_from_bytes(&serialize(&db).unwrap(), false).unwrap();
    assert_eq!(back.langs.len(), 1, "无语言表也要写一个空表");
    assert!(back.langs[0].is_empty());
}

#[test]
fn header_declares_one_lang_table_count() {
    // 文件头的 langCount 是语言表个数，写错会让读取端整体错位
    let db = sample_db();
    let bytes = serialize(&db).unwrap();
    // 魔数 8 + format 4 + versionCount 4 + textureCount 4 + modelCount 4 = 24
    let lang_count = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    assert_eq!(lang_count, 1);
}

#[test]
fn empty_db_with_no_models_or_textures_still_round_trips() {
    let mut db = sample_db();
    db.models.clear();
    db.textures.clear();
    let back = BlockDb::load_from_bytes(&serialize(&db).unwrap(), false).unwrap();
    assert!(back.models.is_empty());
    assert!(back.textures.is_empty());
    assert_eq!(back.versions.len(), 1);
}
