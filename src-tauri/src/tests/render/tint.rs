use super::*;

fn props(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn negative_tintindex_means_no_tint() {
    assert_eq!(tint_color("minecraft:stone", &[], -1), NO_TINT);
    assert_eq!(tint_color("minecraft:oak_planks", &[], -1), NO_TINT);
}

#[test]
fn leaves_get_foliage_green() {
    // 原版橡树树叶在平原是 #77AB2F
    assert_eq!(tint_color("minecraft:oak_leaves", &[], 0), FOLIAGE);
    assert_eq!(tint_color("minecraft:jungle_leaves", &[], 0), FOLIAGE);
    // 云杉与白桦是固定色，不走生物群系
    assert_eq!(tint_color("minecraft:spruce_leaves", &[], 0), SPRUCE);
    assert_eq!(tint_color("minecraft:birch_leaves", &[], 0), BIRCH);
}

#[test]
fn vines_are_foliage_colored() {
    assert_eq!(tint_color("minecraft:vine", &[], 0), FOLIAGE);
}

#[test]
fn cherry_leaves_keep_their_own_pink() {
    // 樱花树叶的贴图本身就是粉色。若按树叶绿染色，粉 × 绿 会得到橄榄色，
    // 与实际观感明显不符，因此这一种必须保持原色。
    assert_eq!(
        tint_color("minecraft:cherry_leaves", &[], 0),
        NO_TINT,
        "樱花树叶不应被染成树叶绿"
    );
    // 同组的其它树叶仍然要染色，别把整类都关掉
    assert_eq!(tint_color("minecraft:oak_leaves", &[], 0), FOLIAGE);
    assert_eq!(tint_color("minecraft:jungle_leaves", &[], 0), FOLIAGE);
    assert_eq!(tint_color("minecraft:mangrove_leaves", &[], 0), FOLIAGE);
}

#[test]
fn grass_is_grass_green_not_foliage() {
    // 下标 0 在草方块上是草绿、在树叶上是叶绿——必须按方块名区分，
    // 否则草方块会长成树叶色
    assert_eq!(tint_color("minecraft:grass_block", &[], 0), GRASS);
    assert_eq!(tint_color("minecraft:short_grass", &[], 0), GRASS);
    assert_eq!(tint_color("minecraft:sugar_cane", &[], 0), GRASS);
    assert_ne!(
        tint_color("minecraft:grass_block", &[], 0),
        tint_color("minecraft:oak_leaves", &[], 0),
        "草与树叶不能同色"
    );
}

#[test]
fn redstone_wire_is_red_and_varies_with_power() {
    let off = tint_color("minecraft:redstone_wire", &props(&[("power", "0")]), 0);
    let on = tint_color("minecraft:redstone_wire", &props(&[("power", "15")]), 0);
    // 必须是红色系：红通道明显高于绿蓝
    for (label, c) in [("power=0", off), ("power=15", on)] {
        assert!(c[0] > c[1] && c[0] > c[2], "{label} 应为红色系，实际 {c:?}");
    }
    // 信号越强越亮
    assert!(on[0] > off[0], "满信号的红色应比无信号更亮");
    // 无信号也不是纯黑（原版 0 档仍能看见线）
    assert!(off[0] > 0.0);
}

#[test]
fn redstone_power_missing_or_bad_is_treated_as_zero() {
    let a = tint_color("minecraft:redstone_wire", &[], 0);
    let b = tint_color("minecraft:redstone_wire", &props(&[("power", "0")]), 0);
    let c = tint_color(
        "minecraft:redstone_wire",
        &props(&[("power", "不是数字")]),
        0,
    );
    assert_eq!(a, b, "缺 power 应按 0 处理");
    assert_eq!(c, b, "非法 power 应按 0 处理");
}

#[test]
fn water_is_blue() {
    let c = tint_color("minecraft:water", &[], 0);
    assert_eq!(c, WATER);
    // 蓝通道最高
    assert!(c[2] > c[0] && c[2] > c[1], "水应为蓝色系，实际 {c:?}");
}

#[test]
fn name_matching_accepts_prefixed_and_bare_names() {
    // 数据库里带 minecraft: 前缀，但不应假设一定带
    assert_eq!(tint_color("oak_leaves", &[], 0), FOLIAGE);
    assert_eq!(tint_color("minecraft:oak_leaves", &[], 0), FOLIAGE);
}

#[test]
fn unknown_tint_index_keeps_original_color() {
    // 原版没有下标 7 对应的默认着色器，不应擅自上色
    assert_eq!(tint_color("minecraft:stone", &[], 7), NO_TINT);
}
