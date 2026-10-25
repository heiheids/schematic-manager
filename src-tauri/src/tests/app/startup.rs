use super::*;

#[test]
fn log_is_written_somewhere() {
    // 两处候选位置至少要有一处可写，否则错误只能靠弹窗传达
    let p = write_log("测试内容").expect("应能写入日志");
    assert!(p.exists());
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "测试内容");
    let _ = std::fs::remove_file(&p);
}

#[test]
fn body_always_carries_the_real_error() {
    let b = build_body(
        "磁盘已满",
        "C:\\app\\a.exe",
        Some("C:\\app\\启动失败.log"),
        "com.example.app",
    );
    assert!(b.contains("磁盘已满"), "必须保留原始错误，否则无从排查");
    assert!(b.contains("C:\\app\\a.exe"), "应给出程序位置");
    assert!(b.contains("C:\\app\\启动失败.log"), "应给日志位置");
}

#[test]
fn webview_error_gets_actionable_hint() {
    // 正文里的目录必须取自传入的标识符，不能是另外写死的字符串：
    // 标识符改了而这里没跟着改，用户就会照着错的路径去删目录
    let b = build_body(
        "failed to create webview: WebView2 error",
        "a.exe",
        None,
        "com.example.app",
    );
    assert!(
        b.contains("com.example.app"),
        "WebView2 失败时应给出可操作的数据目录路径"
    );
    assert!(b.contains("WebView2 运行时"), "应提示安装运行库");
}

#[test]
fn unrelated_error_has_no_webview_hint() {
    let b = build_body("磁盘写入失败：拒绝访问", "a.exe", None, "com.example.app");
    assert!(
        !b.contains("WebView2"),
        "与界面无关的错误不应误导用户去删数据目录"
    );
    assert!(b.contains("磁盘写入失败"), "应保留原始错误信息");
}

#[test]
fn missing_log_path_is_stated_plainly() {
    let b = build_body("x", "a.exe", None, "com.example.app");
    assert!(
        b.contains("无法写入日志文件"),
        "写不了日志时要如实说明，不能让用户以为有日志可查"
    );
}

/// 环境变量是进程级的，测试之间会相互影响，因此串行用同一把锁。
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn dialog_is_allowed_by_default_and_suppressed_only_explicitly() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    std::env::remove_var(NO_DIALOG_ENV);
    assert!(dialog_allowed(), "默认应允许弹窗，否则用户看不到错误");

    // 只有明确的真值才抑制：空串、"0" 都不应改变用户可见行为
    for v in ["", "0", "no", "false"] {
        std::env::set_var(NO_DIALOG_ENV, v);
        assert!(dialog_allowed(), "取值 {v:?} 不应抑制弹窗");
    }
    for v in ["1", "true", "TRUE", " yes "] {
        std::env::set_var(NO_DIALOG_ENV, v);
        assert!(!dialog_allowed(), "取值 {v:?} 应抑制弹窗");
    }
    std::env::remove_var(NO_DIALOG_ENV);
}
