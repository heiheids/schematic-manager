use super::*;
use crate::block::db::{Model, TextureInfo, VersionDef};
use std::collections::HashMap;

fn st(name: &str, props: &[(&str, &str)]) -> BlockState {
    let mut b = BlockState::new(name);
    for (k, v) in props {
        b.set_prop(k, v);
    }
    b
}

fn var(when: &str, model: u32) -> Variant {
    Variant {
        when: when.to_string(),
        model_idx: model,
        rot_x_div90: 0,
        rot_y_div90: 0,
        uvlock: false,
    }
}

fn db_with(block: BlockDef, version_name: &str, data_version: i32) -> BlockDb {
    BlockDb {
        strings: Vec::new(),
        langs: Vec::new(),
        textures: vec![TextureInfo { w: 1, h: 1, offset: 0, len: 4, has_alpha: false }],
        models: vec![Model { translucent: false, elements: Vec::new() }],
        versions: vec![VersionDef {
            name: version_name.to_string(),
            data_version,
            lang_idx: 0,
            texture_by_path: HashMap::new(),
            blocks: vec![block],
            langs: Vec::new(),
            is_overlay: false,
        }],
        pixels: vec![0u8; 4],
    }
}

/// 默认值必须取「首次出现」的值，而不是字典序最小的值。
///
/// 这里刻意让两者不同：facing 的取值域排序后以 east 开头，
/// 但首次出现（即首变体）是 north。若按字典序取，会得到 east。
/// legacy-cpp 的 DefaultProps 用的是 bd.props 的首次出现顺序。
#[test]
fn defaults_use_first_occurrence_not_alphabetical() {
    let bd = BlockDef {
        name: "minecraft:oak_stairs".into(),
        variants: vec![
            var("facing=north", 0),
            var("facing=east|facing=south|facing=west", 0),
        ],
        multipart: Vec::new(),
    };
    let db = db_with(bd, "1.20.1", 3465);
    let info = DbBlockInfo { db: &db, version: &db.versions[0] };
    let d = info.default_props("minecraft:oak_stairs");
    assert_eq!(
        d.iter().find(|(k, _)| k == "facing").map(|(_, v)| v.as_str()),
        Some("north"),
        "默认值应为首次出现的 north，而不是字典序最小的 east；实际 {d:?}"
    );
}
#[test]
fn parse_conds_splits_on_comma_and_first_equals() {
    let c = parse_conds("facing=east,half=bottom");
    assert_eq!(c, vec![
        ("facing".to_string(), "east".to_string()),
        ("half".to_string(), "bottom".to_string()),
    ]);
    // 值里含等号时只按第一个等号切分
    let c = parse_conds("a=b=c");
    assert_eq!(c, vec![("a".to_string(), "b=c".to_string())]);
    // 缺少等号或等号在首位：忽略该片段
    assert!(parse_conds("nope").is_empty());
    assert!(parse_conds("=v").is_empty());
    assert!(parse_conds("").is_empty());
}

#[test]
fn and_semantics_require_all_conditions() {
    let s = st("minecraft:oak_stairs", &[("facing", "east"), ("half", "bottom")]);
    assert!(match_when("facing=east,half=bottom", &s));
    assert!(!match_when("facing=east,half=top", &s), "任一条件不满足即为假");
    assert!(!match_when("facing=west,half=bottom", &s));
    // 方块缺少该属性时也判为不匹配，而不是视为通过
    assert!(!match_when("waterlogged=true", &s));
}

#[test]
fn empty_condition_always_matches() {
    let s = st("minecraft:stone", &[]);
    assert!(match_when("", &s));
    assert!(match_any_when("", &s));
}

#[test]
fn pipe_is_or() {
    let s = st("minecraft:oak_fence", &[("north", "true")]);
    assert!(match_any_when("north=true|south=true", &s));
    assert!(!match_any_when("east=true|south=true", &s));
    // 备选内部仍是与语义：要让整个与条件成立，
    // 每一对键值都必须在该方块上存在且相等。
    // 注意：方块缺少某属性时该条件不成立（而不是「默认通过」），
    // 因此 south=false 在缺少 south 属性时应判为不匹配。
    assert!(!match_any_when("north=true,south=false|east=true", &s),
            "缺少 south 属性时 south=false 不应视为匹配");
    assert!(!match_any_when("north=true,south=true|east=true", &s));
    // 换成方块确实拥有的属性，且第一备选成立时，整体为真
    assert!(match_any_when("north=true|east=true", &s));
    assert!(match_any_when("north=false|north=true", &s), "第二个备选成立");
}

#[test]
fn variants_take_first_match_only() {
    let bd = BlockDef {
        name: "minecraft:oak_stairs".into(),
        variants: vec![var("facing=east", 0), var("facing=east", 0), var("", 0)],
        multipart: Vec::new(),
    };
    let db = db_with(bd, "1.20.1", 3465);
    let got = resolve_models(&db, &db.versions[0], &st("minecraft:oak_stairs", &[("facing", "east")]));
    assert_eq!(got.len(), 1, "variants 只取第一个匹配项");
}

#[test]
fn multipart_stacks_all_matches() {
    let bd = BlockDef {
        name: "minecraft:oak_fence".into(),
        variants: Vec::new(),
        multipart: vec![var("north=true", 0), var("south=true", 0), var("east=true", 0)],
    };
    let db = db_with(bd, "1.20.1", 3465);
    let got = resolve_models(
        &db,
        &db.versions[0],
        &st("minecraft:oak_fence", &[("north", "true"), ("south", "true")]),
    );
    assert_eq!(got.len(), 2, "multipart 应叠加全部匹配项，实际 {got:?}");
}

