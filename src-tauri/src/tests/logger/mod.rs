use super::*;
use std::path::{Path, PathBuf};

/// 测试用临时目录。放在工程内的 target 下，与其余模块的测试保持一致。
fn temp_dir(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join(format!("sm_logger_{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn touch(dir: &Path, name: &str) {
    std::fs::write(dir.join(name), b"archive").unwrap();
}

fn archive_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".log.gz"))
        .collect();
    names.sort();
    names
}

#[test]
fn the_largest_archive_index_comes_from_the_index_segment() {
    let dir = temp_dir("max-index");
    for name in [
        "2026-10-18-2-latest.log.gz",
        "2026-10-19-3-latest.log.gz",
        "2026-10-19-11-latest.log.gz",
        "2026-10-19-7-debug.log.gz",
        "2026-10-19-4-debug.log.gz",
        // 运行日志的归档，不是本模块的
        "2026-10-19-1.log.gz",
    ] {
        touch(&dir, name);
    }

    assert_eq!(get_max_archive_index("latest", &dir), 11);
    assert_eq!(get_max_archive_index("debug", &dir), 7);
    assert_eq!(get_max_archive_index("trace", &dir), 0);
}

#[test]
fn the_next_archive_name_continues_after_the_largest_index() {
    let dir = temp_dir("next-name");
    touch(&dir, "2026-10-19-11-latest.log.gz");

    let next = get_next_archive_name("latest", &dir);
    let name = next.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.ends_with("-12-latest.log.gz"),
        "序号要接着最大值往下走：{name}"
    );
}

#[test]
fn cleaning_keeps_the_newest_archives_by_date_and_index() {
    let dir = temp_dir("clean-order");
    for index in [1, 2, 3, 10, 11, 12] {
        touch(&dir, &format!("2026-10-19-{index}-latest.log.gz"));
    }
    touch(&dir, "2026-10-18-9-latest.log.gz");
    touch(&dir, "2026-10-19-99-debug.log.gz");

    LogFileObject::clean_old_archives(&dir, "latest", 3);

    assert_eq!(
        archive_names(&dir),
        [
            "2026-10-19-10-latest.log.gz",
            "2026-10-19-11-latest.log.gz",
            "2026-10-19-12-latest.log.gz",
            // 清 latest 不能碰 debug 的归档
            "2026-10-19-99-debug.log.gz",
        ]
    );
}

#[test]
fn cleaning_does_nothing_when_the_archives_fit() {
    let dir = temp_dir("clean-under-limit");
    touch(&dir, "2026-10-19-1-latest.log.gz");
    touch(&dir, "2026-10-19-2-latest.log.gz");

    LogFileObject::clean_old_archives(&dir, "latest", 5);

    assert_eq!(
        archive_names(&dir),
        [
            "2026-10-19-1-latest.log.gz",
            "2026-10-19-2-latest.log.gz",
        ]
    );
}

#[test]
fn cleaning_leaves_files_that_only_look_like_archives() {
    let dir = temp_dir("clean-lookalike");
    // 都带 -latest.log.gz 结尾，但取不出合法的日期与序号
    touch(&dir, "backup-latest.log.gz");
    touch(&dir, "2026-13-40-5-latest.log.gz");

    LogFileObject::clean_old_archives(&dir, "latest", 0);

    assert_eq!(archive_names(&dir).len(), 2, "认不出日期与序号的文件不能删");
}
