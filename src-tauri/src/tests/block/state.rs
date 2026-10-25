use super::*;

#[test]
fn when_dict_is_sorted_and_joined_by_comma() {
    // 键顺序不同的两份 JSON 必须得到相同条件串
    let a: Value = serde_json::from_str(r#"{"half":"top","facing":"east"}"#).unwrap();
    let b: Value = serde_json::from_str(r#"{"facing":"east","half":"top"}"#).unwrap();
    assert_eq!(normalize_when_dict(&a), "facing=east,half=top");
    assert_eq!(normalize_when_dict(&a), normalize_when_dict(&b));
}

#[test]
fn when_dict_converts_bool_and_number() {
    let v: Value = serde_json::from_str(r#"{"north":true,"level":3}"#).unwrap();
    assert_eq!(normalize_when_dict(&v), "level=3,north=true");
}

#[test]
fn when_dict_ignores_non_scalar_values() {
    let v: Value = serde_json::from_str(r#"{"a":"x","b":[1,2],"c":null}"#).unwrap();
    assert_eq!(normalize_when_dict(&v), "a=x");
}

#[test]
fn multipart_or_joins_with_pipe() {
    let v: Value = serde_json::from_str(
        r#"{"OR":[{"north":"true"},{"south":"true"}]}"#,
    )
    .unwrap();
    assert_eq!(normalize_when_multipart(&v), "north=true|south=true");
}

#[test]
fn multipart_and_joins_with_comma() {
    let v: Value =
        serde_json::from_str(r#"{"AND":[{"a":"1"},{"b":"2"}]}"#).unwrap();
    assert_eq!(normalize_when_multipart(&v), "a=1,b=2");
}

#[test]
fn nested_or_and_is_flattened() {
    let v: Value = serde_json::from_str(
        r#"{"OR":[{"AND":[{"a":"1"},{"b":"2"}]},{"c":"3"}]}"#,
    )
    .unwrap();
    assert_eq!(normalize_when_multipart(&v), "a=1,b=2|c=3");
}

#[test]
fn or_with_unconditional_alternative_is_unconditional() {
    // 一个备选无条件时整体必然成立，必须返回空串而不是带条件的表示
    let v: Value = serde_json::from_str(r#"{"OR":[{},{"a":"1"}]}"#).unwrap();
    assert_eq!(normalize_when_multipart(&v), "");
}

#[test]
fn plain_object_falls_back_to_dict_form() {
    let v: Value = serde_json::from_str(r#"{"a":"1"}"#).unwrap();
    assert_eq!(normalize_when_multipart(&v), "a=1");
}

#[test]
fn null_when_is_unconditional() {
    let v: Value = Value::Null;
    assert_eq!(normalize_when_multipart(&v), "");
}

#[test]
fn parses_variants_form() {
    let json = br##"{
        "variants": {
            "":            { "model": "block/stone" },
            "facing=east": { "model": "block/stairs", "y": 90, "uvlock": true }
        }
    }"##;
    let def = parse_blockstate(json).unwrap();
    assert_eq!(def.variants.len(), 2);
    assert!(def.multipart.is_empty());
    let east = def.variants.iter().find(|v| v.when == "facing=east").unwrap();
    assert_eq!(east.model, "block/stairs");
    // JSON 里 y=90（角度），字段语义是「90 度倍数」，因此存 1
    assert_eq!(east.rot_y_div90, 1);
    assert!(east.uvlock);
}

#[test]
fn rotation_is_stored_as_multiples_of_90() {
    // 四个朝向都要正确换算，否则写出后旋转角会整体错位
    for (deg, want) in [(0u64, 0u8), (90, 1), (180, 2), (270, 3)] {
        let json = format!(
            r#"{{"variants":{{"":{{"model":"block/x","x":{deg},"y":{deg}}}}}}}"#
        );
        let def = parse_blockstate(json.as_bytes()).unwrap();
        assert_eq!(
            def.variants[0].rot_x_div90, want,
            "x={deg} 度应存为 {want} 个 90 度"
        );
        assert_eq!(
            def.variants[0].rot_y_div90, want,
            "y={deg} 度应存为 {want} 个 90 度"
        );
    }
    // 字段缺失时按 0
    let def = parse_blockstate(br##"{"variants":{"":{"model":"block/x"}}}"##).unwrap();
    assert_eq!(def.variants[0].rot_x_div90, 0);
    assert_eq!(def.variants[0].rot_y_div90, 0);
    // 非 90 的倍数属于异常数据：写出格式只支持 4 个朝向，
    // 取模会得到看似合法却错误的角度，因此按 0 处理
    let def = parse_blockstate(br##"{"variants":{"":{"model":"block/x","y":45}}}"##).unwrap();
    assert_eq!(def.variants[0].rot_y_div90, 0, "45 度不是 90 的倍数，按 0 处理");
}

#[test]
fn variants_value_may_be_an_array() {
    // 同一条件对应多个模型（随机模型）时是数组
    let json = br##"{"variants":{"":{"model":"a"}}}"##;
    let def = parse_blockstate(json).unwrap();
    assert_eq!(def.variants.len(), 1);

    let json2 = br##"{"variants":{"":{"model":"a"}}}"##;
    assert_eq!(parse_blockstate(json2).unwrap().variants[0].model, "a");

    let arr = br##"{"variants":{"":[{"model":"a"},{"model":"b"}]}}"##;
    let d2 = parse_blockstate(arr).unwrap();
    assert_eq!(d2.variants.len(), 2, "数组形式应展开为多条");
    assert_eq!(d2.variants[0].model, "a");
    assert_eq!(d2.variants[1].model, "b");
}

#[test]
fn parses_multipart_form_with_nested_when() {
    let json = br##"{
        "multipart": [
            { "apply": { "model": "block/fence_post" } },
            { "when": { "north": "true" }, "apply": { "model": "block/fence_side" } }
        ]
    }"##;
    let def = parse_blockstate(json).unwrap();
    assert!(def.variants.is_empty());
    assert_eq!(def.multipart.len(), 2);
    assert_eq!(def.multipart[0].when, "", "缺 when 表示无条件");
    assert_eq!(def.multipart[1].when, "north=true");
}

#[test]
fn multipart_apply_may_be_an_array() {
    let json = br##"{
        "multipart": [
            { "when": {"a":"1"}, "apply": [{"model":"m1","y":90},{"model":"m2"}] }
        ]
    }"##;
    let def = parse_blockstate(json).unwrap();
    assert_eq!(def.multipart.len(), 2);
    assert_eq!(def.multipart[0].model, "m1");
    assert_eq!(def.multipart[0].rot_y_div90, 1, "y=90 度应存为 1 个 90 度");
    assert_eq!(def.multipart[1].model, "m2");
    assert_eq!(def.multipart[0].when, def.multipart[1].when);
}

#[test]
fn multipart_accepts_legacy_model_key() {
    // 早期版本用 model 而非 apply
    let json = br##"{"multipart":[{"model":"m1"}]}"##;
    let def = parse_blockstate(json).unwrap();
    assert_eq!(def.multipart.len(), 1);
    assert_eq!(def.multipart[0].model, "m1");
}

#[test]
fn variant_without_model_is_an_error() {
    let json = br##"{"variants":{"":{"x":90}}}"##;
    let e = parse_blockstate(json).unwrap_err().to_string();
    assert!(e.contains("model"), "应指出缺少 model：{e}");
}

#[test]
fn invalid_json_is_an_error() {
    let e = parse_blockstate(b"{not json").unwrap_err().to_string();
    assert!(e.contains("JSON"), "应指出 JSON 解析失败：{e}");
}

#[test]
fn parses_model_with_elements_and_textures() {
    let json = br##"{
        "parent": "block/cube_all",
        "textures": { "all": "block/stone", "particle": "block/stone" },
        "elements": [
            {
                "from": [0, 0, 0],
                "to": [16, 16, 16],
                "faces": {
                    "north": { "texture": "#all", "cullface": "north", "tintindex": 0, "rotation": 90, "uv": [0,0,16,16] }
                }
            }
        ]
    }"##;
    let m = parse_model(json).unwrap();
    assert_eq!(m.parent, "block/cube_all");
    assert_eq!(m.textures.len(), 2);
    assert_eq!(m.elements.len(), 1);
    let e = &m.elements[0];
    assert_eq!(e.from, [0.0, 0.0, 0.0]);
    assert_eq!(e.to, [16.0, 16.0, 16.0]);
    assert_eq!(e.faces.len(), 1);
    assert_eq!(e.faces[0].0, "north");
    assert_eq!(e.faces[0].1.texture, "#all");
    assert_eq!(e.faces[0].1.cullface, "north");
    assert_eq!(e.faces[0].1.tintindex, 0);
    assert_eq!(e.faces[0].1.rotation, 90);
    assert_eq!(e.faces[0].1.uv, Some([0.0, 0.0, 16.0, 16.0]));
}

#[test]
fn model_defaults_when_fields_absent() {
    let json = br##"{"textures":{"all":"block/stone"}}"##;
    let m = parse_model(json).unwrap();
    assert_eq!(m.parent, "");
    assert!(m.elements.is_empty(), "无 elements 的模型（只继承 parent）应可解析");
    assert!(m.ambientocclusion, "ambientocclusion 缺省为 true");
}

#[test]
fn model_without_elements_keeps_parent() {
    // 只声明 parent 与部分贴图是常见写法，不应报错
    let json = br##"{"parent":"block/cube","textures":{"side":"block/x"}}"##;
    let m = parse_model(json).unwrap();
    assert_eq!(m.parent, "block/cube");
}

#[test]
fn model_element_rotation_single_axis() {
    let json = br##"{"elements":[{
        "from":[0,0,0],"to":[16,16,16],
        "rotation":{"origin":[8,8,8],"axis":"y","angle":45,"rescale":true},
        "faces":{}
    }]}"##;
    let m = parse_model(json).unwrap();
    let r = m.elements[0].rotation.as_ref().unwrap();
    assert_eq!(r.origin, [8.0, 8.0, 8.0]);
    assert_eq!(r.axis, "y");
    assert_eq!(r.angle, 45.0);
    assert!(r.rescale);
    assert!(r.rot_vec.is_none(), "单轴形式不应产生三轴角度");
}

#[test]
fn model_element_rotation_three_axes() {
    let json = br##"{"elements":[{
        "from":[0,0,0],"to":[16,16,16],
        "rotation":{"origin":[8,8,8],"x":90,"y":0,"z":0},
        "faces":{}
    }]}"##;
    let m = parse_model(json).unwrap();
    let r = m.elements[0].rotation.as_ref().unwrap();
    assert_eq!(r.rot_vec, Some([90.0, 0.0, 0.0]));
}

