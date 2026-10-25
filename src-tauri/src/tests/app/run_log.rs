use super::*;
use std::path::PathBuf;

/// 测试用临时目录。放在工程内的 target 下，与其余模块的测试保持一致。
fn temp_dir(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join(format!("sm_run_log_{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 解压一份归档，便于断言归档内容
fn unzip(path: &Path) -> String {
    use std::io::Read;
    let f = std::fs::File::open(path).unwrap();
    let mut d = flate2::read::GzDecoder::new(f);
    let mut out = String::new();
    d.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn only_lines_at_or_above_the_configured_level_are_written() {
    let dir = temp_dir("level");
    let mut log = RunLog::open(&dir).unwrap();
    log.configure(Level::Warn, "2026-10-19").unwrap();

    log.write(Level::Debug, "调试细节");
    log.write(Level::Info, "操作记录");
    log.write(Level::Warn, "取不到语言表");
    log.write(Level::Error, "保存失败");

    let text = std::fs::read_to_string(log.path()).unwrap();
    assert!(!text.contains("调试细节"), "debug 行不该出现在 warn 等级");
    assert!(!text.contains("操作记录"), "info 行不该出现在 warn 等级");
    assert!(text.contains("取不到语言表"), "警告要记下来");
    assert!(text.contains("保存失败"), "错误要记下来");
}

#[test]
fn a_message_with_line_breaks_stays_one_entry() {
    let dir = temp_dir("escape");
    let mut log = RunLog::open(&dir).unwrap();
    log.configure(Level::Debug, "2026-10-19").unwrap();
    log.write(Level::Debug, "响应体：{\n  \"a\": 1\n}\t结束");

    let lines = log.read(10);
    assert_eq!(lines.len(), 1, "带换行的说明必须仍是一行，否则会被读成多条日志");
    assert!(
        lines[0].contains("\\n") && lines[0].contains("\\t"),
        "换行与制表符要转义成可见形式：{}",
        lines[0]
    );
}

#[test]
fn the_level_token_is_the_second_field() {
    let dir = temp_dir("token");
    let mut log = RunLog::open(&dir).unwrap();
    log.configure(Level::Debug, "2026-10-19").unwrap();
    log.write(Level::Info, "替换 a.litematic：石头 -> 泥土");

    let line = log.read(1).remove(0);
    let fields: Vec<&str> = line.split('\t').collect();
    assert_eq!(fields.len(), 3, "行应当是「时间戳 / 等级 / 说明」三段：{line}");
    assert!(fields[0].chars().all(|c| c.is_ascii_digit()), "首列是纳秒时间戳");
    assert_eq!(fields[1], "INFO");
    assert_eq!(fields[2], "替换 a.litematic：石头 -> 泥土");
}

#[test]
fn reading_returns_newest_first_and_respects_the_cap() {
    let dir = temp_dir("read");
    let mut log = RunLog::open(&dir).unwrap();
    log.configure(Level::Info, "2026-10-19").unwrap();
    log.write(Level::Info, "第一条");
    log.write(Level::Info, "第二条");

    let lines = log.read(10);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("第二条"), "最新的在最前");
    assert_eq!(log.read(1).len(), 1, "上限要生效");
}

#[test]
fn clearing_keeps_the_file_but_drops_the_content() {
    let dir = temp_dir("clear");
    let mut log = RunLog::open(&dir).unwrap();
    log.configure(Level::Info, "2026-10-19").unwrap();
    log.write(Level::Info, "要清掉的一行");
    log.clear().unwrap();

    assert!(log.path().exists(), "清空不该删掉文件");
    assert_eq!(std::fs::metadata(log.path()).unwrap().len(), 0);
    assert!(log.read(10).is_empty());
}

#[test]
fn the_startup_roll_waits_for_the_local_date_and_keeps_this_session_lines() {
    let dir = temp_dir("startup");
    // 上一份日志：程序上次运行时留下的
    std::fs::write(dir.join(LOG_NAME), "旧的一行\n").unwrap();

    let mut log = RunLog::open(&dir).unwrap();
    // 界面还没给出日期，本次启动的行先进缓冲
    log.write(Level::Info, "本次启动");

    // 界面挂载后给出等级与本地日期
    log.configure(Level::Info, "2026-10-19").unwrap();

    let archive = dir.join("2026-10-19-1.log.gz");
    assert!(archive.exists(), "启动时要滚动出归档");
    assert!(unzip(&archive).contains("旧的一行"), "归档要装上一份日志");

    let now = std::fs::read_to_string(log.path()).unwrap();
    assert!(now.contains("本次启动"), "本次会话的行要留在当前日志里");
    assert!(!now.contains("旧的一行"), "上一份内容不该留在当前日志里");
    assert!(now.contains("日志已滚动"), "滚动这件事本身也要如实记一笔");
}

#[test]
fn an_empty_previous_log_does_not_produce_an_empty_archive() {
    let dir = temp_dir("empty-prev");
    std::fs::write(dir.join(LOG_NAME), b"").unwrap();

    let mut log = RunLog::open(&dir).unwrap();
    log.configure(Level::Info, "2026-10-19").unwrap();

    assert!(
        !dir.join("2026-10-19-1.log.gz").exists(),
        "没有内容可归档时不该产生空归档"
    );
}

#[test]
fn rolling_never_overwrites_an_existing_archive() {
    let dir = temp_dir("index");
    std::fs::write(dir.join(LOG_NAME), "第一份\n").unwrap();
    let mut log = RunLog::open(&dir).unwrap();
    log.configure(Level::Info, "2026-10-19").unwrap();
    assert!(dir.join("2026-10-19-1.log.gz").exists());

    // 再打开一次程序：又要滚动一次，次数必须接着往下走
    std::fs::write(dir.join(LOG_NAME), "第二份\n").unwrap();
    let mut second_session = RunLog::open(&dir).unwrap();
    second_session.configure(Level::Info, "2026-10-19").unwrap();

    let second = dir.join("2026-10-19-2.log.gz");
    assert!(second.exists(), "第二次滚动应生成 -2");
    assert!(
        unzip(&dir.join("2026-10-19-1.log.gz")).contains("第一份"),
        "先前的归档不能被覆盖"
    );
    assert!(unzip(&second).contains("第二份"));
}

#[test]
fn exceeding_the_size_limit_rolls_immediately() {
    let dir = temp_dir("size");
    let mut log = RunLog::open(&dir).unwrap();
    log.configure(Level::Info, "2026-10-19").unwrap();

    // 一次写入就顶过上限，滚动应当在这次写入之后就发生
    log.write(Level::Info, &"x".repeat(MAX_BYTES as usize + 1));

    let archive = dir.join("2026-10-19-1.log.gz");
    assert!(archive.exists(), "超过 5MB 要滚动");
    assert!(unzip(&archive).contains("xxxx"), "归档里要装上超大那一行");
    let now = log.read(10);
    assert!(
        now.iter().any(|l| l.contains("日志超过")),
        "滚动这件事要在新文件里记一笔：{now:?}"
    );
    assert!(
        std::fs::metadata(log.path()).unwrap().len() < MAX_BYTES,
        "滚动后当前文件应当远小于上限"
    );
}

#[test]
fn without_a_local_date_the_startup_roll_is_skipped_instead_of_inventing_one() {
    let dir = temp_dir("no-date");
    std::fs::write(dir.join(LOG_NAME), "上一份\n").unwrap();
    let mut log = RunLog::open(&dir).unwrap();

    // 一直不给日期，且把缓冲灌满——界面起不来时的情形
    for i in 0..(MAX_PENDING_LINES + 5) {
        log.write(Level::Info, &format!("第 {i} 行"));
    }

    assert!(
        std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().ends_with(".log.gz")),
        "没有本地日期时不该编造归档名"
    );
    let text = std::fs::read_to_string(log.path()).unwrap();
    assert!(text.contains("上一份"), "现有内容要保留");
    assert!(text.contains("第 0 行"), "缓冲里的行不能丢");
}

#[test]
fn repeating_the_same_setting_reports_no_change() {
    let dir = temp_dir("configure-change");
    let mut log = RunLog::open(&dir).unwrap();

    assert!(
        log.configure(Level::Debug, "2026-10-19").unwrap(),
        "第一次同步当然算变化"
    );
    assert!(
        !log.configure(Level::Debug, "2026-10-19").unwrap(),
        "同样的等级与日期重复同步不算变化，否则每开一次日志窗口就多一条噪音"
    );
    assert!(
        log.configure(Level::Info, "2026-10-19").unwrap(),
        "等级变了要算变化"
    );
    assert!(
        log.configure(Level::Info, "2026-10-20").unwrap(),
        "跨天之后日期变了也要算变化"
    );
}

#[test]
fn a_failed_archive_write_keeps_the_current_log() {
    let dir = temp_dir("roll-fail");
    std::fs::write(dir.join(LOG_NAME), "必须活下来的内容\n").unwrap();
    let mut log = RunLog::open(&dir).unwrap();

    // 让归档名指向一个不存在的子目录，归档必然写不进去。
    // 真实触发方式（磁盘满、目录权限）在测试里难以稳定复现，
    // 但这里要验的是同一件事：归档失败时当前日志内容不能丢。
    let err = log.configure(Level::Info, "2026/10/19");
    assert!(err.is_err(), "归档写不进去时要报错，而不是假装滚过了");

    assert_eq!(
        std::fs::read_to_string(log.path()).unwrap(),
        "必须活下来的内容\n",
        "归档失败时当前日志必须原样保留"
    );
    assert!(
        std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().ends_with(".log.gz")),
        "失败时不该留下半截归档"
    );
}

#[test]
fn a_long_response_body_is_clipped_with_a_visible_note() {
    let long = "a".repeat(MAX_BODY_BYTES * 2);
    let clipped = clip_body(&long);
    assert!(clipped.len() < long.len(), "超长正文要被截断");
    assert!(clipped.contains("已截断"), "截断要如实说明，不能让读日志的人以为响应就这么短");

    let short = "{\"ok\":true}";
    assert_eq!(clip_body(short), short, "没超限的正文原样保留");
}
