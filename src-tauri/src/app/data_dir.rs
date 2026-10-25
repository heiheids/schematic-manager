//! 应用数据目录的搬迁。
//!
//! bundle identifier 决定应用数据目录（`app_data_dir`、`app_config_dir`）的
//! 位置。identifier 从 `com.heiheids.schematic-manager` 换成
//! `io.github.heiheids.schematic-manager` 之后，已装用户原有的数据全留在旧
//! 目录里：方块库体积可达数百 MB，要靠联网下载或扫本机客户端才能补齐，界面
//! 状态也会重置——对用户来说就是「更新完东西全没了」。所以要在启动时把旧
//! 目录里的内容搬进新位置。
//!
//! 搬迁必须容忍目标目录已经存在：Tauri 在调用用户的 `.setup()` **之前**就按
//! 配置把窗口建好了（`app.rs` 的 `fn setup` 先创建窗口，之后才调用 setup
//! 闭包），而窗口在 Linux/Windows 上会强制把 `app_local_data_dir()` 当作
//! webview 数据目录并 `create_dir_all` 出来（`manager/webview.rs`）；Linux 上
//! `dirs::data_local_dir()` 与 `dirs::data_dir()` 是同一个目录，那正好就是
//! 应用数据目录。因此这里逐项搬入，而不给整个目录改名。
//!
//! 移除条件：旧 identifier 不再有任何在用的发布时，删掉本模块以及 lib.rs 里
//! 的调用（当前最后一个使用旧 identifier 的发布是 v0.1.1-dev.15）。
//!
//! 两个实例同时首次启动时，后启动的那个可能在搬迁中途失败（对手抢先搬走了同
//! 一项），于是它启动失败。数据不会受损，重开即可——项目没有单实例插件，这种
//! 并发是可能的，但代价只是重试一次，不值得为它引入锁。

use crate::error::{AppError, AppResult};
use std::path::Path;

/// 旧 identifier，也就是旧数据目录的目录名。
const LEGACY_IDENTIFIER: &str = "com.heiheids.schematic-manager";

/// 把 `old_root` 里的条目逐个搬进 `new_root`。
///
/// 目标里已有同名条目时保留目标那一份、旧的那份留在原地：哪一份才是用户想要
/// 的无法自动判断，替他选就可能丢掉方块库。旧目录搬空后删掉，还留着同名条目
/// 时保留原地，下次启动会再试一次。
fn take_over(old_root: &Path, new_root: &Path) -> AppResult<()> {
    if !old_root.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(new_root)?;

    for entry in std::fs::read_dir(old_root)? {
        let entry = entry?;
        let dest = new_root.join(entry.file_name());
        if dest.exists() {
            continue;
        }
        std::fs::rename(entry.path(), &dest)
            .map_err(|e| migration_failed(&entry.path(), &dest, &e))?;
    }

    if std::fs::read_dir(old_root)?.next().is_none() {
        std::fs::remove_dir(old_root).map_err(|e| migration_failed(old_root, new_root, &e))?;
    }
    Ok(())
}

/// 搬迁失败的错误，带上新旧路径：用户要照着提示自己把数据挪回去。
fn migration_failed(from: &Path, to: &Path, cause: &std::io::Error) -> AppError {
    AppError::Io(std::io::Error::new(
        cause.kind(),
        format!("把 {} 搬到 {} 失败：{cause}", from.display(), to.display()),
    ))
}

/// 迁移旧 identifier 留下的数据目录。
///
/// 应用数据目录与配置目录分别处理：Windows 上两者是同一个目录，第一次搬完
/// 之后第二次无事可做（旧目录已经不在了）。
pub fn migrate_legacy_data(app: &tauri::AppHandle) -> AppResult<()> {
    use tauri::Manager;

    for new_root in [app.path().app_data_dir(), app.path().app_config_dir()] {
        let new_root =
            new_root.map_err(|e| AppError::Parse(format!("无法定位应用目录：{e}")))?;
        let parent = new_root.parent().ok_or_else(|| {
            AppError::Parse(format!("应用目录没有上级目录：{}", new_root.display()))
        })?;
        take_over(&parent.join(LEGACY_IDENTIFIER), &new_root)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/app/data_dir.rs"]
mod tests;
