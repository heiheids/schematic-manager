//! 模型继承链解析与贴图变量替换。
//!
//! 用途：Minecraft 的模型可以 `parent` 继承另一个模型。导入 jar 时要沿
//! 父链把 textures 逐级合并、并取出最近一级的 elements，最后把面里的
//! 贴图变量（`#all` 之类）替换成实际路径。
//!
//! 对照 legacy-cpp/packbuild.cpp 的 BuildChain 与 ResolveTexRef。
//!
//! 本模块不涉及文件 IO：调用方提供「按 id 取模型」的闭包，
//! 因此可完整测试继承与替换逻辑。
//!
//! 合并规则（与原实现一致）：
//!   - textures 逐级合并，子级覆盖父级
//!   - elements 取链上最近一级出现的那个（子级有则整个替换，不逐元素合并）
//!   - force_translucent 只要链上任意一级声明过即为真

use crate::block::state::ModelDef;
use std::collections::HashMap;

/// 贴图变量的原始值
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TexValue {
    /// 引用内容：可能是 `#other`、也可能是 `namespace:block/stone` 这样的路径
    pub sprite: String,
    /// 该贴图强制半透明（用于玻璃等）
    pub force_translucent: bool,
}

/// 一条继承链解析后的结果
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Chain {
    /// 贴图变量表，保留原始引用形式（可能仍含 `#`）
    pub textures: HashMap<String, TexValue>,
    /// 链上最近一级的 elements
    pub elements: Vec<crate::block::state::ModelElementDef>,
    pub force_translucent: bool,
}

/// 父链的最大深度。
///
/// 原实现用 16：真实模型的继承链通常不超过 4 层，
/// 给一个上限是为了让环形 parent（A 继承 B、B 继承 A）能终止。
const MAX_DEPTH: usize = 16;

/// 把模型 id 规范成 `命名空间:路径`。
///
/// 不带命名空间时沿用当前上下文（通常来自模型所在目录）。
pub fn resolve_model_id(raw: &str, cur_ns: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    match raw.split_once(':') {
        Some((ns, path)) => {
            if path.is_empty() {
                None
            } else {
                Some(format!("{ns}:{path}"))
            }
        }
        None => Some(format!("{cur_ns}:{raw}")),
    }
}

/// 解析继承链。
///
/// `load` 按规范 id 返回模型定义；返回 None 表示该模型不存在
/// （父模型缺失时按「无父级」处理，而不是整体失败——真实资源包里
/// 存在指向不存在父模型的模型，让整个方块因此消失代价过大）。
pub fn build_chain<F>(start_id: &str, cur_ns: &str, load: &mut F) -> Chain
where
    F: FnMut(&str) -> Option<ModelDef>,
{
    let mut visiting: Vec<String> = Vec::new();
    build_inner(start_id, cur_ns, load, &mut visiting, 0)
}

fn build_inner<F>(
    start_id: &str,
    cur_ns: &str,
    load: &mut F,
    visiting: &mut Vec<String>,
    depth: usize,
) -> Chain
where
    F: FnMut(&str) -> Option<ModelDef>,
{
    if depth > MAX_DEPTH {
        return Chain::default();
    }
    let Some(id) = resolve_model_id(start_id, cur_ns) else {
        return Chain::default();
    };
    // 环形继承：已在当前路径上出现过就直接返回，避免无限递归
    if visiting.iter().any(|v| v == &id) {
        return Chain::default();
    }
    let Some(def) = load(&id) else {
        return Chain::default();
    };

    visiting.push(id.clone());
    let mut ch = if def.parent.is_empty() {
        Chain::default()
    } else {
        // 父模型的命名空间沿用当前模型的：`parent` 常写作 "block/cube"，
        // 不带命名空间
        let (ns, _) = id.split_once(':').unwrap_or(("minecraft", ""));
        build_inner(&def.parent, ns, load, visiting, depth + 1)
    };
    visiting.pop();

    // 本级的 textures 覆盖父级
    for (k, v, force) in &def.textures {
        let tv = TexValue {
            sprite: v.clone(),
            force_translucent: *force,
        };
        // 链上任意一级声明过即为真：父级已经置位时不能被本级的 false 抹掉
        if *force {
            ch.force_translucent = true;
        }
        ch.textures.insert(k.clone(), tv);
    }
    // 本级的 elements 存在则整体替换父级的
    if !def.elements.is_empty() {
        ch.elements = def.elements.clone();
    }
    ch
}

/// 把贴图引用迭代替换成最终路径。
///
/// `#all` 这类变量可能指向另一个变量，因此要迭代；出现环时返回 None，
/// 而不是给出一个中间态或死循环。
pub fn resolve_tex_ref(textures: &HashMap<String, TexValue>, raw: &str) -> Option<String> {
    let mut cur = raw.to_string();
    let mut seen: Vec<String> = Vec::new();

    for _ in 0..MAX_DEPTH {
        if !cur.starts_with('#') {
            // 已是字面路径
            return if cur.is_empty() { None } else { Some(cur) };
        }
        let key = cur[1..].to_string();
        if seen.iter().any(|s| s == &key) {
            return None; // 成环
        }
        seen.push(key.clone());
        let v = textures.get(&key)?;
        if v.sprite.is_empty() {
            return None;
        }
        cur = v.sprite.clone();
    }
    // 迭代次数用尽仍未收敛，视为异常
    None
}

/// 判断贴图是否为「强制半透明」。
///
/// 沿变量链查找：链上任意一级声明了 force_translucent 即为真。
pub fn is_force_translucent(
    textures: &HashMap<String, TexValue>,
    raw: &str,
) -> bool {
    let mut cur = raw.to_string();
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..MAX_DEPTH {
        if !cur.starts_with('#') {
            return false;
        }
        let key = cur[1..].to_string();
        if seen.iter().any(|s| s == &key) {
            return false;
        }
        seen.push(key.clone());
        let Some(v) = textures.get(&key) else { return false };
        if v.force_translucent {
            return true;
        }
        cur = v.sprite.clone();
    }
    false
}

#[cfg(test)]
#[path = "../tests/block/state_chain.rs"]
mod tests;
