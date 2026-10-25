//! blockstate 与 model 的 JSON 解析。
//!
//! 用途：导入 Minecraft 客户端/模组 jar 时，`assets/<ns>/blockstates/*.json`
//! 描述方块状态到模型的映射，`models/**/*.json` 描述模型本身。这两类文件
//! 要转成内部资源包格式（blocks.dat）里的条件串与模型记录。
//!
//! 对照 legacy-cpp/packbuild.cpp 的 NormalizeWhenDict / NormalizeWhenMultipart。
//! 条件串的表示必须与读取端（schematic/model.rs）一致，否则方块会解析不出模型：
//!   - `,` 表示 AND
//!   - `|` 表示 OR
//!   - 空串表示无条件
//!
//! 本模块只做解析与归一化，不涉及文件 IO 与压缩，因此可完整测试。

use crate::error::{AppError, AppResult};
use serde_json::Value;
use std::collections::BTreeMap;

/// 一条变体：条件串 + 模型引用 + 旋转
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantEntry {
    /// 条件串，空表示无条件
    pub when: String,
    /// 模型引用，原样保留（含命名空间，未解析成条目路径）
    pub model: String,
    /// 绕 X 的 90 度倍数
    pub rot_x_div90: u8,
    /// 绕 Y 的 90 度倍数
    pub rot_y_div90: u8,
    pub uvlock: bool,
}

/// 一个方块的 blockstate 定义
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlockStateDef {
    /// variants 形式下的条目
    pub variants: Vec<VariantEntry>,
    /// multipart 形式下的条目
    pub multipart: Vec<VariantEntry>,
}

impl BlockStateDef {
    pub fn is_empty(&self) -> bool {
        self.variants.is_empty() && self.multipart.is_empty()
    }
}

