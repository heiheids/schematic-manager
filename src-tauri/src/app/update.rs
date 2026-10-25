//! 更新检测：当前版本、发布列表的解析与语义化版本比较。
//!
//! 检测读 GitHub 的 releases 接口。这个仓库里的发布**全是预发布**（开发构建），
//! 所以不能用 `/releases/latest`：它对只发过预发布的仓库返回 404，什么也查不到。
//! 因此这里取发布列表，自行按语义化版本挑出最高的那个；草稿跳过，预发布算数。
//!
//! 版本比较按 <https://semver.org/#spec-item-11> 的优先级规则实现，包括
//! 预发布标识：`0.0.2-dev.6` 比 `0.0.2` 低，`0.0.2-dev.10` 比 `0.0.2-dev.6` 高。

use crate::error::{AppError, AppResult};
use serde_json::Value;
use std::cmp::Ordering;
use std::time::Duration;

/// 检测目标仓库
pub const REPO: &str = "heiheids/schematic-manager";

/// 取最近 50 条发布，够覆盖一个阶段的开发构建
const RELEASES_URL: &str = "https://api.github.com/repos/heiheids/schematic-manager/releases?per_page=50";

/// 编译进二进制的版本号。
///
/// 取自 Cargo 的包版本，也就是 CI 在构建前用 bump-version.mjs 写入的那个值，
/// 因此它反映的是**这个可执行文件**的版本，而不是界面资源里的某个常量。
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 一条发布记录里我们关心的字段
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// 原始 tag，如 `v0.0.2-dev.6`
    pub tag: String,
    /// 去掉前导 v 的版本号，如 `0.0.2-dev.6`
    pub version: Version,
    pub name: String,
    pub url: String,
    pub published_at: String,
}

/// 一次检测的结果：当前版本 + 挑出的最新发布（没有可解析的发布时为 None）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    pub current: Version,
    pub newest: Option<Release>,
}

impl CheckOutcome {
    pub fn has_update(&self) -> bool {
        match &self.newest {
            Some(r) => r.version > self.current,
            None => false,
        }
    }
}

/// 语义化版本号
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    /// None 表示正式版（无预发布标识），排序时高于同核心号的预发布版
    pre: Option<Vec<PreId>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreId {
    Num(u64),
    Text(String),
}

impl Version {
    /// 解析版本号，容忍前导 `v` 与构建元数据（`+` 之后一律忽略）。
    ///
    /// 解析不出来时返回 None：仓库里若出现不合规范的 tag，应当跳过它去比较
    /// 其余发布，而不是把它当成 0.0.0 或直接让整个检测失败。
    pub fn parse(raw: &str) -> Option<Version> {
        let text = raw.trim();
        let text = text.strip_prefix('v').or_else(|| text.strip_prefix('V')).unwrap_or(text);
        // 构建元数据不参与比较
        let text = text.split('+').next().unwrap_or(text);
        let (core_text, pre_text) = match text.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (text, None),
        };

