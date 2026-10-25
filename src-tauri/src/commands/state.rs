//! 界面状态的读写。

use std::path::{Path, PathBuf};
use tauri::Manager;

// ---------------------------------------------------------------- 界面状态记忆

/// 界面状态文件名。内容是一段前端自己解释的 JSON。
const UI_STATE_FILE: &str = "ui-state.json";

/// 读取界面状态文件；文件不存在返回 `None`（首次启动的正常情况，不是错误）。
///
/// 与定位路径分开：路径依赖运行时的 AppHandle，而读写逻辑本身
/// （尤其是「不存在」与「读失败」的区分）是可以独立验证的。
pub(crate) fn read_ui_state_file(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取界面状态 {} 失败：{e}", path.display())),
    }
}

/// 原子写入界面状态文件。
///
/// 先写同目录的临时文件再改名：直接覆盖时若中途失败（磁盘满、进程被杀）
/// 会留下半截 JSON，下次启动解析失败就等于把用户所有界面设置清零。
/// 改成同目录内的改名在主流文件系统上是原子的，因此读到的要么是旧内容、
/// 要么是完整的新内容。
pub(crate) fn write_ui_state_file(path: &Path, json: &str) -> Result<(), String> {
    // 临时文件必须与目标同目录：跨目录改名不保证原子，甚至可能失败
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes())
        .map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| format!("替换 {} 失败：{e}", path.display()))
}

/// 应用配置目录。
///
/// 放在系统的应用配置目录（`%APPDATA%\<bundle-id>` 等）而不是程序目录：
/// 程序可能装在 Program Files 这类只读位置，而「记住界面」失败不该影响使用。
/// 用 Tauri 核心的 PathResolver 取，不需要额外插件。
pub(crate) fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("无法定位配置目录：{e}"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("无法创建配置目录 {}：{e}", dir.display()))?;
    Ok(dir)
}

/// 界面状态的存放路径。
pub(crate) fn ui_state_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join(UI_STATE_FILE))
}

/// 读取上次保存的界面状态；从未保存过时返回 None（不是错误）。
#[tauri::command]
pub fn load_ui_state(app: tauri::AppHandle) -> Result<Option<String>, String> {
    read_ui_state_file(&ui_state_path(&app)?)
}

/// 保存界面状态。
#[tauri::command]
pub fn save_ui_state(app: tauri::AppHandle, json: String) -> Result<(), String> {
    write_ui_state_file(&ui_state_path(&app)?, &json)
}

#[cfg(test)]
#[path = "../tests/commands/state.rs"]
mod tests;
