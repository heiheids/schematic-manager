use super::*;

/// 转换条目必须带上中文名，各级回退都要对。
///
/// 界面上凡是出现方块 ID 的地方都要同时显示中文名，因此这条盯的是
/// 「名字从哪来」：源方块查原版本的表（它在目标版本里已经不存在），
/// 替代品只查目标版本的表，都查不到时退回不带命名空间的方块名，
/// 而不是留一个空串让界面空白。
#[test]
fn convert_items_carry_chinese_names_with_fallbacks() {
    let mut source = std::collections::HashMap::new();
    source.insert(
        "minecraft:oak_stairs".to_string(),
        ("橡木楼梯".to_string(), "Oak Stairs".to_string()),
    );
    // 只有英文名：缺中文名时不能留白
    source.insert(
        "minecraft:old_block".to_string(),
        (String::new(), "Old Block".to_string()),
    );

    let mut target = std::collections::HashMap::new();
    target.insert(
        "minecraft:pale_oak_stairs".to_string(),
        ("苍白橡木楼梯".to_string(), "Pale Oak Stairs".to_string()),
    );

    let items = vec![
        crate::schematic::convert::ConvertItem {
            name: "minecraft:oak_stairs".into(),
            count: 12,
            suggested: "minecraft:pale_oak_stairs".into(),
            chosen: String::new(),
        },
        // 没有推荐：中文名也必须是空，不能凭空编一个
        crate::schematic::convert::ConvertItem {
            name: "minecraft:old_block".into(),
            count: 3,
            suggested: String::new(),
            chosen: String::new(),
        },
        // 两张表都没有：退回方块名本身
        crate::schematic::convert::ConvertItem {
            name: "mymod:unknown_thing".into(),
            count: 1,
            suggested: String::new(),
            chosen: String::new(),
        },
        // 推荐品只存在于原版本的表里：替代品不该借用源表的名字
        crate::schematic::convert::ConvertItem {
            name: "minecraft:old_block".into(),
            count: 1,
            suggested: "minecraft:oak_stairs".into(),
            chosen: String::new(),
        },
        // 源方块在源表里没有、只在目标表里有：必须退到第二张表去找
        crate::schematic::convert::ConvertItem {
            name: "minecraft:pale_oak_stairs".into(),
            count: 2,
            suggested: String::new(),
            chosen: String::new(),
        },
        // 空名字：两个字段都应是空串，不能拼出去命名空间的分支
        crate::schematic::convert::ConvertItem {
            name: String::new(),
            count: 0,
            suggested: String::new(),
            chosen: String::new(),
        },
    ];

    let out = convert_items_with_zh(items, &source, &target);

    assert_eq!(out[0].name_zh, "橡木楼梯");
    assert_eq!(out[0].name_en, "Oak Stairs", "英文名也要一并给出，供英语界面用");
    assert_eq!(out[0].suggested_zh, "苍白橡木楼梯");
    assert_eq!(out[0].suggested_en, "Pale Oak Stairs");
    assert_eq!(
        out[1].name_zh, "",
        "缺中文名时留空，由界面退回英文名并标注缺项，与方块表一致"
    );
    assert_eq!(out[1].name_en, "Old Block", "这一套名字本身是有的");
    assert_eq!(out[1].suggested_zh, "", "没有推荐时中文名必须为空");
    assert_eq!(out[1].suggested_en, "", "没有推荐时英文名必须为空");
    assert_eq!(out[2].name_zh, "unknown_thing", "查不到应退回方块名而不是空串");
    assert_eq!(out[2].name_en, "unknown_thing");
    assert_eq!(
        out[3].suggested_zh, "oak_stairs",
        "替代品只能在目标版本的表里查，不能借用源版本的名字"
    );
    assert_eq!(out[3].suggested_en, "oak_stairs");
    assert_eq!(
        out[4].name_zh, "苍白橡木楼梯",
        "源表里没有时应退到目标表去找"
    );
    assert_eq!(out[4].name_en, "Pale Oak Stairs");
    assert_eq!(out[5].name_zh, "", "空方块名不该拼出一个名字");
    assert_eq!(out[5].name_en, "");
    assert_eq!(out[5].suggested_zh, "");
    assert_eq!(out[5].suggested_en, "");
    assert_eq!(out[0].name, "minecraft:oak_stairs", "原字段不应被改动");
    assert_eq!(out[0].count, 12);
}
