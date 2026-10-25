use super::*;
use crate::block::db::{BlockDef, LangEntry, Variant, VersionDef};

fn version(name: &str, dv: i32, blocks: &[(&str, &str, &str)], overlay: bool) -> VersionDef {
    VersionDef {
        name: name.into(),
        data_version: dv,
        lang_idx: 0,
        texture_by_path: HashMap::new(),
        blocks: blocks
            .iter()
            .map(|(n, _, _)| BlockDef {
                name: (*n).into(),
                variants: vec![Variant {
                    when: String::new(),
                    model_idx: 0,
                    rot_x_div90: 0,
                    rot_y_div90: 0,
                    uvlock: false,
                }],
                multipart: Vec::new(),
            })
            .collect(),
        langs: blocks
            .iter()
            .map(|(n, zh, en)| LangEntry {
                block_name: (*n).into(),
                zh: (*zh).into(),
                en: (*en).into(),
            })
            .collect(),
        is_overlay: overlay,
    }
}

fn db_with(versions: Vec<VersionDef>) -> BlockDb {
    BlockDb {
        strings: Vec::new(),
        langs: Vec::new(),
        textures: Vec::new(),
        models: Vec::new(),
        versions,
        pixels: Vec::new(),
    }
}

fn sample() -> BlockDb {
    db_with(vec![
        version(
            "1.20.1",
            3465,
            &[
                ("minecraft:stone", "石头", "Stone"),
                ("minecraft:stone_bricks", "石砖", "Stone Bricks"),
                ("minecraft:oak_log", "橡木原木", "Oak Log"),
            ],
            false,
        ),
        version(
            "1.21",
            3953,
            &[("minecraft:stone", "石头", "Stone"), ("minecraft:copper_block", "铜块", "Block of Copper")],
            false,
        ),
    ])
}

#[test]
fn search_by_chinese_name() {
    let db = sample();
    // 完全匹配中文名：只应命中石头本身。
    // 注意「石砖」并不包含「石头」，二者没有子串关系。
    let exact = search(&db, "石头", 20);
    assert_eq!(exact.len(), 1, "只应命中石头，实际 {:?}", exact.iter().map(|h| &h.name).collect::<Vec<_>>());
    assert_eq!(exact[0].name, "minecraft:stone");
    assert_eq!(exact[0].zh, "石头");
}

#[test]
fn search_by_chinese_prefix_matches_several() {
    let db = sample();
    // 前缀「石」应同时命中石头与石砖
    let hits = search(&db, "石", 20);
    let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
    assert!(names.contains(&"minecraft:stone"), "实际 {names:?}");
    assert!(names.contains(&"minecraft:stone_bricks"), "实际 {names:?}");
    // 完全匹配的应排在前面
    assert_eq!(names[0], "minecraft:stone");
}

#[test]
fn search_by_short_and_full_id() {
    let db = sample();
    let by_short = search(&db, "oak_log", 20);
    assert_eq!(by_short.len(), 1);
    assert_eq!(by_short[0].name, "minecraft:oak_log");

    let by_full = search(&db, "minecraft:oak_log", 20);
    assert_eq!(by_full[0].name, "minecraft:oak_log");
}

#[test]
fn search_is_case_insensitive() {
    let db = sample();
    let a = search(&db, "STONE", 20);
    let b = search(&db, "stone", 20);
    assert!(!a.is_empty());
    assert_eq!(a.len(), b.len());
}

#[test]
fn same_block_across_versions_is_merged() {
    let db = sample();
    let hits = search(&db, "minecraft:stone", 20);
    let stone = hits.iter().find(|h| h.name == "minecraft:stone").expect("应有石头");
    assert_eq!(stone.versions.len(), 2, "存在于两个版本时应合并为一条，并列出两个版本");
    assert_eq!(stone.versions[0].data_version, 3465, "版本应按 dataVersion 升序");
    assert_eq!(stone.versions[1].data_version, 3953);
}

#[test]
fn results_are_ranked_exact_first() {
    let db = sample();
    let hits = search(&db, "stone", 20);
    // 完全匹配短名的 minecraft:stone 应排在 minecraft:stone_bricks 之前
    let i_stone = hits.iter().position(|h| h.name == "minecraft:stone").unwrap();
    let i_bricks = hits.iter().position(|h| h.name == "minecraft:stone_bricks").unwrap();
    assert!(i_stone < i_bricks, "完全匹配应在前，实际 {i_stone} vs {i_bricks}");
}

#[test]
fn empty_query_returns_nothing() {
    let db = sample();
    assert!(search(&db, "", 20).is_empty());
    assert!(search(&db, "   ", 20).is_empty(), "全空白也视为空查询");
}

#[test]
fn limit_is_respected() {
    let db = sample();
    assert_eq!(search(&db, "stone", 1).len(), 1);
    // limit 为 0 时至少返回一条，避免界面空白
    assert_eq!(search(&db, "stone", 0).len(), 1);
}

#[test]
fn version_options_sorted_with_overlays_last() {
    let db = db_with(vec![
        version("base-1.21", 3953, &[], false),
        version("base-1.20.1", 3465, &[], false),
        version("modpack", -1, &[], true),
    ]);
    let opts = version_options(&db);
    assert_eq!(opts[0].data_version, 3465);
    assert_eq!(opts[1].data_version, 3953);
    assert!(opts[2].is_overlay, "叠加包应排在最后");
}

#[test]
fn no_match_returns_empty() {
    let db = sample();
    assert!(search(&db, "不存在的方块名", 20).is_empty());
}

#[test]
fn list_all_lists_every_block_across_versions() {
    let db = sample();
    let hits = list_all(&db);
    let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
    // 两个版本合起来共 4 个不同的方块名；没有查询也要全部列出，
    // 否则用户不输入关键字时就挑不到任何方块
    assert_eq!(
        names,
        vec![
            "minecraft:copper_block",
            "minecraft:oak_log",
            "minecraft:stone",
            "minecraft:stone_bricks"
        ],
        "应按方块 ID 升序列出全部方块，实际 {names:?}"
    );
}

#[test]
fn list_all_merges_versions_of_the_same_block() {
    let db = sample();
    let stone = list_all(&db)
        .into_iter()
        .find(|h| h.name == "minecraft:stone")
        .expect("应有石头");
    assert_eq!(stone.versions.len(), 2, "两个版本都有的方块只应出现一次并列出两个版本");
    // 与检索给出同一份版本列表：两处各自合并会让同一方块在两条路径上不一致
    let by_search = search(&db, "minecraft:stone", 20)
        .into_iter()
        .find(|h| h.name == "minecraft:stone")
        .expect("检索也应命中石头");
    assert_eq!(stone.versions, by_search.versions);
}

#[test]
fn list_all_keeps_blocks_without_display_name() {
    let db = db_with(vec![version(
        "1.20.1",
        3465,
        &[("minecraft:stone", "石头", "Stone"), ("mod:mystery", "", "")],
        false,
    )]);
    let hits = list_all(&db);
    let mystery = hits
        .iter()
        .find(|h| h.name == "mod:mystery")
        .expect("没有显示名称的方块也要列出，否则它在选择列表里永远不出现");
    assert!(mystery.zh.is_empty());
    assert!(mystery.en.is_empty());
}

#[test]
fn list_all_of_empty_library_is_empty() {
    assert!(list_all(&db_with(Vec::new())).is_empty());
}
