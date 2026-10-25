use super::*;
use crate::schematic::litematic::{BlockState, Region};

/// 一个只认识楼梯类方块的方块库替身
struct Stairs {
    props: Vec<(String, String)>,
}

impl BlockInfo for Stairs {
    fn supports_prop(&self, block: &str, prop: &str, value: &str) -> bool {
        if !block.starts_with("minecraft:") || !block.contains("stairs") {
            return false;
        }
        match prop {
            "facing" => ["north", "south", "east", "west"].contains(&value),
            "half" => ["top", "bottom"].contains(&value),
            "shape" => ["straight", "inner_left", "outer_left"].contains(&value),
            _ => false,
        }
    }
    fn default_props(&self, block: &str) -> Vec<(String, String)> {
        if block.contains("stairs") {
            vec![("facing".into(), "north".into()), ("half".into(), "bottom".into())]
        } else {
            self.props.clone()
        }
    }
}

fn info() -> Stairs {
    Stairs { props: Vec::new() }
}

fn schematic_with(palette: Vec<BlockState>, indices: Vec<i32>) -> Schematic {
    Schematic {
        root: crate::format::nbt::Value::Compound(vec![]),
        regions: vec![Region {
            name: "r".into(),
            pos: [0, 0, 0],
            size: [indices.len() as i32, 1, 1],
            volume: indices.len(),
            bits: 2,
            palette,
            indices,
            ..Default::default()
        }],
        path: String::new(),
        meta_modified: false,
        blocks_modified: false,
    }
}

fn rule(from: &str, to: &str) -> ReplaceRule {
    ReplaceRule {
        from_name: from.into(),
        to_name: to.into(),
        inherit_props: true,
        ..Default::default()
    }
}

#[test]
fn replaces_palette_and_remaps_indices() {
    let mut sc = schematic_with(
        vec![BlockState::new("minecraft:stone"), BlockState::new("minecraft:dirt")],
        vec![0, 1, 0, 1, 0],
    );
    let out = replace_blocks(&mut sc, &rule("minecraft:stone", "minecraft:gold_block"), &info(), 0);

    assert_eq!(out.replaced, 3, "三个石头实例");
    assert!(sc.blocks_modified, "发生替换后应置改动标志");
    let r = &sc.regions[0];
    for (i, idx) in r.indices.iter().enumerate() {
        let want = if i % 2 == 0 { "minecraft:gold_block" } else { "minecraft:dirt" };
        assert_eq!(r.palette[*idx as usize].name, want, "第 {i} 个");
    }
}

#[test]
fn non_matching_blocks_are_untouched() {
    let mut sc = schematic_with(vec![BlockState::new("minecraft:dirt")], vec![0, 0]);
    let out = replace_blocks(&mut sc, &rule("minecraft:stone", "minecraft:gold_block"), &info(), 0);
    assert_eq!(out.replaced, 0);
    assert!(!sc.blocks_modified);
    assert_eq!(sc.regions[0].palette[0].name, "minecraft:dirt");
}

#[test]
fn from_props_narrow_the_match() {
    let mut east = BlockState::new("minecraft:oak_stairs");
    east.set_prop("facing", "east");
    let mut west = BlockState::new("minecraft:oak_stairs");
    west.set_prop("facing", "west");
    let mut sc = schematic_with(vec![east, west], vec![0, 1, 0]);

    let mut r = rule("minecraft:oak_stairs", "minecraft:spruce_stairs");
    r.from_props = vec![("facing".into(), "east".into())];
    let out = replace_blocks(&mut sc, &r, &info(), 0);

    assert_eq!(out.replaced, 2, "只有朝东的两处命中");
    let reg = &sc.regions[0];
    assert_eq!(reg.palette[reg.indices[1] as usize].name, "minecraft:oak_stairs", "朝西的不变");
}

#[test]
fn supported_props_are_inherited() {
    let mut src = BlockState::new("minecraft:oak_stairs");
    src.set_prop("facing", "east");
    src.set_prop("half", "top");
    let mut sc = schematic_with(vec![src], vec![0]);

    replace_blocks(&mut sc, &rule("minecraft:oak_stairs", "minecraft:spruce_stairs"), &info(), 0);
    let st = &sc.regions[0].palette[sc.regions[0].indices[0] as usize];
    assert_eq!(st.name, "minecraft:spruce_stairs");
    assert_eq!(st.prop("facing"), Some("east"), "目标支持的属性应继承");
    assert_eq!(st.prop("half"), Some("top"));
}

