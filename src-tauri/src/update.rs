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
    if crate::run_log::enabled(crate::run_log::Level::Debug) {
        crate::run_log::debug(&format!(
            "GET {RELEASES_URL} -> {status}\n{}",
            crate::run_log::clip_body(&body)
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
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap_or_else(|| panic!("{s} 应能解析"))
    }

    /// 核心号逐段按数值比较，不是按字符串
    #[test]
    fn core_numbers_compare_numerically() {
        assert!(v("0.0.10") > v("0.0.9"));
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert_eq!(v("1.2.3"), v("v1.2.3"));
        assert_eq!(v("1.2"), v("1.2.0"));
    }

    /// 预发布版低于同核心号的正式版——这是 semver 的规则，也是本项目的实际形态：
    /// 0.0.2-dev.6 是从 0.0.2 的代码构建的开发版，正式版才是更新的那个
    #[test]
    fn prerelease_is_lower_than_the_release() {
        assert!(v("0.0.2") > v("0.0.2-dev.6"));
        assert!(v("0.0.2-dev.6") < v("0.0.2"));
    }

    /// 预发布标识里的数字按数值比较（dev.10 比 dev.6 新），不能按字符串
    #[test]
    fn prerelease_numbers_compare_numerically() {
        assert!(v("0.0.2-dev.10") > v("0.0.2-dev.6"));
        assert!(v("0.0.2-dev.6") > v("0.0.1-dev.3"));
    }

    /// 数字标识低于文本标识；标识多的一方更大
    #[test]
    fn prerelease_identifiers_follow_semver_order() {
        assert!(v("1.0.0-alpha") < v("1.0.0-alpha.1"));
        assert!(v("1.0.0-alpha.1") < v("1.0.0-alpha.beta"));
        assert!(v("1.0.0-alpha.beta") < v("1.0.0-beta"));
        assert!(v("1.0.0-beta.2") < v("1.0.0-beta.11"));
        assert!(v("1.0.0-rc.1") < v("1.0.0"));
    }

    /// 版本性质：带 `-` 后缀的是预览版，纯 x.y.z 的是正式版。
    ///
    /// 界面据此标注「预览版／正式版」，让用户自己决定要不要跨性质更新，
    /// 因此这条判定必须与发布链路一致。
    #[test]
    fn prerelease_kind_follows_the_dash_suffix() {
        assert!(v("0.1.1-dev.13").is_prerelease());
        assert!(v("v0.0.2-rc.1").is_prerelease());
        assert!(!v("0.1.0").is_prerelease(), "纯版本号是正式版");
        assert!(!v("v1.2.3").is_prerelease(), "前导 v 不影响性质");
        assert!(!v("1.2.3+build.9").is_prerelease(), "构建元数据不是预发布标识");
    }

    /// 不合规范的 tag 解析失败，而不是被当成 0.0.0
    #[test]
    fn malformed_versions_are_rejected() {
        for bad in ["", "v", "dev.6", "1.2.3.4", "a.b.c", "1.0.0-"] {
            assert!(Version::parse(bad).is_none(), "{bad} 不该被解析成版本号");
        }
        // 构建元数据不参与比较
        assert_eq!(v("1.2.3+build.9"), v("1.2.3"));
    }

    /// 发布列表：跳过草稿、跳出版本号不合规的条目
    #[test]
    fn releases_skip_drafts_and_bad_tags() {
        let json = r#"[
            {"tag_name":"v0.0.2-dev.6","name":"0.0.2-dev.6","html_url":"https://x/6","published_at":"2026-10-16T18:27:53Z","draft":false},
            {"tag_name":"v0.0.3","name":"草稿","html_url":"https://x/d","draft":true},
            {"tag_name":"nightly","name":"非版本 tag","html_url":"https://x/n","draft":false},
            {"tag_name":"v0.0.1-dev.3","name":"0.0.1-dev.3","html_url":"https://x/3","published_at":"2026-10-15T19:00:54Z"}
        ]"#;
        let list = parse_releases(json).expect("应能解析");
        assert_eq!(list.len(), 2, "草稿与非版本 tag 都要跳过");
        let top = newest(list).expect("应有最新发布");
        assert_eq!(top.tag, "v0.0.2-dev.6");
        assert_eq!(top.url, "https://x/6");
        assert_eq!(top.published_at, "2026-10-16T18:27:53Z");
    }

    /// 检测结论：比当前高才算有更新
    #[test]
    fn outcome_reports_update_only_when_newer() {
        let rel = |tag: &str| Release {
            tag: tag.to_string(),
            version: v(tag),
            name: tag.to_string(),
            url: String::new(),
            published_at: String::new(),
        };
        let with = |cur: &str, tag: &str| CheckOutcome {
            current: v(cur),
            newest: Some(rel(tag)),
        };
        assert!(with("0.0.1", "v0.0.2-dev.6").has_update(), "低于发布 → 有更新");
        assert!(!with("0.0.2", "v0.0.2-dev.6").has_update(), "正式版高于同名开发版");
        assert!(!with("0.0.3", "v0.0.2-dev.6").has_update(), "已高于发布 → 无更新");
        assert!(
            !CheckOutcome { current: v("0.0.2"), newest: None }.has_update(),
            "查不到发布时不能说有更新"
        );
    }

    /// 发布列表不是数组（例如接口返回了错误对象）要明确报错
    #[test]
    fn non_array_payload_is_an_error() {
        assert!(parse_releases(r#"{"message":"Not Found"}"#).is_err());
        assert!(parse_releases("不是 JSON").is_err());
    }

    /// 当前版本号必须能被解析——它是检测的基准
    #[test]
    fn current_version_parses() {
        assert!(Version::parse(current_version()).is_some());
    }
}
