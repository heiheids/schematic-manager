use super::*;

fn scan_test_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("testtmp")
        .join(format!("sm_scan_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

// 注意：这里的路径必须与平台无关。    // std::path::Path 按各平台的分隔符切分，在 Unix 上反斜杠只是普通字符，
// 因此 Windows 风格的 "Z:\\x.litematic" 在 Linux 上会被整体当作文件名。
// 测试只关心「文件不存在时的行为」，用纯文件名即可。
#[test]
fn scan_missing_dir_returns_empty() {
    let r = scan_litematics("__sm_not_exist_dir__".into(), Some(2));
    assert!(r.items.is_empty());
    assert!(r.skipped_dirs > 0, "不存在的目录应计入跳过数");
    assert_eq!(r.root.path, "__sm_not_exist_dir__");
    assert_eq!(r.root.name, "__sm_not_exist_dir__");
    assert!(r.root.relative_path.is_empty());
    assert!(r.root.file_paths.is_empty());
    assert!(r.root.children.is_empty());
    assert!(!r.root.truncated);
}

#[test]
fn peek_missing_file_reports_error() {
    let s = peek_schematic("__sm_not_exist__.litematic".into());
    assert!(!s.error.is_empty(), "文件不存在时应给出错误原因");
    assert_eq!(s.file_name, "__sm_not_exist__.litematic");
}

#[test]
fn scan_finds_litematic_files_only() {
    let dir = std::env::temp_dir().join("sm_scan_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("a.litematic"), b"x").unwrap();
    std::fs::write(dir.join("b.txt"), b"x").unwrap();
    std::fs::write(dir.join("sub/c.LITEMATIC"), b"x").unwrap();

    let r = scan_litematics(dir.to_string_lossy().into_owned(), Some(4));
    let names: Vec<&str> = r.items.iter().map(|s| s.file_name.as_str()).collect();
    assert_eq!(names.len(), 2, "应只收录 .litematic，实际 {names:?}");
    assert!(names.contains(&"a.litematic"));
    assert!(names.contains(&"c.LITEMATIC"), "扩展名匹配应忽略大小写");
    assert_eq!(r.skipped_dirs, 0, "正常目录不应产生跳过");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scan_empty_directory_returns_an_empty_root_node() {
    let dir = scan_test_dir("empty");
    let r = scan_litematics(path_string(&dir), Some(4));

    assert_eq!(r.root.path, path_string(&dir));
    assert_eq!(r.root.name, dir.file_name().unwrap().to_string_lossy());
    assert!(r.root.relative_path.is_empty());
    assert!(r.root.file_paths.is_empty());
    assert!(r.root.children.is_empty());
    assert!(!r.root.truncated);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scan_tree_keeps_directories_that_only_contain_directories() {
    let dir = scan_test_dir("directories");
    std::fs::create_dir_all(dir.join("only/empty")).unwrap();
    let r = scan_litematics(path_string(&dir), Some(4));

    assert!(r.items.is_empty());
    assert_eq!(r.root.children.len(), 1);
    let only = &r.root.children[0];
    assert_eq!(only.name, "only");
    assert_eq!(only.relative_path, path_string(Path::new("only")));
    assert!(only.file_paths.is_empty());
    assert_eq!(only.children.len(), 1);
    let empty = &only.children[0];
    assert_eq!(empty.name, "empty");
    assert_eq!(empty.relative_path, path_string(&Path::new("only").join("empty")));
    assert!(empty.children.is_empty());
    assert!(empty.file_paths.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scan_tree_keeps_nested_files_on_their_direct_parent() {
    let dir = scan_test_dir("nested");
    std::fs::create_dir_all(dir.join("child/nested")).unwrap();
    std::fs::write(dir.join("root.litematic"), b"x").unwrap();
    std::fs::write(dir.join("notes.txt"), b"x").unwrap();
    std::fs::write(dir.join("child/nested/deep.LITEMATIC"), b"x").unwrap();
    let r = scan_litematics(path_string(&dir), Some(5));

    assert_eq!(r.items.len(), 2);
    assert_eq!(r.root.file_paths, vec![path_string(&dir.join("root.litematic"))]);
    let child = r.root.children.iter().find(|n| n.name == "child").unwrap();
    assert!(child.file_paths.is_empty());
    let nested = child.children.iter().find(|n| n.name == "nested").unwrap();
    assert_eq!(
        nested.file_paths,
        vec![path_string(&dir.join("child").join("nested").join("deep.LITEMATIC"))]
    );
    assert!(nested.children.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scan_tree_marks_the_depth_cutoff_without_reading_past_it() {
    let dir = scan_test_dir("depth");
    std::fs::create_dir_all(dir.join("one/two/three")).unwrap();
    std::fs::write(dir.join("root.litematic"), b"x").unwrap();
    std::fs::write(dir.join("one/one.litematic"), b"x").unwrap();
    std::fs::write(dir.join("one/two/two.litematic"), b"x").unwrap();
    let r = scan_litematics(path_string(&dir), Some(2));

    assert_eq!(r.items.len(), 2, "深度截止处的内容不应进入文件列表");
    let one = r.root.children.iter().find(|n| n.name == "one").unwrap();
    assert!(!one.truncated);
    assert_eq!(one.file_paths, vec![path_string(&dir.join("one").join("one.litematic"))]);
    let two = one.children.iter().find(|n| n.name == "two").unwrap();
    assert!(two.truncated);
    assert!(two.file_paths.is_empty());
    assert!(two.children.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn analysis_includes_region_summaries_without_changing_counts() {
    let air = litematic::BlockState::new("minecraft:air");
    let stone = litematic::BlockState::new("minecraft:stone");
    let sc = litematic::Schematic {
        root: crate::format::nbt::Value::Compound(Vec::new()),
        regions: vec![
            litematic::Region {
                name: "north".into(),
                pos: [-4, 2, 8],
                size: [3, 1, 2],
                volume: 6,
                bits: 2,
                palette: vec![air, stone.clone()],
                indices: vec![1, 0, 0, 0, 0, 0],
                ..Default::default()
            },
            litematic::Region {
                name: "south".into(),
                pos: [7, -1, 3],
                size: [1, 2, 1],
                volume: 2,
                bits: 2,
                palette: vec![stone],
                indices: vec![0, 0],
                ..Default::default()
            },
        ],
        path: String::new(),
        meta_modified: false,
        blocks_modified: false,
    };
    let result = analyze_loaded(&sc, &crate::block::db::BlockDb::empty());

    assert_eq!(result.regions.len(), 2);
    assert_eq!(result.regions[0].name, "north");
    assert_eq!(result.regions[0].position, [-4, 2, 8]);
    assert_eq!(result.regions[0].size, [3, 1, 2]);
    assert_eq!(result.regions[1].name, "south");
    assert_eq!(result.regions[1].position, [7, -1, 3]);
    assert_eq!(result.regions[1].size, [1, 2, 1]);
    assert_eq!(result.non_air_total, 3);
    assert_eq!(result.air_total, 5);
    assert_eq!(result.blocks.len(), 1);
    assert_eq!(result.blocks[0].count, 3);
    assert_eq!(result.distinct_block_types, 1);
    assert_eq!(result.distinct_states, 1);
}

#[test]
fn mc_version_command_proxies_table() {
    assert!(!mc_version_name(3465).is_empty());
    assert_eq!(mc_version_name(-1), "");
}

/// 超长元数据必须在写入之前被拒绝。
///
/// NBT 的字符串长度字段是 u16：超限时若只截断长度字段，写出的文件
/// 记录长度与实际字节数不符，后续字段全部错位，而写入路径的终点是
/// 覆盖用户的 .litematic。因此这里要求「明确报错」而不是「写出坏文件」。
#[test]
fn oversized_metadata_is_rejected_before_writing() {
    let too_long = "啊".repeat(20_001);
    let err = save_metadata(
        "__sm_not_exist__.litematic".into(),
        None,
        None,
        Some(too_long),
    )
    .expect_err("超长描述应被拒绝");
    assert!(
        err.contains("过长"),
        "错误信息应指出超长，实际：{err}"
    );
}

/// 三个字段都要校验，不能只检查描述。
#[test]
fn oversized_name_and_author_are_rejected_too() {
    let long = "x".repeat(20_001);
    for (label, name, author) in [
        ("名称", Some(long.clone()), None),
        ("作者", None, Some(long.clone())),
    ] {
        let err = save_metadata(
            "__sm_not_exist__.litematic".into(),
            name,
            author,
            None,
        )
        .expect_err(&format!("超长{label}应被拒绝"));
        assert!(
            err.contains("过长") && err.contains(label),
            "错误信息应指明是哪一项超长，实际：{err}"
        );
    }
}

/// 恰好等于上限时不应因长度被拒。
///
/// 路径不存在，因此会以「文件不存在」失败——关键是不能再报「过长」，
/// 否则说明上限被判成了 >=。
#[test]
fn metadata_at_exact_limit_is_not_rejected_for_length() {
    let ok = "x".repeat(20_000);
    let err = save_metadata(
        "__sm_not_exist__.litematic".into(),
        None,
        None,
        Some(ok),
    )
    .expect_err("文件不存在应报错");
    assert!(
        !err.contains("过长"),
        "恰好达上限不应被判超长，实际：{err}"
    );
    assert!(err.contains("不存在"), "应报文件不存在，实际：{err}");
}
