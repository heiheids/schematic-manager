//! 方块检索：跨版本按名称查找方块。
//!
//! 供界面上的方块选择使用。支持三种匹配目标：完整 ID（minecraft:stone）、
//! 去掉命名空间的短名（stone）、以及中文名（石头）。
//!
//! 同一方块在多个版本中都存在时只返回一次，并附带可选的版本列表，
//! 否则结果里会出现几十条同名条目，无法挑选。

use crate::block::db::BlockDb;
use std::collections::HashMap;

/// 一条检索结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockHit {
    /// 完整方块 ID
    pub name: String,
    /// 中文名，可能为空
    pub zh: String,
    /// 英文名，可能为空
    pub en: String,
    /// 出现该方块的版本（下标 + 名称 + dataVersion），按 dataVersion 升序
    pub versions: Vec<BlockVersion>,
}

/// 方块所在的某个版本
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockVersion {
    pub index: usize,
    pub data_version: i32,
}

/// 版本选项，供界面上的版本下拉使用
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionOption {
    pub index: usize,
    pub name: String,
    pub data_version: i32,
    pub is_overlay: bool,
    pub block_count: usize,
}

/// 列出全部可用版本，按 dataVersion 升序（叠加包排在最后）
pub fn version_options(db: &BlockDb) -> Vec<VersionOption> {
    let mut out: Vec<VersionOption> = db
        .versions
        .iter()
        .enumerate()
        .map(|(i, v)| VersionOption {
            index: i,
            name: v.name.clone(),
            data_version: v.data_version,
            is_overlay: v.is_overlay,
            block_count: v.blocks.len(),
        })
        .collect();
    out.sort_by(|a, b| {
        a.is_overlay
            .cmp(&b.is_overlay)
            .then_with(|| a.data_version.cmp(&b.data_version))
    });
    out
}

/// 为某个版本建立 方块名 -> (中文名, 英文名) 的索引。
///
/// 逐方块在 `langs` 里线性查找会退化成平方复杂度，
/// 因此这里先建索引。`pub(crate)`：版本转换也要按同一套索引取中文名，
/// 两处各建一份就会出现同一方块在检索与转换里显示不同名字。
pub(crate) fn lang_index(db: &BlockDb, version_idx: usize) -> HashMap<String, (String, String)> {
    let mut map = HashMap::new();
    if let Some(v) = db.versions.get(version_idx) {
        for e in &v.langs {
            map.insert(e.block_name.clone(), (e.zh.clone(), e.en.clone()));
        }
    }
    map
}

/// 命中的优先级，越小越靠前
fn rank(name: &str, short: &str, zh: &str, en: &str, q: &str) -> Option<u8> {
    if name == q || short == q {
        return Some(0);
    }
    if zh == q || en.eq_ignore_ascii_case(q) {
        return Some(1);
    }
    if name.starts_with(q) || zh.starts_with(q) || en.to_lowercase().starts_with(q) {
        return Some(2);
    }
    if name.contains(q) || zh.contains(q) || en.to_lowercase().contains(q) {
        return Some(3);
    }
    None
}

/// 逐版本遍历方块，把命中项按方块名合并成一条并汇总版本。
///
/// `rank_of` 收到（已转小写的完整方块 ID、短名、中文名、英文名），返回命中
/// 优先级，`None` 表示不命中。检索与「列出全部」只差这一个判据，合并、去重、
/// 版本汇总的规则完全一样；分两处写会让同一方块在两条路径上拿到不同的版本列表。
fn collect<F>(db: &BlockDb, rank_of: F) -> Vec<(u8, BlockHit)>
where
    F: Fn(&str, &str, &str, &str) -> Option<u8>,
{
    // name -> (rank, hit)
    let mut found: HashMap<String, (u8, BlockHit)> = HashMap::new();

    for (vi, v) in db.versions.iter().enumerate() {
        let langs = lang_index(db, vi);
        for b in &v.blocks {
            let short = b.name.split_once(':').map(|(_, n)| n).unwrap_or(&b.name);
            let (zh, en) = langs.get(&b.name).cloned().unwrap_or_default();
            let Some(r) = rank_of(&b.name.to_lowercase(), &short.to_lowercase(), &zh, &en) else {
                continue;
            };
            match found.get_mut(&b.name) {
                Some((best, hit)) => {
                    if r < *best {
                        *best = r;
                    }
                    hit.versions.push(BlockVersion {
                        index: vi,
                        data_version: v.data_version,
                    });
                }
                None => {
                    found.insert(
                        b.name.clone(),
                        (
                            r,
                            BlockHit {
                                name: b.name.clone(),
                                zh,
                                en,
                                versions: vec![BlockVersion {
                                    index: vi,
                                    data_version: v.data_version,
                                }],
                            },
                        ),
                    );
                }
            }
        }
    }

    let mut out: Vec<(u8, BlockHit)> = found.into_values().collect();
    for (_, hit) in out.iter_mut() {
        hit.versions.sort_by_key(|v| v.data_version);
        hit.versions.dedup();
    }
    out
}

/// 跨版本检索方块。
///
/// query 为空时返回空结果——用户还没输入关键字时应当用 [`list_all`] 取全量，
/// 而不是把整库方块一次性倒给界面。
pub fn search(db: &BlockDb, query: &str, limit: usize) -> Vec<BlockHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }

    let mut out = collect(db, |name, short, zh, en| rank(name, short, zh, en, &q));
    // 先按命中优先级，再按名称，保证结果稳定
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    out.into_iter().take(limit.max(1)).map(|(_, h)| h).collect()
}

/// 列出方块库中的全部方块，按方块 ID 升序。
///
/// 供方块选择列表在用户还没输入关键字时使用：挑一个方块需要先看到全貌，
/// 只给空列表等于要求用户先知道名字。排序按 ID 而不是相关度——没有查询就没有
/// 相关度，按 ID 排是稳定且可预期的顺序。
///
/// 不截断：调用方要的就是完整清单，截一半会让排在后面的方块永远挑不到。
pub fn list_all(db: &BlockDb) -> Vec<BlockHit> {
    let mut out = collect(db, |_, _, _, _| Some(0));
    out.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    out.into_iter().map(|(_, h)| h).collect()
}

#[cfg(test)]
#[path = "../tests/block/search.rs"]
mod tests;
