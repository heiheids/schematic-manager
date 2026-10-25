//! 投影的游戏版本转换。
//!
//! 对照 legacy-cpp/app_convert.cpp。把投影从当前版本转到另一版本：
//! 目标版本里已不存在的方块必须逐条指定替代品，否则转换后这些方块
//! 会变成无法识别的状态。为避免破坏原文件，转换结果另存为新文件。

use crate::block::db::BlockDb;
use crate::schematic::litematic::Schematic;

/// 转换中需要处理的一项：投影用到、但目标版本没有的方块
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertItem {
    /// 源方块 ID
    pub name: String,
    /// 在投影中出现的方块数
    pub count: u64,
    /// 推荐替代品；为空表示没有找到合适的候选
    pub suggested: String,
    /// 用户指定的替代品；为空表示转换时跳过该方块
    pub chosen: String,
}

impl ConvertItem {
    /// 转换时会实际使用的替代品
    pub fn replacement(&self) -> &str {
        if self.chosen.is_empty() {
            &self.suggested
        } else {
            &self.chosen
        }
    }
}

/// 取命名空间之后的部分
fn path_of(id: &str) -> &str {
    id.split_once(':').map(|(_, p)| p).unwrap_or(id)
}

/// 是否为原版方块
fn is_vanilla(id: &str) -> bool {
    id.starts_with("minecraft:")
}

/// 为一个在原版/目标版本中消失的方块推荐替代品。
///
/// 先找同后缀（`_stairs`、`_slab` 之类），再找同前缀（`deepslate_` 之类）。
/// 候选顺序为原版优先、其次字典序，保证同一输入总是给出同一推荐。
fn suggest_block(name: &str, target: &[&str]) -> String {
    let mut cand: Vec<&str> = target.to_vec();
    cand.sort_by(|a, b| match (is_vanilla(a), is_vanilla(b)) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    });

    let p = path_of(name);
    // 1) 同后缀：最后一个下划线起
    if let Some(u) = p.rfind('_') {
        if u + 1 < p.len() {
            let suf = &p[u..];
            if let Some(t) = cand
                .iter()
                .find(|t| path_of(t).len() > suf.len() && path_of(t).ends_with(suf))
            {
                return (*t).to_string();
            }
        }
    }
    // 2) 同前缀：第一个下划线止
    if let Some(u) = p.find('_') {
        let pre = &p[..=u];
        if let Some(t) = cand
            .iter()
            .find(|t| path_of(t).len() > pre.len() && path_of(t).starts_with(pre))
        {
            return (*t).to_string();
        }
    }
    String::new()
}

/// 分析：列出投影中用到、但目标版本不存在的方块。
///
/// 结果按出现数量降序——数量大的方块更值得优先指定替代品。
pub fn analyze(sc: &Schematic, db: &BlockDb, target_idx: usize) -> Vec<ConvertItem> {
    let Some(target) = db.versions.get(target_idx) else {
        return Vec::new();
    };
    let names: Vec<&str> = target.blocks.iter().map(|b| b.name.as_str()).collect();

    // 统计每个方块名在投影中出现的次数
    let mut counts: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    for r in &sc.regions {
        let mut per: Vec<u64> = vec![0; r.palette.len()];
        for idx in &r.indices {
            if let Some(slot) = per.get_mut(*idx as usize) {
                *slot += 1;
            }
        }
        for (i, st) in r.palette.iter().enumerate() {
            if st.is_air() || st.name.is_empty() {
                continue;
            }
            if target.blocks.iter().any(|b| b.name == st.name) {
                continue; // 目标版本有，不用处理
            }
            *counts.entry(st.name.as_str()).or_insert(0) += per[i];
        }
    }

    let mut items: Vec<ConvertItem> = counts
        .into_iter()
        .map(|(name, count)| ConvertItem {
            name: name.to_string(),
            count,
            suggested: suggest_block(name, &names),
            chosen: String::new(),
        })
        .collect();
    // 数量相同则按名称排序，保证结果稳定
    items.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    items
}

