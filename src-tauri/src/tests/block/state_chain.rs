use super::*;
use crate::block::state::{ModelElementDef, ModelFace};

fn elem(y: f32) -> ModelElementDef {
    ModelElementDef {
        from: [0.0, y, 0.0],
        to: [16.0, 16.0, 16.0],
        faces: vec![(
            "north".to_string(),
            ModelFace {
                texture: "#all".to_string(),
                cullface: String::new(),
                tintindex: -1,
                rotation: 0,
                uv: None,
            },
        )],
        rotation: None,
    }
}

/// 构造一个模型。`force` 为该模型全部贴图变量的强制半透明标记。
fn model(parent: &str, textures: &[(&str, &str)], elements: Vec<ModelElementDef>) -> ModelDef {
    model_tex(parent, textures, elements, false)
}

fn model_tex(
    parent: &str,
    textures: &[(&str, &str)],
    elements: Vec<ModelElementDef>,
    force: bool,
) -> ModelDef {
    ModelDef {
        parent: parent.to_string(),
        textures: textures
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string(), force))
            .collect(),
        elements,
        ambientocclusion: true,
    }
}

/// 用一个静态表当模型仓库
fn loader<'a>(
    table: &'a [(&'a str, ModelDef)],
) -> impl FnMut(&str) -> Option<ModelDef> + 'a {
    move |id: &str| table.iter().find(|(k, _)| *k == id).map(|(_, v)| v.clone())
}

#[test]
fn resolve_model_id_adds_current_namespace() {
    assert_eq!(resolve_model_id("block/cube", "minecraft").unwrap(), "minecraft:block/cube");
    assert_eq!(resolve_model_id("mod:block/x", "minecraft").unwrap(), "mod:block/x");
    assert!(resolve_model_id("", "minecraft").is_none());
    assert!(resolve_model_id("mod:", "minecraft").is_none(), "路径为空应拒绝");
}

#[test]
fn model_entry_path_matches_jar_layout() {
    assert_eq!(
        model_entry_path("minecraft:block/cube_all").unwrap(),
        "assets/minecraft/models/block/cube_all.json"
    );
    assert!(model_entry_path("nopath").is_none());
}

#[test]
fn child_textures_override_parent() {
    let table = vec![
        ("minecraft:block/cube", model("", &[("all", "block/iron")], vec![elem(0.0)])),
        ("minecraft:block/door", model("block/cube", &[("all", "block/wood")], vec![])),
    ];
    let mut l = loader(&table);
    let ch = build_chain("block/door", "minecraft", &mut l);
    assert_eq!(
        ch.textures.get("all").unwrap().sprite, "block/wood",
        "子级应覆盖父级同名变量"
    );
}

/// 链级的 force_translucent 必须由 build_chain 累积出来。
///
/// 此前 block/state.rs 解析时整段丢掉了这个字段，TexValue 恒为 false，
/// 于是下游的 is_force_translucent 永远返回 false，特性静默失效。
/// 这条同时覆盖「解析结果被带入链」这一环。
#[test]
fn chain_translucent_is_accumulated_from_model_textures() {
    let table = vec![(
        "minecraft:block/glass",
        model_tex("", &[("all", "block/glass")], vec![elem(0.0)], true),
    )];
    let mut l = loader(&table);
    let ch = build_chain("block/glass", "minecraft", &mut l);
    assert!(
        ch.force_translucent,
        "模型声明了 force_translucent，链上标志应为真"
    );
    assert!(
        ch.textures.get("all").unwrap().force_translucent,
        "对应变量的标记也应为真"
    );
}

/// 普通模型不应把链级标志置位。
#[test]
fn chain_translucent_stays_false_for_plain_model() {
    let table = vec![(
        "minecraft:block/stone",
        model("", &[("all", "block/stone")], vec![elem(0.0)]),
    )];
    let mut l = loader(&table);
    let ch = build_chain("block/stone", "minecraft", &mut l);
    assert!(!ch.force_translucent, "普通模型不应被判为半透明");
}

/// 父级声明过、子级未声明时，链级标志仍应为真。
///
/// 「链上任意一级声明过即为真」是本模块写在文件头的约定，
/// 子级覆盖贴图变量时不能把它抹掉。
#[test]
fn chain_translucent_survives_child_override() {
    let table = vec![
        (
            "minecraft:block/cube",
            model_tex("", &[("all", "block/glass")], vec![elem(0.0)], true),
        ),
        // 子级覆盖了 all，但自身没有 force_translucent
        ("minecraft:block/door", model("block/cube", &[("all", "block/wood")], vec![])),
    ];
    let mut l = loader(&table);
    let ch = build_chain("block/door", "minecraft", &mut l);
    assert!(
        ch.force_translucent,
        "父级声明过，子级覆盖贴图后链级标志仍应为真"
    );
}

