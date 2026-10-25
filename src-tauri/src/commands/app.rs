//! 应用级命令：版本号、更新检查与打开外部链接。

use crate::app::open_url;
use crate::app::update;
use super::dto::UpdateInfoDto;

/// 当前可执行文件的版本号。
///
/// 取自编译进二进制的包版本，而不是界面里的常量：CI 在构建前会把版本号写进
/// Cargo.toml，因此这个值就是安装包自身的版本。
#[tauri::command]
pub fn app_version() -> String {
    update::current_version().to_string()
}

/// 检查是否有新版本（读 GitHub 的发布列表）。
///
/// 网络失败按错误上抛，由界面显示原因——检测更新最需要让用户分清的是
/// 「确实没有新版本」与「没查成」，静默返回「无更新」会把后者说成前者。
#[tauri::command]
pub fn check_update() -> Result<UpdateInfoDto, String> {
    let outcome = update::check().map_err(|e| e.to_string())?;
    let newest = outcome.newest.clone();
    Ok(UpdateInfoDto {
        current: outcome.current.to_string(),
        current_prerelease: outcome.current.is_prerelease(),
        latest: newest
            .as_ref()
            .map(|r| r.version.to_string())
            .unwrap_or_default(),
        latest_prerelease: newest
            .as_ref()
            .map(|r| r.version.is_prerelease())
            .unwrap_or(false),
        tag: newest.as_ref().map(|r| r.tag.clone()).unwrap_or_default(),
        has_update: outcome.has_update(),
        release_url: newest.as_ref().map(|r| r.url.clone()).unwrap_or_default(),
        published_at: newest.map(|r| r.published_at).unwrap_or_default(),
        repo: update::REPO.to_string(),
    })
}

/// 用系统默认浏览器打开一个外部链接。
///
/// URL 来自前端（其中下载地址又来自 GitHub 接口的返回），属于外部数据，
/// 因此这里先校验再执行：只放行 http/https，并挡掉可能干扰命令行解析的
/// 字符。详见 open_url 模块的说明。
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    open_url::open(&url).map_err(|e| e.to_string())
}
