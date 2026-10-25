use super::*;
use crate::block::db::VersionDef;
use crate::block::db::{BlockDef, Variant};
use crate::schematic::litematic::{BlockState, Region};
use std::path::Path;
use std::collections::HashMap;

fn version(name: &str, dv: i32, blocks: &[&str]) -> VersionDef {
    VersionDef {
        name: name.into(),
        data_version: dv,
        lang_idx: 0,
        texture_by_path: HashMap::new(),
        blocks: blocks
            .iter()
            .map(|n| BlockDef {
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
        langs: Vec::new(),
        is_overlay: false,
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

fn schematic(palette: Vec<BlockState>, indices: Vec<i32>, dv: i32) -> Schematic {
    Schematic {
        root: crate::format::nbt::Value::Compound(vec![(
            "MinecraftDataVersion".into(),
            crate::format::nbt::Value::Int(dv),
        )]),
        regions: vec![Region {
            name: "r".into(),
            pos: [0, 0, 0],
            size: [indices.len() as i32, 1, 1],
            volume: indices.len(),
            bits: 4,
            palette,
            indices,
            ..Default::default()
        }],
        path: String::new(),
        meta_modified: false,
        blocks_modified: false,
    }
}

/// 版本名不得把转换结果写出投影所在目录。
///
/// 这不是假想的输入：`base/blocks` 下的单版本包可以由用户自己放进去，
/// 版本名随之而来。这里把当时实测能逃出目录的几个取值固定成用例。
#[test]
fn safe_file_component_blocks_path_traversal() {
    for bad in [
        r"a\..\..\evil",
        "a/../../evil",
        r"..\..\evil",
        "C:/Windows/Temp/evil",
        r"a\..\..\..\..\evil",
    ] {
        let cleaned = safe_file_component(bad);
        // 清理后必须只剩一段文件名：不能含任何路径分隔符。
        // 注意不必禁止子串 ".."——`a_.._.._evil` 里的 ".." 已被下划线
        // 夹住，整体只是一个普通文件名，不再是上级目录引用。
        assert!(
            !cleaned.contains('/') && !cleaned.contains('\\'),
            "版本名 {bad:?} 清理后仍含路径分隔符：{cleaned:?}"
        );
        // 真正要守的性质：拼出的文件仍落在原目录，父目录不变
        let joined = Path::new("/data/proj").join(format!("demo_{cleaned}.litematic"));
        assert_eq!(
            joined.parent(),
            Some(Path::new("/data/proj")),
            "版本名 {bad:?} 清理后仍会写出目录：{}",
            joined.display()
        );
    }
}

#[test]
fn safe_file_component_keeps_normal_version_names() {
    // 正常版本名必须原样保留，否则转换结果的文件名会变得难以辨认
    assert_eq!(safe_file_component("1.20.1"), "1.20.1");
    assert_eq!(safe_file_component("26.3"), "26.3");
    assert_eq!(safe_file_component("我的整合包"), "我的整合包");
}

#[test]
fn safe_file_component_handles_windows_quirks() {
    // 结尾的点与空格在 Windows 上被静默忽略，会导致实际文件名与预期不符
    assert_eq!(safe_file_component("1.20.1."), "1.20.1");
    assert_eq!(safe_file_component("  1.20.1  "), "1.20.1");
    assert_eq!(safe_file_component("1.20.1 "), "1.20.1");
    // 保留字符与控制字符换成下划线
    assert_eq!(safe_file_component("a<b>c"), "a_b_c");
    assert_eq!(safe_file_component("a|b"), "a_b");
    assert_eq!(safe_file_component("a\u{7}b"), "a_b");
    // 全是分隔符时会被换成下划线，得到 ____ 这类可用文件名（不是空串）；
    // 只有清理后**确实为空**（空输入或全是空格/结尾点）才用兜底名
    assert_eq!(safe_file_component(r"\\\/"), "____");
    assert_eq!(safe_file_component("..."), "未知版本");
    assert_eq!(safe_file_component("   "), "未知版本");
    assert_eq!(safe_file_component(""), "未知版本");
}

/// 两个版本名清理后若相同，会写到同一个文件、互相覆盖。
/// 这是清理带来的固有取舍：宁可让用户看到「覆盖」，也不能写出目录。
#[test]
fn safe_file_component_collapses_slash_variants() {
    assert_eq!(
        safe_file_component("a/b"),
        safe_file_component(r"a\b"),
        "路径分隔符的两种写法应归一到同一个文件名"
    );
}

#[test]
fn analyze_lists_only_blocks_missing_from_target() {        let old = version("old", 100, &["minecraft:oak_stairs", "minecraft:stone"]);
    let new = version("new", 200, &["minecraft:stone"]);
    let db = db_with(vec![old, new]);
    let sc = schematic(
        vec![
            BlockState::new("minecraft:oak_stairs"),
            BlockState::new("minecraft:stone"),
        ],
        vec![0, 1, 0],
        100,
    );
    let items = analyze(&sc, &db, 1);
    assert_eq!(items.len(), 1, "只有目标版本缺失的方块才需处理");
    assert_eq!(items[0].name, "minecraft:oak_stairs");
    assert_eq!(items[0].count, 2, "应统计出现次数");
}

#[test]
fn analyze_counts_per_palette_entry_not_per_block() {
    // 同一方块在调色板里只出现一次，但实例数是 3
    let old = version("old", 100, &["minecraft:gone"]);
    let new = version("new", 200, &["minecraft:stone"]);
    let db = db_with(vec![old, new]);
    let sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0, 0, 0], 100);
    let items = analyze(&sc, &db, 1);
    assert_eq!(items[0].count, 3, "应按调色板项折算实例数");
}

#[test]
fn suggest_prefers_same_suffix() {
    let target = ["minecraft:oak_slab", "minecraft:spruce_stairs", "minecraft:deepslate_stone"];
    // 同后缀 _stairs -> 应选 spruce_stairs
    assert_eq!(suggest_block("mod:bamboo_stairs", &target), "minecraft:spruce_stairs");
    // 同后缀 _slab -> 应选 oak_slab
    assert_eq!(suggest_block("mod:birch_slab", &target), "minecraft:oak_slab");
}

#[test]
fn suggest_falls_back_to_same_prefix() {
    let target = ["minecraft:deepslate_bricks", "minecraft:stone"];
    // 没有同后缀候选时用同前缀
    assert_eq!(
        suggest_block("mod:deepslate_tiles", &target),
        "minecraft:deepslate_bricks"
    );
}

#[test]
fn suggest_prefers_vanilla_and_is_stable() {
    let target = ["modpack:aaa_stairs", "minecraft:zzz_stairs"];
    // 原版优先，即便字典序靠后
    assert_eq!(suggest_block("mod:x_stairs", &target), "minecraft:zzz_stairs");
    // 同一输入两次结果一致
    assert_eq!(
        suggest_block("mod:x_stairs", &target),
        suggest_block("mod:x_stairs", &target)
    );
}

#[test]
fn suggest_returns_empty_when_nothing_matches() {
    assert_eq!(suggest_block("minecraft:stone", &["mod:completely_different"]), "");
}

#[test]
fn analyze_sorted_by_count_desc() {
    let old = version("old", 100, &["minecraft:a", "minecraft:b"]);
    let new = version("new", 200, &[]);
    let db = db_with(vec![old, new]);
    let sc = schematic(
        vec![BlockState::new("minecraft:a"), BlockState::new("minecraft:b")],
        vec![0, 0, 0, 1],
        100,
    );
    let items = analyze(&sc, &db, 1);
    assert_eq!(items[0].name, "minecraft:a", "数量多的排前面");
    assert_eq!(items[0].count, 3);
    assert_eq!(items[1].count, 1);
}

#[test]
fn convert_replaces_and_updates_data_version() {
    let old = version("old", 100, &["minecraft:gone", "minecraft:stone"]);
    let new = version("new", 200, &["minecraft:kept"]);
    let db = db_with(vec![old, new]);
    let mut sc = schematic(
        vec![BlockState::new("minecraft:gone"), BlockState::new("minecraft:stone")],
        vec![0, 1, 0],
        100,
    );
    let items = vec![ConvertItem {
        name: "minecraft:gone".into(),
        count: 2,
        suggested: "minecraft:kept".into(),
        chosen: String::new(),
    }];
    let out = convert(&mut sc, &db, 0, 1, &items);

    assert_eq!(out.replaced_kinds, 1);
    assert_eq!(out.replaced_blocks, 2);
    // 方块被换掉
    for idx in &sc.regions[0].indices {
        let st = &sc.regions[0].palette[*idx as usize];
        assert_ne!(st.name, "minecraft:gone", "原方块应已被替换");
    }
    // 版本号被改写
    assert_eq!(
        sc.root.get("MinecraftDataVersion"),
        Some(&crate::format::nbt::Value::Int(200))
    );
}

#[test]
fn convert_skips_items_without_replacement() {
    let old = version("old", 100, &["minecraft:gone"]);
    let new = version("new", 200, &["minecraft:kept"]);
    let db = db_with(vec![old, new]);
    let mut sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0], 100);
    let items = vec![ConvertItem {
        name: "minecraft:gone".into(),
        count: 1,
        suggested: String::new(),
        chosen: String::new(),
    }];
    let out = convert(&mut sc, &db, 0, 1, &items);
    assert_eq!(out.replaced_kinds, 0);
    assert_eq!(out.skipped_kinds, 1, "没有替代品应计入未指定");
    // 方块保持原样，但版本号仍然改写
    let idx = sc.regions[0].indices[0] as usize;
    assert_eq!(sc.regions[0].palette[idx].name, "minecraft:gone");
    assert_eq!(
        sc.root.get("MinecraftDataVersion"),
        Some(&crate::format::nbt::Value::Int(200))
    );
}