#[test]
fn unknown_block_resolves_to_nothing() {
    let bd = BlockDef { name: "minecraft:stone".into(), variants: vec![var("", 0)], multipart: Vec::new() };
    let db = db_with(bd, "1.20.1", 3465);
    assert!(resolve_models(&db, &db.versions[0], &st("mod:unknown", &[])).is_empty());
}

#[test]
fn overlay_block_is_found() {
    // 基础版本没有该方块，叠加包提供时才应解析成功
    let mut db = db_with(
        BlockDef { name: "minecraft:stone".into(), variants: vec![var("", 0)], multipart: Vec::new() },
        "1.20.1",
        3465,
    );
    db.versions.push(VersionDef {
        name: "modpack".into(),
        data_version: -1,
        lang_idx: 0,
        texture_by_path: HashMap::new(),
        blocks: vec![BlockDef {
            name: "mod:custom".into(),
            variants: vec![var("", 0)],
            multipart: Vec::new(),
        }],
        langs: Vec::new(),
        is_overlay: true,
    });
    let got = resolve_models(&db, &db.versions[0], &st("mod:custom", &[]));
    assert_eq!(got.len(), 1, "叠加包中的方块应能被解析到");
}

#[test]
fn prop_domain_comes_from_condition_strings() {
    let bd = BlockDef {
        name: "minecraft:oak_stairs".into(),
        variants: vec![
            var("facing=north,half=bottom,shape=straight", 0),
            var("facing=east,half=top,shape=outer_left", 0),
        ],
        multipart: Vec::new(),
    };
    let db = db_with(bd, "1.20.1", 3465);
    let (domain, defaults) = block_prop_domain(&db, &db.versions[0], "minecraft:oak_stairs").unwrap();

    let facing = domain.get("facing").expect("应有 facing");
    assert_eq!(facing, &vec!["east".to_string(), "north".to_string()], "取值应去重且有序");
    assert_eq!(domain.get("half").unwrap().len(), 2);
    // 默认值取自第一个变体，而不是字母序第一个
    assert_eq!(
        defaults.iter().find(|(k, _)| k == "facing").map(|(_, v)| v.as_str()),
        Some("north"),
        "默认值应来自第一个变体"
    );
}

#[test]
fn prop_domain_includes_multipart_conditions() {
    let bd = BlockDef {
        name: "minecraft:oak_fence".into(),
        variants: vec![var("north=true", 0)],
        multipart: vec![var("waterlogged=true", 0), var("south=true", 0)],
    };
    let db = db_with(bd, "1.20.1", 3465);
    let (domain, _) = block_prop_domain(&db, &db.versions[0], "minecraft:oak_fence").unwrap();
    assert!(domain.contains_key("north"));
    assert!(domain.contains_key("south"), "multipart 的条件也应计入");
    assert!(domain.contains_key("waterlogged"));
}

#[test]
fn supports_prop_checks_value_not_just_name() {
    let bd = BlockDef {
        name: "minecraft:oak_stairs".into(),
        variants: vec![var("facing=north|facing=south", 0)],
        multipart: Vec::new(),
    };
    let db = db_with(bd, "1.20.1", 3465);
    let info = DbBlockInfo { db: &db, version: &db.versions[0] };
    assert!(info.supports_prop("minecraft:oak_stairs", "facing", "north"));
    assert!(info.supports_prop("minecraft:oak_stairs", "facing", "south"));
    // 属性名对但取值不支持，仍应判为不支持
    assert!(!info.supports_prop("minecraft:oak_stairs", "facing", "up"));
    // 未知方块一律不支持
    assert!(!info.supports_prop("mod:unknown", "facing", "north"));
}

#[test]
fn defaults_fall_back_to_domain_first_value() {
    // 第一个变体没提到 shape，但后续变体提到了，
    // 此时默认值应取自取值域中的第一个值，而不是缺失
    let bd = BlockDef {
        name: "minecraft:oak_stairs".into(),
        variants: vec![var("facing=north", 0), var("shape=outer_left|shape=straight", 0)],
        multipart: Vec::new(),
    };
    let db = db_with(bd, "1.20.1", 3465);
    let info = DbBlockInfo { db: &db, version: &db.versions[0] };
    let d = info.default_props("minecraft:oak_stairs");
    assert!(d.iter().any(|(k, v)| k == "facing" && v == "north"));
    assert!(
        d.iter().any(|(k, v)| k == "shape" && v == "outer_left"),
        "未在首变体出现的属性应取取值域第一个值，实际 {d:?}"
    );
}

#[test]
fn variant_rotation_is_carried_through() {
    let bd = BlockDef {
        name: "minecraft:stone".into(),
        variants: vec![Variant {
            when: String::new(),
            model_idx: 0,
            rot_x_div90: 1,
            rot_y_div90: 3,
            uvlock: true,
        }],
        multipart: Vec::new(),
    };
    let db = db_with(bd, "1.20.1", 3465);
    let got = resolve_models(&db, &db.versions[0], &st("minecraft:stone", &[]));
    assert_eq!(got[0].rot_x_div90, 1);
    assert_eq!(got[0].rot_y_div90, 3);
    assert!(got[0].uvlock, "uvlock 必须一并带出，否则贴图方向会错");
}
