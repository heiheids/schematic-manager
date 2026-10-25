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