#[test]
fn chosen_overrides_suggestion() {
    let old = version("old", 100, &["minecraft:gone"]);
    let new = version("new", 200, &["minecraft:kept", "minecraft:picked"]);
    let db = db_with(vec![old, new]);
    let mut sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0], 100);
    let items = vec![ConvertItem {
        name: "minecraft:gone".into(),
        count: 1,
        suggested: "minecraft:kept".into(),
        chosen: "minecraft:picked".into(),
    }];
    convert(&mut sc, &db, 0, 1, &items);
    // 替换会把新状态追加到调色板、并改写索引指向它，
    // 因此必须顺着索引读，不能直接看 palette[0]
    let idx = sc.regions[0].indices[0] as usize;
    assert_eq!(
        sc.regions[0].palette[idx].name, "minecraft:picked",
        "用户指定应优先于推荐"
    );
}

#[test]
fn convert_adds_data_version_when_missing() {
    let old = version("old", 100, &["minecraft:gone"]);
    let new = version("new", 200, &["minecraft:kept"]);
    let db = db_with(vec![old, new]);
    let mut sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0], 100);
    // 移除版本字段，验证会被补上
    if let crate::format::nbt::Value::Compound(f) = &mut sc.root {
        f.retain(|(k, _)| k != "MinecraftDataVersion");
    }
    convert(&mut sc, &db, 0, 1, &[]);
    assert_eq!(
        sc.root.get("MinecraftDataVersion"),
        Some(&crate::format::nbt::Value::Int(200)),
        "缺少版本字段时应补上"
    );
}