#[test]
fn parent_textures_are_inherited_when_not_overridden() {
    let table = vec![
        (
            "minecraft:block/cube",
            model("", &[("all", "block/iron"), ("particle", "block/iron")], vec![elem(0.0)]),
        ),
        ("minecraft:block/door", model("block/cube", &[("all", "block/wood")], vec![])),
    ];
    let mut l = loader(&table);
    let ch = build_chain("block/door", "minecraft", &mut l);
    assert_eq!(ch.textures.get("all").unwrap().sprite, "block/wood");
    assert_eq!(
        ch.textures.get("particle").unwrap().sprite, "block/iron",
        "未被覆盖的父级变量应继承"
    );
}

#[test]
fn elements_come_from_nearest_level_that_has_them() {
    let table = vec![
        ("minecraft:block/base", model("", &[], vec![elem(1.0)])),
        ("minecraft:block/mid", model("block/base", &[], vec![])),
        ("minecraft:block/leaf", model("block/mid", &[], vec![])),
    ];
    let mut l = loader(&table);
    let ch = build_chain("block/leaf", "minecraft", &mut l);
    assert_eq!(ch.elements.len(), 1);
    assert_eq!(ch.elements[0].from[1], 1.0, "应取到 base 的 elements");
}

#[test]
fn child_elements_replace_parent_entirely() {
    let table = vec![
        ("minecraft:block/base", model("", &[], vec![elem(1.0), elem(2.0)])),
        ("minecraft:block/leaf", model("block/base", &[], vec![elem(9.0)])),
    ];
    let mut l = loader(&table);
    let ch = build_chain("block/leaf", "minecraft", &mut l);
    assert_eq!(ch.elements.len(), 1, "子级有 elements 时应整体替换，不逐元素合并");
    assert_eq!(ch.elements[0].from[1], 9.0);
}

#[test]
fn missing_parent_is_tolerated() {
    // 父模型不存在时按无父级处理，本级的定义仍然有效
    let table = vec![(
        "minecraft:block/orphan",
        model("block/does_not_exist", &[("all", "block/x")], vec![elem(3.0)]),
    )];
    let mut l = loader(&table);
    let ch = build_chain("block/orphan", "minecraft", &mut l);
    assert_eq!(ch.textures.get("all").unwrap().sprite, "block/x");
    assert_eq!(ch.elements.len(), 1);
}

#[test]
fn missing_start_model_yields_empty_chain() {
    let table: Vec<(&str, ModelDef)> = Vec::new();
    let mut l = loader(&table);
    let ch = build_chain("block/nope", "minecraft", &mut l);
    assert!(ch.textures.is_empty());
    assert!(ch.elements.is_empty());
}

#[test]
fn cyclic_parent_terminates() {
    // A 继承 B、B 继承 A：不能无限递归
    let table = vec![
        ("minecraft:a", model("b", &[("x", "block/1")], vec![])),
        ("minecraft:b", model("a", &[("y", "block/2")], vec![])),
    ];
    let mut l = loader(&table);
    let ch = build_chain("a", "minecraft", &mut l);

    // 断言确切结果，而不是「x 或 y 至少有一个」这种析取式——
    // 后者在只取到一半数据时也会通过，等于没测。
    //
    // 成环的处理方式：递归到再次遇到 "a" 时返回空链，但外层两级
    // 各自的 textures 仍会正常合并，因此最终两级都拿到。
    // 这既是确定行为，也是合理的尽力而为：宁可给出部分数据，
    // 也不因为一个环就整条链作废。
    assert_eq!(
        ch.textures.get("x").map(|v| v.sprite.as_str()),
        Some("block/1"),
        "A 自己的贴图变量应保留"
    );
    assert_eq!(
        ch.textures.get("y").map(|v| v.sprite.as_str()),
        Some("block/2"),
        "B 的贴图变量也应合并进来"
    );
    assert_eq!(ch.textures.len(), 2, "不应多出别的条目");
}

