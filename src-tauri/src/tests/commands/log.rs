use super::*;

/// 界面状态里记下的日志等级要被启动流程采纳，认不出或缺键时回落 info。
///
/// 这条守住的是「debug 用户从启动第一行起就有完整日志」：等级存在界面状态里，
/// 键名或取值一变，这段就只会静默回落，用户再也拿不到启动阶段的调试日志。
#[test]
fn saved_log_level_comes_from_ui_state_and_falls_back_to_info() {
    assert_eq!(
        log_level_from_ui_state(r#"{"version":1,"logLevel":"debug"}"#),
        crate::app::run_log::Level::Debug
    );
    assert_eq!(
        log_level_from_ui_state(r#"{"version":1,"sidebarWidth":419}"#),
        crate::app::run_log::Level::Info,
        "没记过等级时用默认等级"
    );
    assert_eq!(
        log_level_from_ui_state(r#"{"version":1,"logLevel":"verbose"}"#),
        crate::app::run_log::Level::Info,
        "认不出的取值不该当成某个等级"
    );
    assert_eq!(
        log_level_from_ui_state("这不是 JSON"),
        crate::app::run_log::Level::Info,
        "界面状态坏了也不该影响启动"
    );
}

/// 仅接受 `YYYY-MM-DD`：这个值会被拼进归档文件名。
#[test]
fn local_date_must_be_a_plain_calendar_date() {
    assert!(is_local_date("2026-10-19"));
    assert!(!is_local_date("2026-1-9"), "位数不对就不是约定形状");
    assert!(!is_local_date("2026/10/19"), "分隔符必须是短横线");
    assert!(!is_local_date("../2026-10-19"), "路径片段不能被当成日期");
    assert!(!is_local_date(""), "空串不该通过");
}

/// 界面送来的等级/日期不合约定时要如实报错，不能悄悄按默认处理。
///
/// 静默按默认的后果是：用户在界面上选了「调试」，日志却仍按 info 记，
/// 而他以为已经开了——这类「以为开了其实没开」最难排查。
#[test]
fn log_commands_reject_values_that_do_not_match_the_contract() {
    let bad_level = configure_run_log("verbose".to_string(), "2026-10-19".to_string());
    assert!(bad_level.is_err(), "认不出的等级要报错");
    assert!(
        bad_level.unwrap_err().contains("verbose"),
        "报错要说清是哪个取值不认"
    );
    assert!(
        configure_run_log("info".to_string(), "2026/10/19".to_string()).is_err(),
        "日期形状不对要报错"
    );
    assert!(
        log_ui_event("loud".to_string(), "随便一条".to_string()).is_err(),
        "界面报的事件等级同样要校验"
    );
}
