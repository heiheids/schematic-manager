use super::*;
use std::path::PathBuf;

/// 测试用临时目录。放在工程内的 target 下，与其余模块的测试保持一致，
/// 避免依赖系统临时目录的可写性。
fn temp_dir(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join(format!("sm_data_dir_{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 旧、新两个目录名，以及它们共同的上级目录。
fn layout(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = temp_dir(tag);
    let old = base.join(LEGACY_IDENTIFIER);
    let new = base.join("io.github.heiheids.schematic-manager");
    (base, old, new)
}

#[test]
fn take_over_moves_contents_into_a_target_that_already_exists() {
    // Linux 上目标目录会被 webview 先建出来（空壳），搬迁仍须进行
    let (_base, old, new) = layout("existing");
    let blocks = old.join("base").join("blocks");
    std::fs::create_dir_all(&blocks).unwrap();
    std::fs::write(blocks.join("1.20.dat.z"), b"lib").unwrap();
    std::fs::create_dir_all(&new).unwrap();

    take_over(&old, &new).unwrap();

    assert_eq!(
        std::fs::read(new.join("base").join("blocks").join("1.20.dat.z")).unwrap(),
        b"lib",
        "目标目录已存在（哪怕是空的）时也要把内容搬过去"
    );
    assert!(!old.exists(), "搬空之后旧目录应被删掉");
}

#[test]
fn take_over_resumes_a_partial_move() {
    let (_base, old, new) = layout("resume");
    // 上一次迁移搬走了一项就退出了：那一项已经不在旧目录里了
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("state.json"), b"ui").unwrap();
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(new.join("blocks.dat.z"), b"lib").unwrap();

    take_over(&old, &new).unwrap();

    assert_eq!(std::fs::read(new.join("state.json")).unwrap(), b"ui");
    assert_eq!(
        std::fs::read(new.join("blocks.dat.z")).unwrap(),
        b"lib",
        "已经搬过去的那一份不该被覆盖"
    );
    assert!(!old.exists(), "剩下的都搬走后旧目录应被删掉");
}

#[test]
fn take_over_keeps_both_copies_when_a_name_exists_on_both_sides() {
    let (_base, old, new) = layout("conflict");
    std::fs::create_dir_all(old.join("base")).unwrap();
    std::fs::write(old.join("base").join("old.txt"), b"old").unwrap();
    std::fs::create_dir_all(new.join("base")).unwrap();
    std::fs::write(new.join("base").join("new.txt"), b"new").unwrap();

    take_over(&old, &new).unwrap();

    assert_eq!(std::fs::read(new.join("base").join("new.txt")).unwrap(), b"new");
    assert_eq!(
        std::fs::read(old.join("base").join("old.txt")).unwrap(),
        b"old",
        "同名条目不能覆盖目标里已有的那份，也不能把它删掉"
    );
}

#[test]
fn take_over_without_the_old_dir_changes_nothing() {
    let (_base, old, new) = layout("none");

    take_over(&old, &new).unwrap();

    assert!(!old.exists());
    assert!(!new.exists(), "没有旧目录时不该凭空建出新目录");
}