/// 把 `{"facing":"east","half":"top"}` 这类对象归一化成 `facing=east,half=top`。
///
/// 键排序后拼接：JSON 对象的键顺序在规范上不保证，若不排序，同一份
/// blockstate 在不同解析器下会得到不同条件串，进而算出不同的模型记录，
/// 让资源包内容不稳定。
pub fn normalize_when_dict(when: &Value) -> String {
    let Some(obj) = when.as_object() else {
        return String::new();
    };
    // BTreeMap 保证键有序，结果可复现
    let mut kv: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in obj {
        let s = match v {
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            // 其余类型（数组/对象/null）在 blockstate 里没有意义，跳过
            _ => continue,
        };
        kv.insert(k.clone(), s);
    }
    kv.into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// 把 multipart 的 `when` 归一化成条件串。
///
/// 支持嵌套的 `{"OR":[...]}` 与 `{"AND":[...]}`：
///   - OR 内任一成立即可 → 用 `|` 连接备选
///   - AND 内全部成立 → 用 `,` 连接
///   - 二者都没有 → 当作普通属性字典
///
/// OR 的某个备选为空（无条件）时整体视为无条件并返回空串——
/// 否则会写出「某个分支必然成立」却仍带条件的错误表示。
pub fn normalize_when_multipart(when: &Value) -> String {
    match when {
        Value::Null => String::new(),
        Value::Object(obj) => {
            if let Some(Value::Array(alts)) = obj.get("OR") {
                let mut parts = Vec::with_capacity(alts.len());
                for a in alts {
                    let s = normalize_when_multipart(a);
                    if s.is_empty() {
                        return String::new(); // 有空备选 = 无条件
                    }
                    parts.push(s);
                }
                return parts.join("|");
            }
            if let Some(Value::Array(parts)) = obj.get("AND") {
                let mut out = Vec::with_capacity(parts.len());
                for a in parts {
                    let s = normalize_when_multipart(a);
                    if !s.is_empty() {
                        out.push(s);
                    }
                }
                return out.join(",");
            }
            normalize_when_dict(when)
        }
        _ => String::new(),
    }
}

/// 取整数字段，缺失或类型不符时用默认值
/// 取 blockstate 里的旋转字段，换算成 90 度倍数。
///
/// JSON 里的 `x` / `y` 是**角度**（90、180、270），而字段语义与磁盘格式
/// 都是「90 度倍数」（见 VariantEntry::rot_x_div90 的说明，
/// legacy-cpp 写出时也是 `/ 90`，见 packbuild.cpp:513-514）。
/// 这里不换算的话，写出端再按「倍数」解释会得到错误的旋转角：
/// 例如 y=90 被当成 2 个 90 度（即 180 度），y=180 被当成 0 度。
fn get_rot_div90(obj: &serde_json::Map<String, Value>, key: &str) -> u8 {
    let deg = obj.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
    // 非 90 的整数倍（异常数据）按 0 处理：写出格式只支持 4 个朝向，
    // 取整或取模都会得到看似合法却错误的角度。
    if deg % 90 != 0 {
        return 0;
    }
    ((deg / 90) % 4) as u8
}

/// 解析一个变体对象
fn parse_variant(when: String, v: &Value) -> AppResult<VariantEntry> {
    let obj = v
        .as_object()
        .ok_or_else(|| AppError::Parse("blockstate 变体不是对象".into()))?;
    let model = obj
        .get("model")
        .and_then(|m| m.as_str())
        .ok_or_else(|| AppError::Parse("blockstate 变体缺少 model 字段".into()))?;
    Ok(VariantEntry {
        when,
        model: model.to_string(),
        rot_x_div90: get_rot_div90(obj, "x"),
        rot_y_div90: get_rot_div90(obj, "y"),
        uvlock: obj.get("uvlock").and_then(|b| b.as_bool()).unwrap_or(false),
    })
}

/// 解析 blockstate JSON。
///
/// 两种形式二选一：`variants` 的键即条件串；`multipart` 是数组，
/// 每项的 `when` 是嵌套条件结构、缺省表示无条件。
pub fn parse_blockstate(json: &[u8]) -> AppResult<BlockStateDef> {
    let root: Value = serde_json::from_slice(json)
        .map_err(|e| AppError::Parse(format!("blockstate JSON 解析失败：{e}")))?;
    let obj = root
        .as_object()
        .ok_or_else(|| AppError::Parse("blockstate 根节点不是对象".into()))?;

    let mut def = BlockStateDef::default();

    if let Some(Value::Object(vs)) = obj.get("variants") {
        for (key, v) in vs {
            // 键可能是数组：一个条件对应多个模型（例如随机模型）
            let when = key.trim().to_string();
            match v {
                Value::Array(items) => {
                    for it in items {
                        def.variants.push(parse_variant(when.clone(), it)?);
                    }
                }
                other => def.variants.push(parse_variant(when.clone(), other)?),
            }
        }
    }

    if let Some(Value::Array(items)) = obj.get("multipart") {
        for it in items {
            let obj = it
                .as_object()
                .ok_or_else(|| AppError::Parse("multipart 项不是对象".into()))?;
            let when = obj
                .get("when")
                .map(normalize_when_multipart)
                .unwrap_or_default();
            // 现版本把模型放在 apply 里；早期版本直接写作 model。
            // 注意早期形式是 `{"model":"m1"}`——模型字段与 x/y/uvlock 同级，
            // 因此不能拿 model 的值当 apply 用（那是个字符串，不是对象）。
            if obj.contains_key("model") && !obj.contains_key("apply") {
                let model = obj
                    .get("model")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::Parse("multipart 项的 model 字段不是字符串".into())
                    })?;
                // 模型字段与 x/y/uvlock 同级，直接复用整项即可：
                // parse_variant 正是按这套字段解析的
                let mut entry = parse_variant(when.clone(), it)?;
                entry.model = model.to_string();
                def.multipart.push(entry);
                continue;
            }

            let apply = obj.get("apply").ok_or_else(|| {
                AppError::Parse("multipart 项缺少 apply 字段".into())
            })?;
            match apply {
                Value::Array(list) => {
                    for a in list {
                        def.multipart.push(parse_variant(when.clone(), a)?);
                    }
                }
                other => def.multipart.push(parse_variant(when.clone(), other)?),
            }
        }
    }

    Ok(def)
}

/// 模型的单个面
#[derive(Debug, Clone, PartialEq)]
pub struct ModelFace {
    /// 贴图引用（`#all`、`#side` 或直接路径）
    pub texture: String,
    /// cullface 的面名，空表示不剔除
    pub cullface: String,
    pub tintindex: i32,
    /// UV 的旋转角度（0/90/180/270）
    pub rotation: u16,
    /// u1,v1,u2,v2；全为 0 表示未指定，需按元素边界推导
    pub uv: Option<[f32; 4]>,
}

/// 模型的一个元素（立方体）
#[derive(Debug, Clone, PartialEq)]
pub struct ModelElementDef {
    pub from: [f32; 3],
    pub to: [f32; 3],
    /// 已设置的面：面名 -> 面定义
    pub faces: Vec<(String, ModelFace)>,
    /// 元素自身的旋转
    pub rotation: Option<ElementRotation>,
}

/// 元素旋转
#[derive(Debug, Clone, PartialEq)]
pub struct ElementRotation {
    pub origin: [f32; 3],
    /// single 时为轴方向 + 角度；多轴时为三轴角度
    pub axis: String,
    pub angle: f32,
    pub rescale: bool,
    /// `{"x":..,"y":..,"z":..}` 形式的旋转角度
    pub rot_vec: Option<[f32; 3]>,
}

/// 解析后的模型
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelDef {
    /// 父模型引用（用于继承链）
    pub parent: String,
    /// 贴图变量：`all` -> (`minecraft:block/stone`, 是否强制半透明)
    ///
    /// 第二个分量来自 `{"sprite": "...", "force_translucent": true}` 形式；
    /// 字符串形式恒为 false。玻璃、冰一类方块靠它避免被剔除成不透明。
    pub textures: Vec<(String, String, bool)>,
    pub elements: Vec<ModelElementDef>,
    /// 是否参与环境光遮蔽（仅记录，不影响渲染）
    pub ambientocclusion: bool,
}

