//! 方块替换。
//!
//! 对照 legacy-cpp/litematic.cpp 的 ReplaceRule / ReplaceBlocks。
//!
//! 替换在**调色板层面**进行：同一状态的方块在区域里共享一个调色板项，
//! 因此只需逐项计算替换结果并重映射索引，而不必逐方块改写。
//! 调色板通常只有几十项，而方块可能上百万个，差别很大。

use crate::schematic::litematic::Schematic;

/// 方块知识来源。
///
/// 替换需要知道目标方块支持哪些属性、以及必需属性的默认值，
/// 否则会写出游戏无法识别的状态（例如给石头加上 facing）。
/// 由方块库实现，测试可提供自己的实现。
pub trait BlockInfo {
    /// 目标方块是否支持某属性取某值
    fn supports_prop(&self, block: &str, prop: &str, value: &str) -> bool;
    /// 目标方块的必需属性及默认值
    fn default_props(&self, block: &str) -> Vec<(String, String)>;
}

/// 一条替换规则
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplaceRule {
    /// 源方块名（必填，不含属性）
    pub from_name: String,
    /// 可选：限定源方块必须同时满足这些属性
    pub from_props: Vec<(String, String)>,
    /// 目标方块名
    pub to_name: String,
    /// 可选：替换后强制设置的属性
    pub to_props: Vec<(String, String)>,
    /// 是否继承源方块中目标也支持的属性
    pub inherit_props: bool,
}

/// 替换结果统计
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReplaceOutcome {
    /// 被改写的方块实例数
    pub replaced: u64,
    /// 实际发生变化的调色板项数
    pub palette_changed: usize,
}

/// 判断源方块是否命中规则
fn matches(state: &crate::schematic::litematic::BlockState, rule: &ReplaceRule) -> bool {
    if state.name != rule.from_name {
        return false;
    }
    rule.from_props
        .iter()
        .all(|(k, v)| state.prop(k) == Some(v.as_str()))
}

/// 计算替换后的新状态。返回 None 表示无需替换。
fn replacement(
    state: &crate::schematic::litematic::BlockState,
    rule: &ReplaceRule,
    info: &dyn BlockInfo,
) -> Option<crate::schematic::litematic::BlockState> {
    if !matches(state, rule) {
        return None;
    }
    let mut next = crate::schematic::litematic::BlockState::new(&rule.to_name);

    // 继承：源属性里目标也支持的才保留。
    // 没有「整份照抄源属性」的开关：目标不支持的属性会让游戏读不出该状态，
    // legacy-cpp 也没有提供这种用法（其 keepUnknownProps 字段从未被读取）。
    if rule.inherit_props {
        for (k, v) in &state.props {
            if info.supports_prop(&rule.to_name, k, v) {
                next.set_prop(k, v);
            }
        }
    }

    // 补齐目标必需的属性；已有取值则保留（继承来的优先）
    for (k, v) in info.default_props(&rule.to_name) {
        if next.prop(&k).is_none() {
            next.set_prop(&k, &v);
        }
    }

    // 强制属性最后应用，优先级最高
    for (k, v) in &rule.to_props {
        next.set_prop(k, v);
    }

    // 结果与原来完全一致时视作未替换，避免把「没变化」计入统计
    if next == *state {
        None
    } else {
        Some(next)
    }
}

/// 统计会被改写的方块实例数，不修改投影。
///
/// 供界面上「预计替换数量」使用。逐调色板项判断即可：
/// 同一调色板项的方块要么全被改写、要么全不改，因此只需知道每个
/// 调色板项对应多少个实例，而不必逐方块判断。
pub fn count_matches(sc: &Schematic, rule: &ReplaceRule, info: &dyn BlockInfo) -> u64 {
    let mut total = 0u64;
    for r in &sc.regions {
        // 调色板项 -> 命中与否及实例数
        let hits: Vec<bool> = r
            .palette
            .iter()
            .map(|st| {
                matches(st, rule)
                    && replacement(st, rule, info).is_some()
            })
            .collect();
        if !hits.iter().any(|h| *h) {
            continue;
        }
        let mut per: Vec<u64> = vec![0; r.palette.len()];
        for idx in &r.indices {
            if let Some(slot) = per.get_mut(*idx as usize) {
                *slot += 1;
            }
        }
        for (i, hit) in hits.iter().enumerate() {
            if *hit {
                total += per[i];
            }
        }
    }
    total
}

/// 对整个投影执行替换。
///
/// `limit` 大于 0 时最多替换那么多个方块实例。
pub fn replace_blocks(
    sc: &mut Schematic,
    rule: &ReplaceRule,
    info: &dyn BlockInfo,
    limit: u64,
) -> ReplaceOutcome {
    // 空名会让方块被改成无名状态，写出损坏数据。
    // legacy-cpp 在 ReplaceBlocks 开头即做同样检查（litematic.cpp:457）。
    if rule.from_name.is_empty() || rule.to_name.is_empty() {
        return ReplaceOutcome::default();
    }

    let mut outcome = ReplaceOutcome::default();

    for r in sc.regions.iter_mut() {
        // 逐调色板项算出替换后的状态；None 表示不变
        let mut mapped: Vec<Option<usize>> = vec![None; r.palette.len()];
        // 按索引遍历：循环体内会向 palette 追加项，不能同时持有它的借用
        let original_len = r.palette.len();
        for i in 0..original_len {
            let st = r.palette[i].clone();
            let Some(next) = replacement(&st, rule, info) else { continue };
            // 已存在同状态则复用，避免调色板无谓膨胀
            let idx = match r.palette.iter().position(|p| *p == next) {
                Some(k) => k,
                None => {
                    r.palette.push(next);
                    r.palette.len() - 1
                }
            };
            mapped[i] = Some(idx);
        }

        if mapped.iter().all(|m| m.is_none()) {
            continue;
        }

        for slot in r.indices.iter_mut() {
            let cur = *slot as usize;
            let Some(Some(target)) = mapped.get(cur).copied() else { continue };
            if limit > 0 && outcome.replaced >= limit {
                continue;
            }
            if target != cur {
                *slot = target as i32;
                outcome.replaced += 1;
            }
        }
        outcome.palette_changed += mapped.iter().filter(|m| m.is_some()).count();
    }

    if outcome.replaced > 0 {
        sc.blocks_modified = true;
    }
    outcome
}

#[cfg(test)]
#[path = "../tests/schematic/replace.rs"]
mod tests;