#[test]
fn unsupported_props_are_dropped_and_defaults_filled() {
    // 源是楼梯（有 facing/half），目标是石头（都不支持）
    let mut src = BlockState::new("minecraft:oak_stairs");
    src.set_prop("facing", "east");
    src.set_prop("half", "top");
    let mut sc = schematic_with(vec![src], vec![0]);

    replace_blocks(&mut sc, &rule("minecraft:oak_stairs", "minecraft:stone"), &info(), 0);
    let st = &sc.regions[0].palette[sc.regions[0].indices[0] as usize];
    assert_eq!(st.name, "minecraft:stone");
    assert!(st.prop("facing").is_none(), "目标不支持的属性应丢弃");
    assert!(st.prop("half").is_none());
}

#[test]
fn to_props_override_inherited_values() {
    let mut src = BlockState::new("minecraft:oak_stairs");
    src.set_prop("facing", "east");
    let mut sc = schematic_with(vec![src], vec![0]);

    let mut r = rule("minecraft:oak_stairs", "minecraft:spruce_stairs");
    r.to_props = vec![("facing".into(), "west".into())];
    replace_blocks(&mut sc, &r, &info(), 0);

    let st = &sc.regions[0].palette[sc.regions[0].indices[0] as usize];
    assert_eq!(st.prop("facing"), Some("west"), "强制属性应覆盖继承值");
}

#[test]
fn limit_caps_replaced_blocks() {
    let mut sc = schematic_with(vec![BlockState::new("minecraft:stone")], vec![0, 0, 0, 0, 0]);
    let out = replace_blocks(&mut sc, &rule("minecraft:stone", "minecraft:gold_block"), &info(), 2);
    assert_eq!(out.replaced, 2, "最多替换 2 个");
}

#[test]
fn equivalent_result_is_not_counted() {
    // 替换成同一状态不应产生改动
    let mut sc = schematic_with(vec![BlockState::new("minecraft:stone")], vec![0, 0]);
    let out = replace_blocks(&mut sc, &rule("minecraft:stone", "minecraft:stone"), &info(), 0);
    assert_eq!(out.replaced, 0);
    assert!(!sc.blocks_modified, "无实际变化时不应置改动标志");
}

#[test]
fn count_matches_agrees_with_actual_replacement() {
    let mut sc = schematic_with(
        vec![BlockState::new("minecraft:stone"), BlockState::new("minecraft:dirt")],
        vec![0, 1, 0, 1, 0],
    );
    let r = rule("minecraft:stone", "minecraft:gold_block");
    let predicted = count_matches(&sc, &r, &info());
    assert_eq!(predicted, 3);

    // 预测值必须与实际替换数一致，否则界面上的预览会误导用户
    let out = replace_blocks(&mut sc, &r, &info(), 0);
    assert_eq!(out.replaced, predicted);
}

#[test]
fn count_matches_is_zero_when_nothing_changes() {
    let mut sc = schematic_with(vec![BlockState::new("minecraft:stone")], vec![0, 0, 0]);
    // 替换成同一状态：命中但无实际变化，应计为 0
    assert_eq!(count_matches(&sc, &rule("minecraft:stone", "minecraft:stone"), &info()), 0);

    let out = replace_blocks(&mut sc, &rule("minecraft:stone", "minecraft:stone"), &info(), 0);
    assert_eq!(out.replaced, 0, "预测与实际应一致");
}

#[test]
fn count_matches_handles_multiple_regions() {
    let mut sc = schematic_with(vec![BlockState::new("minecraft:stone")], vec![0, 0]);
    let mut second = sc.regions[0].clone();
    second.name = "r2".into();
    second.indices = vec![0, 0, 0];
    sc.regions.push(second);
    assert_eq!(count_matches(&sc, &rule("minecraft:stone", "minecraft:gold_block"), &info()), 5);
}

#[test]
fn reuse_existing_palette_entry() {
    // 目标状态已在调色板中时，应复用而不是追加新项
    let mut sc = schematic_with(
        vec![BlockState::new("minecraft:stone"), BlockState::new("minecraft:gold_block")],
        vec![0, 0],
    );
    replace_blocks(&mut sc, &rule("minecraft:stone", "minecraft:gold_block"), &info(), 0);
    assert_eq!(sc.regions[0].palette.len(), 2, "调色板不应膨胀");
    assert_eq!(sc.regions[0].indices, vec![1, 1]);
}