#[test]
fn texture_object_takes_sprite() {
    let json = br##"{"textures":{"all":{"sprite":"block/x"},"bad":42}}"##;
    let m = parse_model(json).unwrap();
    assert_eq!(m.textures, vec![("all".to_string(), "block/x".to_string(), false)]);
}

/// 贴图对象里的 force_translucent 必须被解析出来。
///
/// 这是 `{"sprite": "...", "force_translucent": true}` 形式，玻璃一类
/// 方块靠它避免被当成不透明。此前该字段被整段忽略，导致特性失效。
#[test]
fn texture_object_reads_force_translucent() {
    let json = br##"{"textures":{
        "glass":{"sprite":"block/glass","force_translucent":true},
        "stone":"block/stone",
        "ice":{"sprite":"block/ice","force_translucent":false}
    }}"##;
    let m = parse_model(json).unwrap();
    let get = |k: &str| {
        m.textures
            .iter()
            .find(|(name, _, _)| name == k)
            .map(|(_, p, f)| (p.clone(), *f))
            .expect("应存在该贴图变量")
    };
    assert_eq!(get("glass"), ("block/glass".to_string(), true));
    assert_eq!(get("stone"), ("block/stone".to_string(), false), "字符串形式应为 false");
    assert_eq!(get("ice"), ("block/ice".to_string(), false), "显式 false 应为 false");
}

#[test]
fn model_face_without_texture_is_an_error() {
    let json = br##"{"elements":[{"from":[0,0,0],"to":[1,1,1],"faces":{"north":{}}}]}"##;
    let e = parse_model(json).unwrap_err().to_string();
    assert!(e.contains("texture"), "应指出缺少 texture：{e}");
}