fn read_vec3(v: &Value, def: f32) -> [f32; 3] {
    let arr = match v.as_array() {
        Some(a) if a.len() >= 3 => a,
        _ => return [def; 3],
    };
    [
        arr[0].as_f64().unwrap_or(def as f64) as f32,
        arr[1].as_f64().unwrap_or(def as f64) as f32,
        arr[2].as_f64().unwrap_or(def as f64) as f32,
    ]
}

/// 解析 model JSON
pub fn parse_model(json: &[u8]) -> AppResult<ModelDef> {
    let root: Value = serde_json::from_slice(json)
        .map_err(|e| AppError::Parse(format!("model JSON 解析失败：{e}")))?;
    let obj = root
        .as_object()
        .ok_or_else(|| AppError::Parse("model 根节点不是对象".into()))?;

    let mut m = ModelDef {
        parent: obj
            .get("parent")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        ambientocclusion: obj
            .get("ambientocclusion")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        ..Default::default()
    };

    if let Some(Value::Object(tex)) = obj.get("textures") {
        for (k, v) in tex {
            // 贴图值可以是字符串，也可以是 `{"sprite": "...", "force_translucent": true}`
            // 形式，后者取 sprite 与 force_translucent，取不到 sprite 就跳过——
            // 猜一个路径比留空更糟。force_translucent 丢失会让玻璃一类方块
            // 被当成不透明，与 legacy-cpp 的行为不一致。
            let (path, force) = match v {
                Value::String(s) => (s.clone(), false),
                Value::Object(o) => match o.get("sprite").and_then(|s| s.as_str()) {
                    Some(s) => (
                        s.to_string(),
                        o.get("force_translucent")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(false),
                    ),
                    None => continue,
                },
                _ => continue,
            };
            m.textures.push((k.clone(), path, force));
        }
    }

    if let Some(Value::Array(elems)) = obj.get("elements") {
        for e in elems {
            let eo = e
                .as_object()
                .ok_or_else(|| AppError::Parse("model 元素不是对象".into()))?;
            let from = eo.get("from").map(|v| read_vec3(v, 0.0)).unwrap_or([0.0; 3]);
            let to = eo.get("to").map(|v| read_vec3(v, 16.0)).unwrap_or([16.0; 3]);

            let rotation = eo.get("rotation").and_then(|r| r.as_object()).map(|r| {
                let origin = r
                    .get("origin")
                    .map(|v| read_vec3(v, 8.0))
                    .unwrap_or([8.0; 3]);
                let axis = r
                    .get("axis")
                    .and_then(|v| v.as_str())
                    .unwrap_or("y")
                    .to_string();
                let angle = r.get("angle").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let rescale = r.get("rescale").and_then(|v| v.as_bool()).unwrap_or(false);
                // 多轴形式：x/y/z 三个角度都在
                let has_axes = r.contains_key("x") && r.contains_key("y") && r.contains_key("z");
                let rot_vec = if has_axes {
                    Some([
                        r.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                        r.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                        r.get("z").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                    ])
                } else {
                    None
                };
                ElementRotation { origin, axis, angle, rescale, rot_vec }
            });

            let mut faces = Vec::new();
            if let Some(Value::Object(fo)) = eo.get("faces") {
                for (name, f) in fo {
                    let fo = match f.as_object() {
                        Some(o) => o,
                        None => continue,
                    };
                    let texture = fo
                        .get("texture")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            AppError::Parse(format!("模型面 {name} 缺少 texture 字段"))
                        })?
                        .to_string();
                    let uv = fo.get("uv").and_then(|v| v.as_array()).and_then(|a| {
                        if a.len() < 4 {
                            return None;
                        }
                        Some([
                            a[0].as_f64().unwrap_or(0.0) as f32,
                            a[1].as_f64().unwrap_or(0.0) as f32,
                            a[2].as_f64().unwrap_or(16.0) as f32,
                            a[3].as_f64().unwrap_or(16.0) as f32,
                        ])
                    });
                    faces.push((
                        name.clone(),
                        ModelFace {
                            texture,
                            cullface: fo
                                .get("cullface")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            tintindex: fo
                                .get("tintindex")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(-1) as i32,
                            rotation: fo.get("rotation").and_then(|v| v.as_u64()).unwrap_or(0) as u16,
                            uv,
                        },
                    ));
                }
            }
            faces.sort_by(|a, b| a.0.cmp(&b.0)); // 结果稳定

            m.elements.push(ModelElementDef { from, to, faces, rotation });
        }
    }

    Ok(m)
}

#[cfg(test)]
#[path = "../tests/block/state.rs"]
mod tests;
