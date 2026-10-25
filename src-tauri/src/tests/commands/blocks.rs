use super::*;

use crate::block::db::{BlockDb, BlockDef, LangEntry, Model, ModelElement, ModelFace, TextureInfo, Variant, VersionDef};

use std::collections::HashMap;

/// 替换日志要带上方块中文名。
///
/// 日志是事后翻的：只写 `minecraft:stone` 得对着方块库才看得懂替换了什么，
/// 只写中文名又对不上文件里的内容。查不到名字时必须只写 ID，
/// 不能留下空括号。
#[test]
fn replace_log_line_carries_chinese_names() {
    let mut db = sample_db("pack", 100, "minecraft:stone", "tex", 0);
    db.langs = vec![vec![
        LangEntry {
            block_name: "minecraft:stone".into(),
            en: "Stone".into(),
            zh: "石头".into(),
        },
        LangEntry {
            block_name: "minecraft:dirt".into(),
            en: "Dirt".into(),
            zh: "泥土".into(),
        },
    ]];

    let line = replace_log_line(&db, Some(0), "demo.litematic", "minecraft:stone", "minecraft:dirt", 12);
    assert!(line.starts_with("替换 demo.litematic："), "应保留原有结构：{line}");
    assert!(line.contains("石头（minecraft:stone）"), "源方块应带中文名：{line}");
    assert!(line.contains("泥土（minecraft:dirt）"), "目标方块应带中文名：{line}");
    assert!(line.ends_with("共 12 个"), "应保留数量：{line}");

    // 方块库里查不到名字（模组方块）：只写 ID，不留空括号
    let unknown = replace_log_line(&db, Some(0), "demo.litematic", "mod:unknown", "minecraft:dirt", 1);
    assert!(unknown.contains("mod:unknown ->"), "查不到名字时应只写 ID：{unknown}");
    assert!(!unknown.contains("（）"), "不该出现空括号：{unknown}");
}

/// 造一个可序列化、可读回的最小方块库
fn sample_db(pack: &str, dv: i32, block: &str, tex: &str, pixel: u8) -> BlockDb {
    let strings = vec![pack.to_string(), tex.to_string()];
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
            face_mask: 0b1,
            faces: vec![ModelFace {
                texture: 1,
                uv: [0.0, 0.0, 16.0, 16.0],
                cullface: 0xFF,
                tintindex: -1,
                rotation: 0,
            }],
        }],
    };
    let version = VersionDef {
        name: pack.into(),
        data_version: dv,
        lang_idx: 0,
        texture_by_path: HashMap::from([(tex.to_string(), 0u32)]),
        blocks: vec![BlockDef {
            name: block.into(),
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
        strings,
        langs: vec![vec![LangEntry {
            block_name: block.into(),
            en: "EN".into(),
            zh: "中文名".into(),
        }]],
        textures: vec![TextureInfo { w: 1, h: 1, offset: 0, len: 4, has_alpha: false }],
        models: vec![model],
        versions: vec![version],
        pixels: vec![pixel, pixel, pixel, 255],
    }
}
