use super::*;
use crate::test_support::temp_state_dir;

/// 重复筛查必须走进子目录。
///
/// 原先只读目录第一层：投影按分类放进子目录时，列表里明明有成对的副本，
/// 筛查却一律报「没有发现重复文件」。用户的投影正是按版本分类放在
/// 子目录里的，这条守住那个场景。
#[test]
fn duplicate_scan_recurses_into_subdirectories() {
    let root = temp_state_dir("dup-recurse");
    let sub = root.join("1.21.1");
    std::fs::create_dir_all(&sub).expect("建子目录失败");
    std::fs::write(sub.join("a.litematic"), b"same bytes").unwrap();
    std::fs::write(sub.join("b.litematic"), b"same bytes").unwrap();
    std::fs::write(root.join("c.litematic"), b"different!!").unwrap();

    let r = scan_duplicates_in(&[root.to_string_lossy().into_owned()]);

    assert_eq!(r.scanned, 3, "子目录里的文件也要计入筛查范围");
    assert_eq!(r.groups.len(), 1, "子目录里的一对副本应被识别");
    assert_eq!(r.groups[0].paths.len(), 2);
    assert!(r.unreadable.is_empty(), "不应有读取失败：{:?}", r.unreadable);

    let _ = std::fs::remove_dir_all(&root);
}

/// 跨目录的同一份内容必须归到同一组。
///
/// 原先按目录分别筛查再拼接：跨目录的副本各自只有一个成员，
/// 而只有一个成员的组会被丢掉，于是「同一份投影复制到两个目录」
/// 这种最常见的重复反而查不出来。
#[test]
fn duplicate_scan_groups_copies_across_directories() {
    let a = temp_state_dir("dup-cross-a");
    let b = temp_state_dir("dup-cross-b");
    std::fs::write(a.join("x.litematic"), b"copy of one").unwrap();
    std::fs::write(b.join("y.litematic"), b"copy of one").unwrap();
    std::fs::write(b.join("z.litematic"), b"something else").unwrap();

    let dirs = vec![
        a.to_string_lossy().into_owned(),
        b.to_string_lossy().into_owned(),
    ];
    let r = scan_duplicates_in(&dirs);

    assert_eq!(r.scanned, 3);
    assert_eq!(r.groups.len(), 1, "两个目录里的同一份内容应合成一组");
    assert_eq!(r.groups[0].paths.len(), 2, "组内应含两个目录各一份");

    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}

/// 父子目录同时被加载时，同一个文件只能算一份候选。
///
/// 否则组里会出现同一个路径两次，「多余的副本」会被多算，
/// 默认勾选也就跟着出错。
#[test]
fn duplicate_scan_counts_each_file_once() {
    let root = temp_state_dir("dup-overlap");
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).expect("建子目录失败");
    std::fs::write(sub.join("a.litematic"), b"only one").unwrap();

    let dirs = vec![
        root.to_string_lossy().into_owned(),
        sub.to_string_lossy().into_owned(),
    ];
    let r = scan_duplicates_in(&dirs);

    assert_eq!(r.scanned, 1, "父子目录叠加不应把同一个文件算两次");
    assert!(r.groups.is_empty(), "只有一份的文件不该成组");

    let _ = std::fs::remove_dir_all(&root);
}

/// 异步命令外壳本身也要能真的出结果。
///
/// 上面几条直接调同步实现，覆盖不到命令外壳：`spawn_blocking` 要 await
/// 才拿得到值、错误要经 JoinHandle 再映射一次，写成 `?` 而不是 `.await?`
/// 这类问题只有真跑一遍才暴露。
#[test]
fn duplicate_scan_command_runs_on_the_async_runtime() {
    let root = temp_state_dir("dup-cmd");
    std::fs::write(root.join("a.litematic"), b"same").unwrap();
    std::fs::write(root.join("b.litematic"), b"same").unwrap();

    let dirs = vec![root.to_string_lossy().into_owned()];
    let r = tauri::async_runtime::block_on(scan_duplicates(dirs)).expect("命令应返回结果");

    assert_eq!(r.scanned, 2);
    assert_eq!(r.groups.len(), 1, "命令外壳拿到的结果应与同步实现一致");

    let _ = std::fs::remove_dir_all(&root);
}
