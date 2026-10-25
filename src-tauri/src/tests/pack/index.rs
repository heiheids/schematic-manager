use super::*;

use std::collections::BTreeSet;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn indexes_blockstates_models_and_textures() {
    let entries = s(&[
        "assets/minecraft/blockstates/stone.json",
        "assets/minecraft/models/block/stone.json",
        "assets/minecraft/textures/block/stone.png",
    ]);
    let idx = build(&entries);
    let ns = idx.namespaces.get("minecraft").unwrap();
    assert_eq!(ns.blockstates, s(&["assets/minecraft/blockstates/stone.json"]));
    assert_eq!(
        ns.models.get("minecraft:block/stone").unwrap(),
        "assets/minecraft/models/block/stone.json"
    );
    assert_eq!(ns.textures, s(&["assets/minecraft/textures/block/stone.png"]));
}

#[test]
fn model_id_map_includes_unqualified_short_name_for_minecraft() {
    // parent 常写作 "block/cube"，不带命名空间，因此短名必须可查
    let entries = s(&["assets/minecraft/models/block/cube.json"]);
    let idx = build(&entries);
    let ns = idx.namespaces.get("minecraft").unwrap();
    assert_eq!(
        ns.models.get("block/cube").unwrap(),
        "assets/minecraft/models/block/cube.json"
    );
    assert!(ns.models.contains_key("minecraft:block/cube"));
}

#[test]
fn non_minecraft_namespace_has_no_short_name() {
    // 短名只对 minecraft 有意义，其他命名空间补前缀会歧义
    let entries = s(&["assets/mymod/models/block/x.json"]);
    let idx = build(&entries);
    let ns = idx.namespaces.get("mymod").unwrap();
    assert!(ns.models.contains_key("mymod:block/x"));
    assert!(!ns.models.contains_key("block/x"));
}

#[test]
fn ignores_unrelated_entries() {
    let entries = s(&[
        "META-INF/MANIFEST.MF",
        "pack.mcmeta",
        "assets/",
        "assets/minecraft/",
        "assets/minecraft/sounds/a.ogg",
        "assets/minecraft/lang/en_us.json",
        "assets/minecraft/textures/block/stone.png.bak",
    ]);
    let idx = build(&entries);
    // 只有 textures 目录下的 .png 才算，上面都不满足
    assert!(idx.is_empty(), "实际 {:?}", idx.namespaces);
}

#[test]
fn textures_outside_textures_dir_are_ignored() {
    let entries = s(&[
        "assets/minecraft/blockstates/x.png",
        "assets/minecraft/models/a.png",
    ]);
    let idx = build(&entries);
    assert!(idx.is_empty());
}

#[test]
fn only_json_under_blockstates_and_models_counts() {
    let entries = s(&[
        "assets/minecraft/blockstates/x.txt",
        "assets/minecraft/models/y.json.txt",
    ]);
    let idx = build(&entries);
    assert!(idx.is_empty(), "扩展名不符不应计入");
}

#[test]
fn multiple_namespaces_are_separated() {
    let entries = s(&[
        "assets/minecraft/blockstates/stone.json",
        "assets/mymod/blockstates/thing.json",
        "assets/mymod/models/block/thing.json",
    ]);
    let idx = build(&entries);
    assert_eq!(idx.blockstate_namespaces(), vec!["minecraft", "mymod"]);
    assert_eq!(idx.namespaces.len(), 2);
    // 各命名空间互不干扰
    assert!(idx.namespaces["mymod"].models.contains_key("mymod:block/thing"));
    assert!(!idx.namespaces["mymod"].models.contains_key("minecraft:block/stone"));
}

#[test]
fn blockstate_namespaces_only_lists_those_with_blockstates() {
    // 只有贴图的命名空间不应被当作可构建的方块来源
    let entries = s(&[
        "assets/onlytextures/textures/block/x.png",
        "assets/minecraft/blockstates/a.json",
    ]);
    let idx = build(&entries);
    assert_eq!(idx.blockstate_namespaces(), vec!["minecraft"]);
    assert!(idx.namespaces.contains_key("onlytextures"), "但其资源仍被索引");
}

#[test]
fn output_is_independent_of_input_order() {
    let a = s(&[
        "assets/minecraft/blockstates/z.json",
        "assets/minecraft/blockstates/a.json",
        "assets/minecraft/textures/block/z.png",
        "assets/minecraft/textures/block/a.png",
    ]);
    let mut b = a.clone();
    b.reverse();
    let ia = build(&a);
    let ib = build(&b);
    assert_eq!(ia, ib, "排序后结果应与输入顺序无关");
    assert_eq!(
        ia.namespaces["minecraft"].blockstates[0],
        "assets/minecraft/blockstates/a.json"
    );
}

#[test]
fn texture_path_strips_prefix_and_extension() {
    assert_eq!(
        texture_path("assets/minecraft/textures/block/stone.png").unwrap(),
        "block/stone"
    );
    // 嵌套目录也要保留
    assert_eq!(
        texture_path("assets/mymod/textures/block/deep/x.png").unwrap(),
        "block/deep/x"
    );
    assert!(texture_path("assets/minecraft/models/block/x.json").is_none());
    assert!(texture_path("assets/minecraft/textures/a.gif").is_none());
}

#[test]
fn uppercase_png_extension_is_accepted() {
    // 有些资源包用大写扩展名
    let entries = s(&["assets/minecraft/textures/block/x.PNG"]);
    let idx = build(&entries);
    assert_eq!(idx.namespaces["minecraft"].textures.len(), 1);
    assert_eq!(
        texture_path("assets/minecraft/textures/block/x.PNG").unwrap(),
        "block/x"
    );
}

#[test]
fn qualify_adds_current_namespace() {
    assert_eq!(qualify("block/stone", "minecraft").unwrap(), "minecraft:block/stone");
    assert_eq!(qualify("mymod:block/x", "minecraft").unwrap(), "mymod:block/x");
    assert!(qualify("", "minecraft").is_none());
    assert!(qualify("mymod:", "minecraft").is_none(), "空路径应拒绝");
}

#[test]
fn namespaces_collects_all() {
    let entries = s(&[
        "assets/a/blockstates/x.json",
        "assets/b/textures/block/y.png",
        "META-INF/x",
    ]);
    let ns = namespaces(&entries);
    assert_eq!(ns.len(), 2);
    assert!(ns.contains("a") && ns.contains("b"));
}

#[test]
fn empty_input_yields_empty_index() {
    let idx = build(&[]);
    assert!(idx.is_empty());
    assert!(idx.blockstate_namespaces().is_empty());
    assert!(build(&s(&["nothing/here"])).is_empty());
}

/// 把贴图条目名转成资源路径（去掉 `assets/<ns>/textures/` 前缀与扩展名）。
///
/// blockstate 与 model 里引用贴图用的就是这个形式，例如
/// `assets/minecraft/textures/block/stone.png` -> `block/stone`。
pub fn texture_path(entry: &str) -> Option<String> {
    let ns = namespace_of(entry)?;
    let prefix = format!("assets/{ns}/textures/");
    let rest = entry.strip_prefix(&prefix)?;
    let stem = rest.strip_suffix(".png").or_else(|| rest.strip_suffix(".PNG"))?;
    if stem.is_empty() {
        return None;
    }
    Some(stem.to_string())
}

/// 收集全部命名空间（含只有贴图没有 blockstates 的）
pub fn namespaces(entries: &[String]) -> BTreeSet<String> {
    entries.iter().filter_map(|e| namespace_of(e)).map(|s| s.to_string()).collect()
}
