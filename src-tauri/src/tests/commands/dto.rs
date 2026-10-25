use super::*;

/// 取排序后的键名。
fn keys(v: &serde_json::Value) -> Vec<String> {
    let mut k: Vec<String> = v
        .as_object()
        .expect("应是对象")
        .keys()
        .cloned()
        .collect();
    k.sort();
    k
}

/// 断言键名集合（**忽略顺序**）。
///
/// 两端都排序：手写顺序极易出错，例如 `failedBlocks` 是
/// `failedBlockstates` 的前缀、排序时在前，而按业务语序写就会写成
/// 相反的顺序；这里按排序后的集合断言，顺序写错不会成为漏网之鱼。
fn assert_keys(v: &serde_json::Value, expected: &[&str]) {
    let mut want: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(keys(v), want);
}

/// 前端的字段名是驼峰；后端靠 `#[serde(rename_all = "camelCase")]` 转换。
/// 这条契约没有任何编译期保障：改名或漏写属性都不会编译失败，
/// 只会在运行时表现为「前端读到的字段是 undefined」。
///
/// 因此这里对每个跨边界的 DTO 断言其**序列化后的键名**，
/// 与前端 `src/types.ts` 的字段逐一对应。
///
/// 注意：本地检查环境用 serde 桩（无过程宏，rename_all 不生效），
/// 该测试只在 CI 的真实 serde 下运行。
/// 纳秒时间戳必须以字符串传递：数值会超出 JavaScript 的安全整数范围
#[test]
fn backup_entry_created_ns_is_a_string() {
    let dto = BackupEntryDto {
        path: "/b/x".into(),
        stem: "x".into(),
        op: "编辑".into(),
        created_ns: "1791826869720983600".into(),
        size: 12,
    };
    let v = serde_json::to_value(&dto).expect("序列化失败");
    assert!(
        v.get("createdNs").expect("应有 createdNs").is_string(),
        "createdNs 必须是字符串，用数值会丢精度"
    );
}

#[test]
fn convertible_keys_are_camel_case() {
    // 抽查几个既有 DTO，防止后续改动破坏既有契约
    let save = SaveOutcome {
        path: "/x".into(),
        backup_path: "/b/x".into(),
    };
    let v = serde_json::to_value(&save).unwrap();
    assert_keys(&v, &["backupPath", "path"]);

    let ver = VersionOptionDto {
        index: 0,
        name: "1.20.1".into(),
        data_version: 3465,
        is_overlay: false,
        block_count: 1005,
    };
    let v = serde_json::to_value(&ver).unwrap();
    assert_keys(&v, &["blockCount", "dataVersion", "index", "isOverlay", "name"]);
}

#[test]
fn update_info_keys_are_camel_case() {
    let info = UpdateInfoDto {
        current: "0.0.2".into(),
        latest: "0.0.2-dev.6".into(),
        tag: "v0.0.2-dev.6".into(),
        has_update: false,
        release_url: "https://github.com/x/y/releases/tag/v0.0.2-dev.6".into(),
        published_at: "2026-10-16T18:27:53Z".into(),
        repo: "heiheids/schematic-manager".into(),
        current_prerelease: false,
        latest_prerelease: true,
    };
    let v = serde_json::to_value(&info).unwrap();
    assert_keys(
        &v,
        &[
            "current",
            "currentPrerelease",
            "latest",
            "latestPrerelease",
            "tag",
            "hasUpdate",
            "releaseUrl",
            "publishedAt",
            "repo",
        ],
    );
}
