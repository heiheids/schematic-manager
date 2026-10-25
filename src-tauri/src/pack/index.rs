//! jar 内的资源索引。
//!
//! 用途：从 Minecraft 客户端/模组 jar 的条目名里找出各命名空间下的
//! blockstates、models 与 textures，供后续解析与构建资源包使用。
//!
//! 对照 legacy-cpp/packbuild.cpp 的 BuildFromJar 前半段（收集命名空间与
//! 建立模型 id 到条目路径的映射）。
//!
//! 纯字符串处理，不涉及文件 IO，因此可完整测试。

use std::collections::BTreeMap;

/// assets 下的固定子目录名
const DIR_BLOCKSTATES: &str = "blockstates";
const DIR_MODELS: &str = "models";
const DIR_TEXTS: &str = "textures";

/// 从条目名里取出 `assets/<ns>/` 之后的命名空间。
///
/// 只认 `assets/` 开头且确有命名空间段的名字，其余（META-INF、根目录文件、
/// 以及 `assets/` 下直接放文件的情形）返回 None。
fn namespace_of(entry: &str) -> Option<&str> {
    let rest = entry.strip_prefix("assets/")?;
    let (ns, _) = rest.split_once('/')?;
    if ns.is_empty() {
        return None;
    }
    Some(ns)
}

/// 从 `assets/<ns>/<dir>/<path>.json` 里取出 `<path>`；扩展名必须匹配
fn path_under(entry: &str, ns: &str, dir: &str, ext: &str) -> Option<String> {
    let prefix = format!("assets/{ns}/{dir}/");
    let rest = entry.strip_prefix(&prefix)?;
    let path = rest.strip_suffix(ext)?;
    if path.is_empty() {
        return None;
    }
    Some(path.to_string())
}

/// 一个命名空间下索引到的资源
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NamespaceAssets {
    /// blockstates 的条目名（保持输入顺序）
    pub blockstates: Vec<String>,
    /// 模型 id -> jar 内条目路径。id 形如 `minecraft:block/stone`
    pub models: BTreeMap<String, String>,
    /// 贴图条目名（`assets/<ns>/textures/**` 下的 .png）
    pub textures: Vec<String>,
}

impl NamespaceAssets {
    pub fn is_empty(&self) -> bool {
        self.blockstates.is_empty() && self.models.is_empty() && self.textures.is_empty()
    }
}

/// jar 内资源的整体索引
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JarIndex {
    /// 命名空间 -> 该命名空间下的资源
    pub namespaces: BTreeMap<String, NamespaceAssets>,
}

impl JarIndex {
    /// 含有 blockstates 的命名空间，按名称排序（保证构建结果可复现）
    pub fn blockstate_namespaces(&self) -> Vec<&str> {
        self.namespaces
            .iter()
            .filter(|(_, v)| !v.blockstates.is_empty())
            .map(|(k, _)| k.as_str())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.namespaces.values().all(|v| v.is_empty())
    }
}

/// 由条目名列表建立索引。
///
/// `entries` 的顺序会影响各列表内的顺序，因此调用方应先排序——
/// jar 的中央目录本身是有序的，但 zip 规范不保证，排序后结果才可复现。
pub fn build(entries: &[String]) -> JarIndex {
    let mut idx = JarIndex::default();

    for e in entries {
        let Some(ns) = namespace_of(e) else { continue };
        let slot = idx.namespaces.entry(ns.to_string()).or_default();

        if let Some(p) = path_under(e, ns, DIR_BLOCKSTATES, ".json") {
            slot.blockstates.push(e.clone());
            let _ = p;
            continue;
        }
        if let Some(p) = path_under(e, ns, DIR_MODELS, ".json") {
            // 模型 id 用命名空间限定；另存一份不带命名空间的短名，
            // 因为 parent 常写作 "block/cube" 而不带 minecraft 前缀
            slot.models.insert(format!("{ns}:{p}"), e.clone());
            if ns == "minecraft" {
                slot.models.insert(p, e.clone());
            }
            continue;
        }
        // 贴图：只收 .png，且必须位于 textures 下
        if e.starts_with(&format!("assets/{ns}/{DIR_TEXTS}/"))
            && e.to_ascii_lowercase().ends_with(".png")
        {
            slot.textures.push(e.clone());
        }
    }

    // 各列表排序，使结果与输入顺序无关
    for v in idx.namespaces.values_mut() {
        v.blockstates.sort();
        v.textures.sort();
    }
    idx
}

/// 把模型/贴图引用规范成 `<ns>:<path>` 形式。
///
/// 引用可能已是全名（`minecraft:block/stone`），也可能只是路径
/// （`block/stone`），后者要补上当前命名空间。
pub fn qualify(reference: &str, cur_ns: &str) -> Option<String> {
    if reference.is_empty() {
        return None;
    }
    match reference.split_once(':') {
        Some((ns, path)) => {
            if ns.is_empty() || path.is_empty() {
                None
            } else {
                Some(format!("{ns}:{path}"))
            }
        }
        None => Some(format!("{cur_ns}:{reference}")),
    }
}

#[cfg(test)]
#[path = "../tests/pack/index.rs"]
mod tests;