        let mut core = [0u64; 3];
        let parts: Vec<&str> = core_text.split('.').collect();
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }
        for (i, p) in parts.iter().enumerate() {
            // 允许 0.0 这种写法，缺的部分按 0 处理
            core[i] = p.parse::<u64>().ok()?;
        }

        let pre = match pre_text {
            None => None,
            Some(p) => {
                if p.is_empty() {
                    return None;
                }
                let ids: Option<Vec<PreId>> = p
                    .split('.')
                    .map(|id| {
                        if id.is_empty() {
                            return None;
                        }
                        // 纯数字标识按数值比较，其余按文本
                        match id.parse::<u64>() {
                            Ok(n) => Some(PreId::Num(n)),
                            Err(_) => Some(PreId::Text(id.to_string())),
                        }
                    })
                    .collect();
                Some(ids?)
            }
        };

        Some(Version { core, pre })
    }

    /// 是否预览版（带 `-` 后缀，如 `0.1.1-dev.13`）。
    ///
    /// 发布链路判定 GitHub 上 `prerelease` 标记用的就是同一条规则，所以界面
    /// 据此标注「预览版」，与发布页上显示的性质一致，不会各说各话。
    pub fn is_prerelease(&self) -> bool {
        self.pre.is_some()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.core.cmp(&other.core) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match (&self.pre, &other.pre) {
            // 正式版高于预发布版
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => {
                for (x, y) in a.iter().zip(b.iter()) {
                    let ord = match (x, y) {
                        (PreId::Num(m), PreId::Num(n)) => m.cmp(n),
                        (PreId::Num(_), PreId::Text(_)) => Ordering::Less,
                        (PreId::Text(_), PreId::Num(_)) => Ordering::Greater,
                        (PreId::Text(m), PreId::Text(n)) => m.cmp(n),
                    };
                    if ord != Ordering::Equal {
                        return ord;
                    }
                }
                // 前缀相同时，标识多的那个更高
                a.len().cmp(&b.len())
            }
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// 还原成 `0.0.2-dev.6` 这样的文本，用于回报给界面。
///
/// 内部拆成核心号与预发布标识是为了比较，展示时必须拼回去——直接把结构体
/// 交给界面会得到 `[0, 0, 2]` 之类看不懂的东西。
impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.core[0], self.core[1], self.core[2])?;
        if let Some(ids) = &self.pre {
            let text: Vec<String> = ids
                .iter()
                .map(|id| match id {
                    PreId::Num(n) => n.to_string(),
                    PreId::Text(t) => t.clone(),
                })
                .collect();
            write!(f, "-{}", text.join("."))?;
        }
        Ok(())
    }
}

/// 把 releases 接口返回的 JSON 解析成发布列表。
///
/// 跳过草稿与解析不出发布号的条目；`published_at` 缺失时留空。
pub fn parse_releases(json: &str) -> AppResult<Vec<Release>> {
    let root: Value = serde_json::from_str(json)
        .map_err(|e| AppError::Parse(format!("发布列表不是合法 JSON：{e}")))?;
    let items = root
        .as_array()
        .ok_or_else(|| AppError::Parse("发布列表不是数组".to_string()))?;

    let mut out = Vec::new();
    for item in items {
        if item.get("draft").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let Some(tag) = item.get("tag_name").and_then(Value::as_str) else {
            continue;
        };
        let Some(version) = Version::parse(tag) else {
            continue;
        };
        out.push(Release {
            tag: tag.to_string(),
            version,
            name: item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(tag)
                .to_string(),
            url: item
                .get("html_url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            published_at: item
                .get("published_at")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        });
    }
    Ok(out)
}

/// 从发布列表里挑出版本最高的一条
pub fn newest(releases: Vec<Release>) -> Option<Release> {
    releases.into_iter().max_by(|a, b| a.version.cmp(&b.version))
}

/// 查询发布列表并与当前版本比较。
///
/// 超时给 20 秒：这里只是一个几十 KB 的 JSON，卡住时应当尽快告诉用户
/// 「检测失败」，而不是让窗口一直转圈。
pub fn check() -> AppResult<CheckOutcome> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build();
    let resp = agent
        .get(RELEASES_URL)
        // GitHub 的接口要求带 User-Agent，缺了会直接 403
        .set("User-Agent", "schematic-manager-update-check")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| AppError::Parse(format!("检查更新失败 {RELEASES_URL}：{e}")))?;
    let status = resp.status();
    let body = resp
        .into_string()
        .map_err(|e| AppError::Parse(format!("读取发布列表失败：{e}")))?;
    if crate::app::run_log::enabled(crate::app::run_log::Level::Debug) {
        crate::app::run_log::debug(&format!(
            "GET {RELEASES_URL} -> {status}\n{}",
            crate::app::run_log::clip_body(&body)
        ));
    }

    let current = Version::parse(current_version())
        .ok_or_else(|| AppError::Parse(format!("当前版本号无法解析：{}", current_version())))?;
    Ok(CheckOutcome {
        current,
        newest: newest(parse_releases(&body)?),
    })
}

#[cfg(test)]
#[path = "../tests/app/update.rs"]
mod tests;
