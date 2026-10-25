//! 方块状态到模型的解析。
//!
//! 对照 legacy-cpp/blockdb.cpp 的 MatchWhen / ResolveModels。
//! 方块模型定义来自 Minecraft 的资源包语义：
//!   - `variants`：**取第一个匹配的条目**（不是全部）
//!   - `multipart`：**所有匹配的条目叠加**，条件为空表示恒成立
//!   - 条件串用 `,` 表示「与」，用 `|` 表示「或」

use crate::block::db::{BlockDb, BlockDef, Variant, VersionDef};
use crate::schematic::replace::BlockInfo;
use std::collections::BTreeMap;
use crate::schematic::litematic::BlockState;

/// 解析 "a=1,b=2" 形式的与条件。
///
/// 只按第一个 `=` 切分：属性值本身可能含有 `=`。
/// 缺少 `=` 或 `=` 在首位的片段被忽略，这与原实现一致。
pub fn parse_conds(when: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if when.is_empty() {
        return out;
    }
    for one in when.split(',') {
        if let Some(eq) = one.find('=') {
            if eq > 0 {
                out.push((one[..eq].to_string(), one[eq + 1..].to_string()));
            }
        }
    }
    out
}

/// 全部条件都满足才为真（与语义）
pub fn match_conds(conds: &[(String, String)], st: &BlockState) -> bool {
    conds.iter().all(|(k, v)| st.prop(k) == Some(v.as_str()))
}

/// 单个条件串是否匹配。空串恒为真。
pub fn match_when(when: &str, st: &BlockState) -> bool {
    if when.is_empty() {
        return true;
    }
    match_conds(&parse_conds(when), st)
}

/// multipart 的条件可含 `|` 分隔的多个备选，任一成立即为真。
pub fn match_any_when(when: &str, st: &BlockState) -> bool {
    if when.is_empty() {
        return true;
    }
    when.split('|').any(|alt| match_conds(&parse_conds(alt), st))
}

/// 解析出的单个模型引用：模型下标 + 该变体要求的旋转
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedModel {
    pub model_idx: u32,
    /// 绕 X 轴旋转的 90 度倍数
    pub rot_x_div90: u8,
    /// 绕 Y 轴旋转的 90 度倍数
    pub rot_y_div90: u8,
    pub uvlock: bool,
}

impl From<&Variant> for ResolvedModel {
    fn from(v: &Variant) -> Self {
        Self {
            model_idx: v.model_idx,
            rot_x_div90: v.rot_x_div90,
            rot_y_div90: v.rot_y_div90,
            uvlock: v.uvlock,
        }
    }
}

/// 在指定版本（及其叠加包）中按方块名查方块定义。
///
/// 叠加包是模组提供的方块，基础版本里没有它们的定义。
/// 查找顺序与旧实现一致：本版本 -> 绑定到该版本的叠加包 -> 任意叠加包。
pub fn find_block<'a>(db: &'a BlockDb, version: &'a VersionDef, name: &str) -> Option<&'a BlockDef> {
    if let Some(b) = version.blocks.iter().find(|b| b.name == name) {
        return Some(b);
    }
    db.versions
        .iter()
        .filter(|v| v.is_overlay)
        .find_map(|v| v.blocks.iter().find(|b| b.name == name))
}

/// 解析方块状态对应的模型列表。
///
/// variants 取第一个匹配项，multipart 叠加全部匹配项——两者结果合并返回，
/// 顺序为 variants 在前、multipart 在后。
pub fn resolve_models<'a>(
    db: &'a BlockDb,
    version: &'a VersionDef,
    st: &BlockState,
) -> Vec<ResolvedModel> {
    let Some(bd) = find_block(db, version, &st.name) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    // variants：取第一个匹配的（Minecraft 语义），命中后即停止
    for v in &bd.variants {
        if match_when(&v.when, st) {
            out.push(ResolvedModel::from(v));
            break;
        }
    }

    // multipart：所有匹配项叠加
    for v in &bd.multipart {
        if match_any_when(&v.when, st) {
            out.push(ResolvedModel::from(v));
        }
    }

    out
}

/// 一个方块支持的属性及其取值域：属性名 -> 取值（有序，便于结果稳定）
pub type PropDomain = BTreeMap<String, Vec<String>>;

/// 收集方块支持的属性取值域。
///
/// 信息全部来自变体与 multipart 的条件串——方块库并不单独存属性表，
/// 条件串里出现过的 prop=value 就是该方块支持的取值。
pub fn block_prop_domain(
    db: &BlockDb,
    version: &VersionDef,
    name: &str,
) -> Option<(PropDomain, Vec<(String, String)>)> {
    let bd = find_block(db, version, name)?;
    let mut domain: PropDomain = BTreeMap::new();
    // 默认值取自第一个变体的条件：blockstate 文件里默认变体排在前面，
    // 因此它列出的取值最接近游戏内的默认状态。
    let mut defaults: Vec<(String, String)> = Vec::new();

    // 默认值只看 variants[0]：legacy-cpp 要求 !variants.empty() &&
    // !variants[0].when.empty()（blockdb.cpp:552）。若把 multipart 也算进
    // 「第一个变体」，variants 为空的方块会拿 multipart[0] 当默认值。
    let first_variant_when = bd
        .variants
        .first()
        .map(|v| v.when.as_str())
        .unwrap_or("");
    if !first_variant_when.is_empty() {
        for alt in first_variant_when.split('|') {
            for (k, val) in parse_conds(alt) {
                defaults.push((k, val));
            }
        }
    }

    for v in bd.variants.iter().chain(bd.multipart.iter()) {
        if v.when.is_empty() {
            continue;
        }
        for alt in v.when.split('|') {
            for (k, val) in parse_conds(alt) {
                let slot = domain.entry(k.clone()).or_default();
                if !slot.contains(&val) {
                    slot.push(val.clone());
                }
            }
        }
    }
    // 兜底必须在排序**之前**取「首次出现」的值：legacy-cpp 的
    // DefaultProps 用的是 bd.props 的首次出现顺序（blockdb.cpp:556-557），
    // 而排序后取首值会拿到字典序最小的那个，两者对「首变体未提及该属性」
    // 的方块会给出不同的默认状态（实测 46469 个组合中有 1264 个不同）。
    for (k, vals) in &domain {
        if !defaults.iter().any(|(dk, _)| dk == k) {
            if let Some(v) = vals.first() {
                defaults.push((k.clone(), v.clone()));
            }
        }
    }
    for vals in domain.values_mut() {
        vals.sort();
    }
    Some((domain, defaults))
}

/// 以方块库为知识来源的替换信息
pub struct DbBlockInfo<'a> {
    pub db: &'a BlockDb,
    pub version: &'a VersionDef,
}

impl BlockInfo for DbBlockInfo<'_> {
    fn supports_prop(&self, block: &str, prop: &str, value: &str) -> bool {
        match block_prop_domain(self.db, self.version, block) {
            Some((domain, _)) => domain
                .get(prop)
                .map(|vals| vals.iter().any(|v| v == value))
                .unwrap_or(false),
            None => false,
        }
    }

    fn default_props(&self, block: &str) -> Vec<(String, String)> {
        block_prop_domain(self.db, self.version, block)
            .map(|(_, d)| d)
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "../tests/schematic/model.rs"]
mod tests;
