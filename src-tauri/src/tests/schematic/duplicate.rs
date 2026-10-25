use super::*;

fn write(dir: &Path, name: &str, data: &[u8]) -> Candidate {
    let p = dir.join(name);
    std::fs::write(&p, data).unwrap();
    Candidate { path: p, size: data.len() as u64 }
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("testtmp")
        .join(format!("sm_dup_{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn finds_identical_files_under_different_names() {
    let d = temp_dir("basic");
    let a = write(&d, "a.litematic", b"same content here");
    let b = write(&d, "b.litematic", b"same content here");
    let c = write(&d, "c.litematic", b"different content!");
    let r = find(&[a, b, c]);
    assert_eq!(r.groups.len(), 1);
    assert_eq!(r.groups[0].paths.len(), 2);
    assert!(r.groups[0].paths.contains(&d.join("a.litematic")));
    assert!(r.groups[0].paths.contains(&d.join("b.litematic")));
    assert!(r.unreadable.is_empty(), "不应有读取失败");
}

#[test]
fn same_name_different_content_is_not_duplicate() {
    let d = temp_dir("notdup");
    // 大小相同但内容不同，必须靠逐字节比对排除
    let a = write(&d, "a.litematic", b"AAAA");
    let b = write(&d, "b.litematic", b"BBBB");
    assert!(find(&[a, b]).groups.is_empty(), "内容不同不应判为重复");
}

#[test]
fn no_duplicates_yields_empty() {
    let d = temp_dir("none");
    let a = write(&d, "a.litematic", b"one");
    let b = write(&d, "b.litematic", b"two two");
    assert!(find(&[a, b]).groups.is_empty());
    assert!(find(&[]).groups.is_empty(), "空输入不应 panic");
}

#[test]
fn groups_three_or_more() {
    let d = temp_dir("three");
    let a = write(&d, "a.litematic", b"xyz");
    let b = write(&d, "b.litematic", b"xyz");
    let c = write(&d, "c.litematic", b"xyz");
    let r = find(&[a, b, c]);
    assert_eq!(r.groups.len(), 1);
    assert_eq!(r.groups[0].paths.len(), 3);
    assert_eq!(r.groups[0].size, 3);
}

#[test]
fn multiple_independent_groups() {
    let d = temp_dir("multi");
    let a1 = write(&d, "a1.litematic", b"first");
    let a2 = write(&d, "a2.litematic", b"first");
    let b1 = write(&d, "b1.litematic", b"second!!");
    let b2 = write(&d, "b2.litematic", b"second!!");
    let r = find(&[a1, a2, b1, b2]);
    assert_eq!(r.groups.len(), 2, "应识别出两组独立的重复");
    assert!(r.groups.iter().all(|g| g.paths.len() == 2));
}

#[test]
fn unreadable_file_is_reported_not_silently_dropped() {
    let d = temp_dir("missing");
    let a = write(&d, "a.litematic", b"data");
    let ghost_path = d.join("不存在的文件.litematic");
    let ghost = Candidate { path: ghost_path.clone(), size: 4 };
    let r = find(&[a, ghost]);

    // 读不了的文件既不能成组，也不能被默默当成「与谁都不重复」
    assert!(r.groups.is_empty());
    assert_eq!(r.unreadable.len(), 1, "必须上报读取失败");
    assert_eq!(r.unreadable[0].0, ghost_path, "应报告具体是哪个文件");
    assert!(!r.unreadable[0].1.is_empty(), "应带上失败原因");
}

#[test]
fn unreadable_member_does_not_hide_real_group() {
    let d = temp_dir("mixed");
    let a = write(&d, "a.litematic", b"same");
    let b = write(&d, "b.litematic", b"same");
    // 与 a、b 大小相同但读不出来的文件混在中间
    let ghost = Candidate { path: d.join("ghost.litematic"), size: 4 };
    let r = find(&[a, b, ghost]);
    assert_eq!(r.groups.len(), 1, "读取失败不应影响真实的重复判定");
    assert_eq!(r.groups[0].paths.len(), 2);
    assert_eq!(r.unreadable.len(), 1);
}

#[test]
fn empty_files_of_equal_size_are_duplicates() {
    let d = temp_dir("empty");
    let a = write(&d, "a.litematic", b"");
    let b = write(&d, "b.litematic", b"");
    let r = find(&[a, b]);
    assert_eq!(r.groups.len(), 1, "两个空文件内容相同，应判为重复");
    assert_eq!(r.groups[0].size, 0);
}

#[test]
fn result_order_is_stable() {
    let d = temp_dir("order");
    let paths: Vec<Candidate> = ["z", "a", "m"]
        .iter()
        .map(|n| write(&d, &format!("{n}.litematic"), b"same"))
        .collect();
    let g1 = find(&paths);
    let mut rev = paths.clone();
    rev.reverse();
    let g2 = find(&rev);
    assert_eq!(g1, g2, "输入顺序不应影响结果");
    let names: Vec<String> = g1.groups[0]
        .paths
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["a.litematic", "m.litematic", "z.litematic"]);
}
