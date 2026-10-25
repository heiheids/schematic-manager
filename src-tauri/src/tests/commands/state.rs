use super::*;
use crate::test_support::temp_state_dir;

#[test]
fn ui_state_round_trips_through_the_file() {
    let dir = temp_state_dir("rt");
    let path = dir.join(UI_STATE_FILE);
    let json = r#"{"version":1,"sidebarWidth":820,"dirs":["C:/D1"]}"#;

    write_ui_state_file(&path, json).expect("写入失败");
    let got = read_ui_state_file(&path).expect("读取失败");
    assert_eq!(got.as_deref(), Some(json), "写进去的应能原样读回来");

    // 覆盖写：第二次必须替换掉第一次，而不是追加或残留
    let json2 = r#"{"version":1,"sidebarWidth":666,"dirs":[]}"#;
    write_ui_state_file(&path, json2).expect("覆盖写入失败");
    assert_eq!(
        read_ui_state_file(&path).expect("读取失败").as_deref(),
        Some(json2),
        "第二次保存应替换旧内容"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ui_state_missing_file_is_not_an_error() {
    let dir = temp_state_dir("missing");
    let path = dir.join(UI_STATE_FILE);
    // 首次启动：文件还不存在。这是正常情况，必须返回 None 而不是错误，
    // 否则每次首启都会弹出一个「读取界面状态失败」的告警。
    let got = read_ui_state_file(&path).expect("文件不存在不应视为错误");
    assert!(got.is_none(), "未保存过时应返回 None");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ui_state_write_leaves_no_temp_file_behind() {
    let dir = temp_state_dir("tmp");
    let path = dir.join(UI_STATE_FILE);
    write_ui_state_file(&path, "{}").expect("写入失败");
    // 临时文件必须已被改名带走：残留会在下次写入时产生
    // 「目标已存在」之类的意外，也会让用户目录里留下垃圾
    assert!(!path.with_extension("json.tmp").exists(), "不应残留临时文件");
    let names: Vec<String> = std::fs::read_dir(&dir)
        .expect("列目录失败")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![UI_STATE_FILE.to_string()], "目录里应只剩状态文件本身");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ui_state_failed_write_keeps_previous_content() {
    let dir = temp_state_dir("fail");
    let path = dir.join(UI_STATE_FILE);
    let good = r#"{"version":1,"sidebarWidth":700}"#;
    write_ui_state_file(&path, good).expect("首次写入失败");

    // 用一个必然失败的目标模拟「写盘中途失败」：
    // 把临时文件的位置占成一个目录，写入就会失败。
    // 关键是**原有内容必须完好**——否则用户所有界面设置会被清零。
    std::fs::create_dir_all(path.with_extension("json.tmp")).expect("占位失败");
    let r = write_ui_state_file(&path, r#"{"version":1,"sidebarWidth":1}"#);
    assert!(r.is_err(), "临时文件写不进去时应报错，而不是静默成功");
    assert_eq!(
        read_ui_state_file(&path).expect("读取失败").as_deref(),
        Some(good),
        "写入失败后原有内容必须保持不变"
    );

    std::fs::remove_dir_all(&dir).ok();
}
