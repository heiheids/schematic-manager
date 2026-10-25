use super::*;

#[test]
fn table_is_sorted_and_unique() {
    for w in DATA_VERSIONS.windows(2) {
        assert!(w[0].0 < w[1].0, "dataVersion 未严格递增: {:?} {:?}", w[0], w[1]);
    }
}

#[test]
fn lookup_exact_below_and_between() {
    let (first_dv, first_name) = DATA_VERSIONS[0];
    assert_eq!(version_name_from_data_version(first_dv), first_name);
    // 低于整张表
    assert_eq!(version_name_from_data_version(0), "");
    assert_eq!(version_name_from_data_version(first_dv - 1), "");

    // 真正落在相邻两条目之间：应取较小的那个
    if DATA_VERSIONS.len() >= 2 {
        let (lo_dv, lo_name) = DATA_VERSIONS[0];
        let (hi_dv, _) = DATA_VERSIONS[1];
        if hi_dv > lo_dv + 1 {
            assert_eq!(
                version_name_from_data_version(lo_dv + 1),
                lo_name,
                "两条目之间应取不大于给定值的最大条目"
            );
        }
    }
}

#[test]
fn unknown_high_version_returns_last() {
    let last = DATA_VERSIONS.last().unwrap();
    assert_eq!(version_name_from_data_version(last.0 + 100000), last.1);
}

/// 用外部已知的版本号钉住查表结果。
///
/// 上面三条测试的期望值都取自 `DATA_VERSIONS` 自身，只能验证「查表
/// 逻辑与表结构自洽」。若表里某个版本名写错（例如把 3465 写成
/// "1.20.2"），它们全部照样通过——而错误会一路传到界面显示与
/// 版本转换的目标选择上。
///
/// 这里写死若干 dataVersion 与版本名的对应关系。它们来自 Minecraft
/// 官方的 dataVersion 编号（每个正式版发布时确定，此后不再变动），
/// 不是从这张表读出来的，因此能真正发现表里的笔误。
#[test]
fn well_known_data_versions_map_to_expected_names() {
    let anchors = [
        (3465, "1.20.1"),
        (3120, "1.19.2"),
        (2975, "1.18.2"),
        (2586, "1.16.5"),
        (2230, "1.15.2"),
        (1976, "1.14.4"),
        (1343, "1.12.2"),
    ];
    for (dv, want) in anchors {
        assert_eq!(
            version_name_from_data_version(dv),
            want,
            "dataVersion {dv} 应为 {want}，表里写成了别的版本名"
        );
    }
}

/// 表里每个版本名都不得为空、不得带首尾空白、应为可打印 ASCII。
///
/// 注意：这张表**有意**同时收录正式版与快照。实测 159 条里有 101 条是
/// 快照命名（`15w14a`、`19w13_3d_shareware_v1.34`、`20w14~` 等），
/// 形式五花八门，因此不能要求「必须是 `<数字>.<数字>[.<数字>]`」——
/// 那样会把合法的快照名误判为错误。
///
/// 这里只断言所有条目都成立的性质，用来抓「多打了空格」「写成空串」
/// 「混入不可见字符」这类笔误。这些错误会被上面基于表自身的测试放过。
#[test]
fn every_version_name_is_clean() {
    for (dv, name) in DATA_VERSIONS {
        assert!(!name.is_empty(), "dataVersion {dv} 的版本名为空");
        assert_eq!(
            name.trim(),
            *name,
            "dataVersion {dv} 的版本名 {name:?} 首尾有空白"
        );
        assert!(
            name.chars().all(|c| c.is_ascii_graphic() || c == ' '),
            "dataVersion {dv} 的版本名 {name:?} 含不可见或非 ASCII 字符"
        );
        assert!(
            !name.contains("  "),
            "dataVersion {dv} 的版本名 {name:?} 含连续空格"
        );
    }
}

/// 正式版命名的条目必须是 `<数字>.<数字>[.<数字>]` 形式。
///
/// 判据是「以数字开头且不含空格」——快照名以两位年份开头（如 `15w14a`），
/// 也以数字开头，因此这里进一步要求整串只由数字与点组成；
/// 快照名含字母或下划线，自然被排除在外。
#[test]
fn release_style_names_are_wellformed() {
    let mut checked = 0;
    for (dv, name) in DATA_VERSIONS {
        let release_like = name.chars().all(|c| c.is_ascii_digit() || c == '.');
        if !release_like {
            continue; // 快照命名，跳过
        }
        checked += 1;
        let parts: Vec<&str> = name.split('.').collect();
        assert!(
            (2..=3).contains(&parts.len()),
            "dataVersion {dv} 的版本名 {name:?} 段数应为 2 或 3"
        );
        for p in &parts {
            assert!(
                !p.is_empty(),
                "dataVersion {dv} 的版本名 {name:?} 含空段"
            );
        }
    }
    assert!(
        checked >= 20,
        "只校验到 {checked} 条正式版命名，说明判据可能写错了"
    );
}

/// 表必须覆盖到较新的版本，避免忘记补充新版本后静默退化。
///
/// 判据用「不小于已知的较新版本」，而不是断言某个固定最大值——
/// 后者会在每次补新版本时被迫改测试。
#[test]
fn table_covers_recent_versions() {
    let (max_dv, _) = *DATA_VERSIONS.last().unwrap();
    assert!(
        max_dv >= 3465,
        "表的最新 dataVersion 为 {max_dv}，应至少覆盖 1.20.1（3465）"
    );
}