#[test]
fn convert_with_unknown_version_does_nothing() {
    let db = db_with(vec![version("old", 100, &["minecraft:gone"])]);
    let mut sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0], 100);
    let items = vec![ConvertItem {
        name: "minecraft:gone".into(),
        count: 1,
        suggested: "minecraft:kept".into(),
        chosen: String::new(),
    }];
    // 目标版本不存在：不能转换，也不能改写版本号
    let out = convert(&mut sc, &db, 0, 9, &items);
    assert_eq!(out.replaced_blocks, 0);
    let idx = sc.regions[0].indices[0] as usize;
    assert_eq!(sc.regions[0].palette[idx].name, "minecraft:gone");
    assert_eq!(
        sc.root.get("MinecraftDataVersion"),
        Some(&crate::format::nbt::Value::Int(100)),
        "目标版本无效时不应改写版本号"
    );
}

#[test]
fn targets_exclude_overlays_and_source() {
    let mut overlay = version("pack", -1, &[]);
    overlay.is_overlay = true;
    let db = db_with(vec![
        version("b", 200, &[]),
        overlay,
        version("a", 100, &[]),
    ]);
    let t = targets(&db, 100);
    assert_eq!(t.len(), 1, "应排除叠加包与源版本");
    assert_eq!(t[0].name, "b");
    assert_eq!(t[0].data_version, 200);
}

/// 没有任何方块被替换时，不应把 blocks_modified 置位。
///
/// 该标志只控制是否重编码 BlockStates；仅改版本号时区块数据没变，
/// 重编码既慢又可能因编码差异改动原文件。
#[test]
fn no_replacement_leaves_blocks_modified_false() {
    let old = version("old", 100, &["minecraft:gone"]);
    let new = version("new", 200, &["minecraft:stone"]);
    let db = db_with(vec![old, new]);
    let mut sc = schematic(vec![BlockState::new("minecraft:gone")], vec![0], 100);
    // 没有可替换项
    let out = convert(&mut sc, &db, 0, 1, &[]);
    assert_eq!(out.replaced_blocks, 0);
    assert!(
        !sc.blocks_modified,
        "未替换任何方块时不应置 blocks_modified"
    );
    // 但版本号仍然改写
    assert_eq!(
        sc.root.get("MinecraftDataVersion"),
        Some(&crate::format::nbt::Value::Int(200))
    );
}
