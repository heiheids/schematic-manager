use super::*;

/// 测试用临时目录。
/// 不用 std::env::temp_dir()：该路径在部分受限环境下不可写，
/// 会让测试因环境而失败。放在工程内的 target 下更可控。
fn temp_dir(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("testtmp")
        .join(format!("sm_backup_{tag}_{}", timestamp()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn create_copies_file_into_stem_subdir() {
    let base = temp_dir("basic");
    let src = base.join("demo.litematic");
    std::fs::write(&src, b"hello").unwrap();

    let out = create(&base, &src, "metadata").unwrap();
    assert!(out.backup_path.exists(), "备份文件应存在");
    assert_eq!(std::fs::read(&out.backup_path).unwrap(), b"hello");

    // 备份应位于 backups/<主干>/ 之下
    let rel = out.backup_path.strip_prefix(backup_root(&base)).unwrap();
    assert_eq!(rel.components().count(), 2, "应有一层以文件主干命名的子目录");
    let name = out.backup_path.file_name().unwrap().to_string_lossy();
    assert!(name.starts_with("demo__"));
    assert!(name.ends_with("__metadata.litematic"));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn create_for_missing_source_errors() {
    let base = temp_dir("missing");
    let r = create(&base, &base.join("nope.litematic"), "metadata");
    assert!(r.is_err(), "源文件不存在时必须报错，不能静默产生空备份");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn repeated_backups_do_not_overwrite() {
    let base = temp_dir("repeat");
    let src = base.join("a.litematic");
    std::fs::write(&src, b"1").unwrap();
    let first = create(&base, &src, "metadata").unwrap();
    // 内容改成不同值后再次备份，两份都应保留
    std::fs::write(&src, b"2").unwrap();
    let second = create(&base, &src, "metadata").unwrap();

    assert!(first.backup_path.exists());
    assert!(second.backup_path.exists());
    // 只断言「两个文件都存在」测不出覆盖：路径相同时这两条依然通过，
    // 因此必须显式断言路径不同。
    assert_ne!(
        first.backup_path, second.backup_path,
        "两次备份必须落在不同文件，否则后者会静默覆盖前者"
    );
    let _ = std::fs::remove_dir_all(&base);
}
#[test]
fn parse_backup_name_handles_underscores_in_stem() {
    // 主干本身含 "__" 时，必须从右侧切分
    let r = parse_backup_name("我的__投影__1700000000000__metadata.litematic");
    assert_eq!(r, Some(("我的__投影".to_string(), 1700000000000, "metadata".to_string())));
    assert!(parse_backup_name("没有分隔符.litematic").is_none());
    assert!(parse_backup_name("a__notanumber__op.litematic").is_none());
    assert!(parse_backup_name("a__1__op.txt").is_none(), "扩展名不符应拒绝");
}

#[test]
fn list_finds_backups_and_sorts_newest_first() {
    let base = temp_dir("list");
    let root = backup_root(&base);
    let src = base.join("demo.litematic");
    std::fs::write(&src, b"x").unwrap();
    let first = create(&base, &src, "metadata").unwrap();

    // 手工造一条时间戳更大的备份，验证排序。
    // 注意真实备份用的是纳秒时间戳（19 位），这里取 u64::MAX 才确保更大；
    // 早先写成毫秒量级的 13 位数字，反而比真实备份小。
    let dir = root.join("demo");
    std::fs::create_dir_all(&dir).unwrap();
    let newer = dir.join(format!("demo__{}__replace.litematic", u64::MAX));
    std::fs::write(&newer, b"y").unwrap();

    let items = list(&root);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].path, newer, "最新的应排在最前");
    assert_eq!(items[0].op, "replace");
    assert_eq!(items[1].path, first.backup_path);
    assert_eq!(items[0].size, 1);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn restore_copies_content_back() {
    let base = temp_dir("restore");
    let root = backup_root(&base);
    let src = base.join("a.litematic");
    std::fs::write(&src, b"original").unwrap();
    let b = create(&base, &src, "metadata").unwrap();
    std::fs::write(&src, b"modified").unwrap();

    restore(&b.backup_path, &src).unwrap();
    assert_eq!(std::fs::read(&src).unwrap(), b"original");

    // 备份不存在时必须报错，而不是产生空文件
    let missing = root.join("a").join("nope.litematic");
    let e = restore(&missing, &src).unwrap_err().to_string();
    assert!(e.contains("不存在"), "应说明备份不存在：{e}");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn delete_removes_backup_and_empty_group() {
    let base = temp_dir("delete");
    let root = backup_root(&base);
    let src = base.join("b.litematic");
    std::fs::write(&src, b"x").unwrap();
    let b = create(&base, &src, "metadata").unwrap();
    assert!(b.backup_path.exists());

    delete(&b.backup_path).unwrap();
    assert!(!b.backup_path.exists());
    assert!(!root.join("b").exists(), "空的分组目录应被清理");

    assert!(delete(&b.backup_path).is_err(), "删除不存在的备份应报错");
    let _ = std::fs::remove_dir_all(&base);
}

/// 同一文件的多份备份共用一个分组与一份 .orig_path。
/// 删掉其中一份不能影响其余备份的还原能力——否则用户删了旧备份之后，
/// 想还原较新的那份会被「不同源」错误地拒绝。
#[test]
fn deleting_one_backup_keeps_others_restorable() {
    let base = temp_dir("delete_keeps_siblings");
    let src = base.join("demo.litematic");
    std::fs::write(&src, b"v1").unwrap();
    let older = create(&base, &src, "metadata").unwrap();

    std::fs::write(&src, b"v2").unwrap();
    let newer = create(&base, &src, "replace").unwrap();

    // 两份应在同一分组（同一文件的两次备份）
    assert_eq!(older.backup_path.parent(), newer.backup_path.parent());

    delete(&older.backup_path).unwrap();

    // 剩下那份必须仍可还原
    assert!(
        matches_target(&newer.backup_path, &src),
        "同组还有备份时，不能把 .orig_path 一起删掉，否则其余备份永远无法还原"
    );
    restore(&newer.backup_path, &src).unwrap();
    assert_eq!(std::fs::read(&src).unwrap(), b"v2");

    // 删掉最后一份时才应清理分组目录
    delete(&newer.backup_path).unwrap();
    assert!(!newer.backup_path.parent().unwrap().exists(), "最后一份删掉后分组目录应被清理");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn matches_target_requires_same_original_path() {
    let base = temp_dir("match");
    let root = backup_root(&base);

    // 两个不同目录下的同名文件
    let dir_a = base.join("A");
    let dir_b = base.join("B");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    let fa = dir_a.join("demo.litematic");
    let fb = dir_b.join("demo.litematic");
    std::fs::write(&fa, b"a").unwrap();
    std::fs::write(&fb, b"b").unwrap();

    let ba = create(&base, &fa, "metadata").unwrap();

    // 同源：完整路径一致
    assert!(matches_target(&ba.backup_path, &fa));
    // 跨目录同名文件绝不能被判为同源，否则 A 的备份会盖到 B 上
    assert!(
        !matches_target(&ba.backup_path, &fb),
        "A/demo.litematic 的备份不能还原到 B/demo.litematic"
    );
    // 文件名不合规时判为不匹配，而不是默认通过
    assert!(!matches_target(&dir_a.join("demo.litematic"), &fa));
    // 记录缺失时也不放行
    let orphan = root.join("x").join("x__1__metadata.litematic");
    std::fs::create_dir_all(orphan.parent().unwrap()).unwrap();
    std::fs::write(&orphan, b"x").unwrap();
    assert!(!matches_target(&orphan, &fa));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn same_name_in_different_dirs_gets_separate_groups() {
    let base = temp_dir("groups");
    let dir_a = base.join("A");
    let dir_b = base.join("B");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    let fa = dir_a.join("demo.litematic");
    let fb = dir_b.join("demo.litematic");
    std::fs::write(&fa, b"a").unwrap();
    std::fs::write(&fb, b"b").unwrap();

    let ba = create(&base, &fa, "metadata").unwrap();
    let bb = create(&base, &fb, "metadata").unwrap();

    assert_ne!(
        ba.backup_path.parent(),
        bb.backup_path.parent(),
        "同名但不同目录的文件不能混进同一分组"
    );
    // 各自的备份只能还原到自己的原文件
    assert!(matches_target(&ba.backup_path, &fa));
    assert!(matches_target(&bb.backup_path, &fb));
    assert!(!matches_target(&ba.backup_path, &fb));

    let _ = std::fs::remove_dir_all(&base);
}

/// 重命名后备份必须仍然「跟着」这个文件。
///
/// 这条覆盖三处依赖旧名字的关联：分组目录名、分组内的 .orig_path、
/// 备份文件名里的主干。任何一处没跟上，用户都会看到备份凭空消失，
/// 或者还原时被拒绝为「不同源」。
#[test]
fn relink_follows_rename_so_backups_stay_usable() {
    let base = temp_dir("relink");
    let root = backup_root(&base);
    let old = base.join("old.litematic");
    std::fs::write(&old, b"v1").unwrap();
    let b = create(&base, &old, "metadata").unwrap();

    // 改名之前：备份认这个文件
    assert!(matches_target(&b.backup_path, &old));

    let new = base.join("新名字.litematic");
    std::fs::rename(&old, &new).unwrap();
    let n = relink(&root, &old, &new).unwrap();
    assert_eq!(n, 1, "应重新链接 1 个分组");

    // 备份文件本身也被改名了，因此后面的比对要用 list() 给出的**当前**路径
    // （b.backup_path 指向的旧路径已经不存在了）
    let items = list(&root);
    assert_eq!(items.len(), 1, "备份文件本身不能丢");
    assert_eq!(items[0].stem, "新名字", "备份文件名里的主干未跟随");
    assert!(matches_target(&items[0].path, &new), ".orig_path 未跟随改名");
    assert!(!matches_target(&items[0].path, &old), "不该还认旧路径");

    // 内容与还原能力都要保住
    assert_eq!(std::fs::read(&items[0].path).unwrap(), b"v1");
    assert!(!b.backup_path.exists(), "旧路径的备份应已随分组一起移走");
    let _ = std::fs::remove_dir_all(&base);
}

/// 只改大小写时的重新链接。
///
/// 这条防的是把刚写好的 .orig_path 连同目录一起删掉——
/// 那样备份文件还在、却再也无法还原。
///
/// 分组目录名取自 list() 给出的**当前**路径，不能用 create() 当时返回的
/// 旧路径：在大小写敏感的文件系统上，分组目录会随改名从 `demo_<哈希>`
/// 变成 `DEMO_<哈希>`，旧目录按设计已被清理，拿旧路径断言必然落空。
#[test]
fn relink_handles_case_only_rename_in_place() {
    let base = temp_dir("relink_case");
    let root = backup_root(&base);
    let old = base.join("demo.litematic");
    std::fs::write(&old, b"v1").unwrap();
    create(&base, &old, "metadata").unwrap();

    let new = base.join("DEMO.litematic");
    std::fs::rename(&old, &new).unwrap();
    let n = relink(&root, &old, &new).unwrap();
    assert_eq!(n, 1);

    // 改名后仍只有一组备份，且分组目录与 .orig_path 都落在新名字下
    let items = list(&root);
    assert_eq!(items.len(), 1, "改名不该多出或丢掉分组");
    assert_eq!(items[0].stem, "DEMO");
    let dir = items[0].path.parent().unwrap();
    assert!(dir.exists(), "分组目录不能被自己删掉");
    assert!(
        dir.join(ORIG_PATH_FILE).exists(),
        ".orig_path 不能被自己删掉，否则备份永远无法还原"
    );
    assert!(matches_target(&items[0].path, &new), ".orig_path 应指向新路径");

    // 备份内容与还原能力都要保住：把目标文件改脏再还原，应回到备份里那份
    std::fs::write(&new, b"dirty").unwrap();
    restore(&items[0].path, &new).unwrap();
    assert_eq!(std::fs::read(&new).unwrap(), b"v1", "重新链接后的备份应仍可还原");
    let _ = std::fs::remove_dir_all(&base);
}

/// 同一个文件多次改名的备份要能累积在一起。
#[test]
fn relink_keeps_every_backup_across_repeated_renames() {
    let base = temp_dir("relink_twice");
    let root = backup_root(&base);
    let a = base.join("a.litematic");
    std::fs::write(&a, b"v1").unwrap();
    let b1 = create(&base, &a, "metadata").unwrap();

    let b_path = base.join("b.litematic");
    std::fs::rename(&a, &b_path).unwrap();
    relink(&root, &a, &b_path).unwrap();
    // 改内容再备份：两份内容若相同，就测不出「备份对象被弄混」
    std::fs::write(&b_path, b"v2").unwrap();
    let b2 = create(&base, &b_path, "replace").unwrap();

    let c = base.join("c.litematic");
    std::fs::rename(&b_path, &c).unwrap();
    relink(&root, &b_path, &c).unwrap();

    let items = list(&root);
    assert_eq!(items.len(), 2, "两次改名前后的备份都应保留");
    assert!(items.iter().all(|e| e.stem == "c"), "两份都应跟着新主干");
    assert!(items.iter().all(|e| matches_target(&e.path, &c)));
    // 两份内容必须与各自的来源对应：v1 是改名前的，v2 是改名后的
    let by_op = |op: &str| -> Vec<u8> {
        let e = items.iter().find(|e| e.op == op).expect("应存在该操作的备份");
        std::fs::read(&e.path).unwrap()
    };
    assert_eq!(by_op("metadata"), b"v1", "第一次备份应是改名前的旧内容");
    assert_eq!(by_op("replace"), b"v2", "第二次备份应是改名后的新内容");
    // 旧路径下的备份文件都应已移走
    assert!(!b1.backup_path.exists(), "旧路径的备份应已随分组移走");
    assert!(!b2.backup_path.exists(), "旧路径的备份应已随分组移走");

    // 只有一个分组目录，没有留下空壳
    let dirs: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    assert_eq!(dirs.len(), 1, "不应留下空分组目录：{dirs:?}");
    let _ = std::fs::remove_dir_all(&base);
}

/// 无法搬动的备份（目标处已被占用）必须让整次 relink 失败，
/// 并且**不能**留下「已搬走却没有 .orig_path」的孤儿备份。
///
/// 这条钉的是失败顺序：先建立新分组的关联再搬文件，
/// 中途失败时已搬过去的备份仍然可还原。
#[test]
fn relink_refuses_to_clobber_and_leaves_every_backup_restorable() {
    let base = temp_dir("relink_clobber");
    let root = backup_root(&base);
    let a = base.join("a.litematic");
    std::fs::write(&a, b"v1").unwrap();
    let ba = create(&base, &a, "metadata").unwrap();

    // 手工在目标分组里放一个同名文件，制造撞名
    let dest_dir = group_dir(&base, &base.join("b.litematic")).unwrap();
    std::fs::create_dir_all(&dest_dir).unwrap();
    let (stem, ns, op) = parse_backup_name(
        &ba.backup_path.file_name().unwrap().to_string_lossy(),
    )
    .unwrap();
    assert_eq!(stem, "a");
    std::fs::write(dest_dir.join(format!("b__{ns}__{op}.litematic")), b"other").unwrap();
    std::fs::write(dest_dir.join(ORIG_PATH_FILE), orig_path_of(&base.join("b.litematic")))
        .unwrap();

    let b = base.join("b.litematic");
    std::fs::write(&b, b"v2").unwrap();
    let e = relink(&root, &a, &b).unwrap_err().to_string();
    assert!(e.contains("已存在"), "{e}");

    // 撞名被拒绝，两侧的内容都不能被动过
    assert_eq!(
        std::fs::read(dest_dir.join(format!("b__{ns}__{op}.litematic"))).unwrap(),
        b"other",
        "目标处已有的备份不能被覆盖"
    );
    assert!(ba.backup_path.exists(), "撞名时不应把源备份搬走");
    // 源备份仍带着旧的 .orig_path，因此仍可还原回旧路径
    assert!(matches_target(&ba.backup_path, &a));
    let _ = std::fs::remove_dir_all(&base);
}

/// 与旧文件无关的分组（同目录下的另一个文件）不能被改到新名字上。
#[test]
fn relink_leaves_other_files_backups_untouched() {        let base = temp_dir("relink_other");
    let root = backup_root(&base);
    let a = base.join("a.litematic");
    let other = base.join("other.litematic");
    std::fs::write(&a, b"a").unwrap();
    std::fs::write(&other, b"o").unwrap();
    let ba = create(&base, &a, "metadata").unwrap();
    let bo = create(&base, &other, "metadata").unwrap();

    let a2 = base.join("a2.litematic");
    std::fs::rename(&a, &a2).unwrap();
    relink(&root, &a, &a2).unwrap();

    let items = list(&root);
    assert_eq!(items.len(), 2, "两个文件的备份都应还在");
    let moved = items.iter().find(|e| e.stem == "a2").expect("a 的备份应已跟随改名为 a2");
    assert!(matches_target(&moved.path, &a2), "a 的备份应能还原到 a2");
    let kept = items.iter().find(|e| e.stem == "other").expect("other 的备份主干不应被改动");
    assert!(kept.path.exists(), "别的文件的备份不能被搬走");
    assert!(matches_target(&kept.path, &other), "别的文件仍应能还原");
    assert!(!ba.backup_path.exists(), "a 的旧路径备份应已移走");
    assert_eq!(bo.backup_path, kept.path, "other 的备份路径不应变化");
    let _ = std::fs::remove_dir_all(&base);
}

/// 目标分组已存在时（同名文件曾存在、备份还在）应并入同一组，
/// 而不是报错或覆盖。
#[test]
fn relink_merges_into_existing_group_for_same_path() {
    let base = temp_dir("relink_merge");
    let root = backup_root(&base);
    let a = base.join("a.litematic");
    let b_path = base.join("b.litematic");
    std::fs::write(&a, b"a").unwrap();
    std::fs::write(&b_path, b"b").unwrap();
    let ba = create(&base, &a, "metadata").unwrap();
    let bb = create(&base, &b_path, "metadata").unwrap();

    // b 被删掉，但它的备份分组还在（备份不随源文件删除而删除）
    std::fs::remove_file(&b_path).unwrap();
    std::fs::rename(&a, &b_path).unwrap();

    let n = relink(&root, &a, &b_path).unwrap();
    assert_eq!(n, 1);

    let items = list(&root);
    assert_eq!(items.len(), 2, "两组备份应合并保留，一份都不能丢");
    assert!(items.iter().all(|e| matches_target(&e.path, &b_path)));

    let dirs: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    assert_eq!(dirs.len(), 1, "合并后只应剩一个分组：{dirs:?}");
    // a 的备份搬了过来，旧分组被清理；b 的备份本来就在目标分组里，原地不动
    assert!(!ba.backup_path.exists(), "a 的旧分组应已清理");
    assert!(bb.backup_path.exists(), "b 的备份本就在目标分组，不应被搬动");
    assert_eq!(items.len(), 2, "两份备份都要在");
    for e in &items {
        assert!(e.path.starts_with(&dirs[0].path()), "两份都应在同一个分组内");
        assert!(e.path.exists(), "列出来的备份必须真实存在");
    }
    let _ = std::fs::remove_dir_all(&base);
}
#[test]
fn is_inside_rejects_paths_outside_root() {
    let root = PathBuf::from("/data/backups");
    assert!(is_inside(&root, &root.join("demo").join("a.litematic")));
    assert!(is_inside(&root, &root));
    // 同级或完全无关的路径必须拒绝
    assert!(!is_inside(&root, &PathBuf::from("/data/other/a.litematic")));
    assert!(!is_inside(&root, &PathBuf::from("/etc/passwd")));
    // 用 .. 逃逸的路径不能绕过检查
    assert!(!is_inside(&root, &root.join("..").join("other").join("a.litematic")));
}