/// 转换结果统计
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConvertOutcome {
    /// 被替换掉的方块种类数
    pub replaced_kinds: usize,
    /// 被改写的方块实例数
    pub replaced_blocks: u64,
    /// 因没有指定替代品而保持原样的种类数
    pub skipped_kinds: usize,
}

/// 在内存中执行转换：逐条替换后再改写 dataVersion。
///
/// 不改动磁盘：调用方决定另存到哪里。转换失败时不应破坏原文件。
pub fn convert(
    sc: &mut Schematic,
    db: &BlockDb,
    source_idx: usize,
    target_idx: usize,
    items: &[ConvertItem],
) -> ConvertOutcome {
    let Some(target) = db.versions.get(target_idx) else {
        return ConvertOutcome::default();
    };
    let Some(source) = db.versions.get(source_idx) else {
        // 源版本未知时无法判断属性是否被支持，宁可不转换
        return ConvertOutcome::default();
    };

    let mut out = ConvertOutcome::default();
    let info = crate::schematic::model::DbBlockInfo { db, version: source };

    for it in items {
        let repl = it.replacement();
        if repl.is_empty() {
            out.skipped_kinds += 1;
            continue;
        }
        let rule = crate::schematic::replace::ReplaceRule {
            from_name: it.name.clone(),
            from_props: Vec::new(),
            to_name: repl.to_string(),
            to_props: Vec::new(),
            inherit_props: true,
        };
        let r = crate::schematic::replace::replace_blocks(sc, &rule, &info, 0);
        if r.replaced > 0 {
            out.replaced_kinds += 1;
            out.replaced_blocks += r.replaced;
        }
    }

    // 改写制作版本号，使文件被识别为目标版本
    set_data_version(sc, target.data_version);
    out
}

/// 改写投影的 MinecraftDataVersion。
///
/// Value::set 在键已存在时替换、不存在时插入，两种情况都正确处理。
fn set_data_version(sc: &mut Schematic, dv: i32) {
    sc.root
        .set("MinecraftDataVersion", crate::format::nbt::Value::Int(dv));
}

/// 目标版本的显示信息，供界面列出版本
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertTarget {
    pub index: usize,
    pub name: String,
    pub data_version: i32,
}

/// 把版本名清理成可安全用作**文件名**的一段。
///
/// 为什么必须清理：转换结果的文件名由版本名拼接而成
/// （`<原文件名主干>_<版本名>.litematic`），而版本名来自方块库：
/// 基础库的版本名取自客户端 jar 里的 `version.json`，用户也可以自己放
/// 单版本包进 `base/blocks`，因此它可能含 `..`、`/`、`\` 等字符：
/// 实测 `a\..\..\evil` 会让写出路径从投影所在目录一路退回上层目录，
/// 把结果写到别处去。
///
/// 清理规则：路径分隔符与 Windows 保留字符换成下划线，去掉结尾的点与空格
/// （它们在 Windows 上会被静默忽略，导致实际文件名与预期不符）。
pub fn safe_file_component(name: &str) -> String {
    let mut base: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    while base.ends_with('.') || base.ends_with(' ') {
        base.pop();
    }
    // 清理后可能只剩空串（例如版本名全是分隔符）：给一个可读的兜底名，
    // 而不是让调用方拼出 `<主干>_.litematic` 这种含义不明或者干脆失败的名字
    if base.is_empty() {
        base.push_str("未知版本");
    }
    base
}

/// 列出可作为转换目标的版本（排除叠加包与源版本本身）
pub fn targets(db: &BlockDb, source_data_version: i32) -> Vec<ConvertTarget> {
    let mut out: Vec<ConvertTarget> = db
        .versions
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.is_overlay && v.data_version != source_data_version)
        .map(|(i, v)| ConvertTarget {
            index: i,
            name: v.name.clone(),
            data_version: v.data_version,
        })
        .collect();
    out.sort_by_key(|t| t.data_version);
    out
}

#[cfg(test)]
#[path = "../tests/schematic/convert.rs"]
mod tests;