#[test]
fn deep_chain_terminates_at_limit() {
    // 构造超过上限的链：既要能终止，也要真的在上限处截断。
    //
    // 每一级用**不同的贴图变量名**，这样「递归到第几级」可以从结果里
    // 数出来。若各级同名（原写法都是 "all"），截断与不截断的合并结果
    // 完全一样，测试就只能证明「不 panic」，证明不了上限生效。
    let n = MAX_DEPTH + 5;
    let mut table: Vec<(String, ModelDef)> = Vec::new();
    for i in 0..n {
        // 直接构造 ModelDef，以便用动态生成的变量名（辅助函数只收 &str）
        let parent = if i + 1 < n { format!("block/m{}", i + 1) } else { String::new() };
        table.push((
            format!("minecraft:block/m{i}"),
            ModelDef {
                parent,
                textures: vec![(format!("k{i}"), format!("block/{i}"), false)],
                elements: Vec::new(),
                ambientocclusion: true,
            },
        ));
    }
    let pairs: Vec<(&str, ModelDef)> = table.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    let mut l = loader(&pairs);
    let ch = build_chain("block/m0", "minecraft", &mut l);

    // build_inner 在 depth > MAX_DEPTH 时返回空链；depth 从 0 起算，
    // 因此实际处理的是 m0..=m{MAX_DEPTH}，共 MAX_DEPTH + 1 级。
    assert_eq!(
        ch.textures.len(),
        MAX_DEPTH + 1,
        "应恰好处理 MAX_DEPTH + 1（{}）级，实际 {} 级",
        MAX_DEPTH + 1,
        ch.textures.len()
    );
    assert!(ch.textures.contains_key("k0"), "第 0 级应在结果里");
    assert!(
        ch.textures.contains_key(&format!("k{MAX_DEPTH}")),
        "第 {MAX_DEPTH} 级（最后一个在范围内的）应在结果里"
    );
    assert!(
        !ch.textures.contains_key(&format!("k{}", MAX_DEPTH + 1)),
        "第 {} 级超出上限，不应出现在结果里",
        MAX_DEPTH + 1
    );
    assert!(
        !ch.textures.contains_key(&format!("k{}", n - 1)),
        "最末一级（深度 {}）远离上限，不应出现",
        n - 1
    );
}

#[test]
fn resolves_variable_to_literal_path() {
    let mut t = HashMap::new();
    t.insert(
        "all".to_string(),
        TexValue { sprite: "minecraft:block/stone".into(), force_translucent: false },
    );
    assert_eq!(
        resolve_tex_ref(&t, "#all").unwrap(),
        "minecraft:block/stone"
    );
}

#[test]
fn resolves_chained_variables() {
    let mut t = HashMap::new();
    t.insert("a".into(), TexValue { sprite: "#b".into(), force_translucent: false });
    t.insert("b".into(), TexValue { sprite: "#c".into(), force_translucent: false });
    t.insert("c".into(), TexValue { sprite: "block/final".into(), force_translucent: false });
    assert_eq!(resolve_tex_ref(&t, "#a").unwrap(), "block/final");
}

#[test]
fn literal_path_is_returned_as_is() {
    let t = HashMap::new();
    assert_eq!(resolve_tex_ref(&t, "block/stone").unwrap(), "block/stone");
}

#[test]
fn variable_cycle_is_rejected() {
    let mut t = HashMap::new();
    t.insert("a".into(), TexValue { sprite: "#b".into(), force_translucent: false });
    t.insert("b".into(), TexValue { sprite: "#a".into(), force_translucent: false });
    assert!(resolve_tex_ref(&t, "#a").is_none(), "成环应返回 None");
}

#[test]
fn unknown_variable_is_rejected() {
    let t = HashMap::new();
    assert!(resolve_tex_ref(&t, "#missing").is_none());
}

#[test]
fn empty_sprite_is_rejected() {
    let mut t = HashMap::new();
    t.insert("a".into(), TexValue { sprite: String::new(), force_translucent: false });
    assert!(resolve_tex_ref(&t, "#a").is_none(), "指向空值应拒绝");
}

#[test]
fn self_reference_is_rejected() {
    let mut t = HashMap::new();
    t.insert("a".into(), TexValue { sprite: "#a".into(), force_translucent: false });
    assert!(resolve_tex_ref(&t, "#a").is_none(), "自引用应被识别为成环");
}

#[test]
fn force_translucent_is_found_along_chain() {
    let mut t = HashMap::new();
    t.insert("a".into(), TexValue { sprite: "#b".into(), force_translucent: false });
    t.insert("b".into(), TexValue { sprite: "block/glass".into(), force_translucent: true });
    assert!(is_force_translucent(&t, "#a"), "链上任意一级声明即应为真");
}

#[test]
fn force_translucent_false_for_plain_texture() {
    let mut t = HashMap::new();
    t.insert("a".into(), TexValue { sprite: "block/stone".into(), force_translucent: false });
    assert!(!is_force_translucent(&t, "#a"));
    // 字面路径无从判断，按否处理
    assert!(!is_force_translucent(&t, "block/stone"));
}

#[test]
fn force_translucent_cycle_terminates() {
    let mut t = HashMap::new();
    t.insert("a".into(), TexValue { sprite: "#b".into(), force_translucent: false });
    t.insert("b".into(), TexValue { sprite: "#a".into(), force_translucent: false });
    assert!(!is_force_translucent(&t, "#a"), "成环时不应死循环");
}

/// 模型 id 对应的 jar 内条目路径
pub fn model_entry_path(id: &str) -> Option<String> {
    let (ns, path) = id.split_once(':')?;
    if ns.is_empty() || path.is_empty() {
        return None;
    }
    Some(format!("assets/{ns}/models/{path}.json"))
}
