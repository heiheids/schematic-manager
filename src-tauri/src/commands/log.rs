//! 运行日志的读取、写入与等级设置。

use super::state::{read_ui_state_file, ui_state_path};

/// 读取运行日志，由新到旧。
#[tauri::command]
pub fn read_run_log(max_lines: Option<usize>) -> Vec<String> {
    crate::app::run_log::read(max_lines.unwrap_or(200).min(5000))
}

/// 清空运行日志。
///
/// 只清空内容、不删文件（理由见 `RunLog::clear`）。这是用户显式要求的
/// 动作，失败必须报出来——不像写日志那样可以悄悄放过。
#[tauri::command]
pub fn clear_run_log() -> Result<(), String> {
    crate::app::run_log::clear().map_err(|e| e.to_string())
}

/// 在文件管理器里打开运行日志所在的目录。
///
/// 路径在后端算：日志位置是后端的约定（`run_log` 决定），前端再拼一遍
/// 就成了两处规则，改一处必然对不上。
#[tauri::command]
pub fn open_run_log_dir() -> Result<(), String> {
    let Some(log) = crate::app::run_log::path() else {
        return Err("运行日志尚未启用，无法定位所在目录".to_string());
    };
    // 日志存在就定位到它：资源管理器会打开所在目录并选中该文件。
    // 从没写过日志时它还不存在，那就先建出日志目录再打开目录本身——
    // 用户此刻想知道的正是「日志放在哪」。
    if log.exists() {
        return crate::app::file_ops::reveal_in_file_manager(&log).map_err(|e| e.to_string());
    }
    let dir = log.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("创建目录 {} 失败：{e}", dir.display()))?;
    crate::app::file_ops::open_directory(&dir).map_err(|e| e.to_string())
}

/// 界面把自身遇到的警告与错误交给后端记录。
///
/// 界面侧的问题（界面状态读写失败、未捕获异常等）只有界面知道，不交给后端就
/// 永远不会出现在运行日志里，用户报「界面不对劲」时也就无据可查。
/// 等级非法时直接报错：把界面拼错的等级静默当成默认等级，等于让日志悄悄丢内容。
#[tauri::command]
pub fn log_ui_event(level: String, message: String) -> Result<(), String> {
    let parsed = crate::app::run_log::Level::parse(&level)
        .ok_or_else(|| format!("无法识别的日志等级：{level}"))?;
    // 前缀标明来源：日志里界面与后端的行混在一起，不标就分不清是谁写的
    crate::app::run_log::write(parsed, &format!("界面 {message}"));
    Ok(())
}

/// 运行日志所在目录；日志没启用时返回 None。
///
/// 目录由后端算（`run_log` 决定），界面拿来显示给用户看。让界面自己拼一遍
/// 应用数据目录就成了两处规则，identifier 一变就会指向不存在的位置——
/// 这个项目已经改过一次 identifier（见 `data_dir`）。
#[tauri::command]
pub fn run_log_dir() -> Option<String> {
    crate::app::run_log::path()
        .and_then(|p| p.parent().map(|d| d.display().to_string()))
}

/// 界面把日志等级与本地日期交给后端。
/// 等级由界面持久化（界面状态文件），这里只负责落地：等级决定写入哪些行，
/// 本地日期只用于归档文件名——Rust 侧不存本地时间，避免为文件名引入时区依赖。
/// 界面每次改等级、每次打开日志窗口都会调用，因此日期跟着刷新。
#[tauri::command]
pub fn configure_run_log(level: String, local_date: String) -> Result<(), String> {
    let parsed = crate::app::run_log::Level::parse(&level)
        .ok_or_else(|| format!("无法识别的日志等级：{level}"))?;
    // 日期会被拼进归档文件名，因此按约定形状校验：只认 YYYY-MM-DD。
    // 界面那边固定这么产出，这里是防「拼进去一个奇怪的路径片段」这类意外。
    if !is_local_date(&local_date) {
        return Err(format!("本地日期格式应为 YYYY-MM-DD：{local_date}"));
    }
    let changed =
        crate::app::run_log::configure(parsed, &local_date).map_err(|e| e.to_string())?;
    // 只在真的变了时记一行：界面每开一次日志窗口都会同步一次同样的设置，
    // 每次都记就成了噪音
    if changed {
        crate::app::run_log::info(&format!(
            "日志等级设为 {}，归档日期 {}",
            parsed.token(),
            local_date
        ));
    }
    Ok(())
}

/// 是不是 `YYYY-MM-DD` 形状的本地日期。
fn is_local_date(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
}

/// 启动时按界面存下的等级取值；没存过或认不出时用 info（默认等级）。
///
/// 之所以从界面状态文件里读而不是等界面推过来：debug 用户要的是**从启动第一行**
/// 起的完整日志，如果等事件循环里界面来设置，启动阶段早就记完了。
///
/// 读的是前端 `uiState.ts` 写下的 `logLevel` 键：界面是这份状态的唯一作者，
/// 因此键改名要两边一起改（见 src/store/uiState.ts 的 UiState）。
pub(crate) fn saved_log_level(app: &tauri::AppHandle) -> crate::app::run_log::Level {
    let path = match ui_state_path(app) {
        Ok(p) => p,
        Err(e) => {
            crate::app::run_log::warn(&format!("读取界面状态路径失败，日志等级按默认：{e}"));
            return crate::app::run_log::Level::Info;
        }
    };
    match read_ui_state_file(&path) {
        Ok(Some(json)) => log_level_from_ui_state(&json),
        // 从未保存过界面状态：正常情况，用默认等级
        Ok(None) => crate::app::run_log::Level::Info,
        Err(e) => {
            crate::app::run_log::warn(&format!("读取界面状态失败，日志等级按默认：{e}"));
            crate::app::run_log::Level::Info
        }
    }
}

/// 从界面状态 JSON 里取日志等级；取不到或认不出时用默认的 info。
fn log_level_from_ui_state(json: &str) -> crate::app::run_log::Level {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| {
            v.get("logLevel")
                .and_then(|l| l.as_str())
                .and_then(crate::app::run_log::Level::parse)
        })
        .unwrap_or(crate::app::run_log::Level::Info)
}

#[cfg(test)]
#[path = "../tests/commands/log.rs"]
mod tests;
