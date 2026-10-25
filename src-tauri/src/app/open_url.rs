//! 用系统默认程序打开外部链接。
//!
//! 为什么不用 tauri-plugin-opener：需求只是把链接交给默认浏览器，为此引入
//! 一个插件（Rust 依赖、前端包、权限声明三处都要改）不划算。这里按平台调用
//! 系统自带的处理程序：Windows 走 `cmd /c start`，macOS 走 `open`，Linux 走
//! `xdg-open`。
//!
//! **URL 必须当作外部数据对待**：它来自 GitHub 接口的返回，一旦被拼进命令行
//! 就可能变成命令注入。Windows 分支经 cmd 启动，而 cmd 会做变量展开与元字符
//! 解析，因此 `validate` 不是可选的加固，而是执行的前置条件——校验不过就不
//! 执行，报错交给界面显示。

use crate::error::{AppError, AppResult};
use std::process::Command;

/// 允许打开的最长 URL。浏览器与命令行的实际上限都在几千字符，
/// 这里留出余量即可；设上限是为了不让异常数据变成一次命令行调用。
const MAX_URL_LEN: usize = 2000;

/// 校验 URL 是否允许交给系统打开。
///
/// 只放行 http/https：`file:` 之类会启动本机程序，`javascript:` 一类更不该
/// 出现在这里。同时拒绝所有可能干扰命令行解析的字符——cmd 在双引号内仍会
/// 展开 `%VAR%`，`"` 则能直接闭合引号，二者都必须挡掉。
pub fn validate(url: &str) -> AppResult<()> {
    if url.is_empty() {
        return Err(AppError::Parse("链接为空".to_string()));
    }
    if url.len() > MAX_URL_LEN {
        return Err(AppError::Parse(format!(
            "链接过长（{} 字符，上限 {MAX_URL_LEN}）",
            url.len()
        )));
    }
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(AppError::Parse(format!(
            "只允许打开 http/https 链接，收到：{}",
            // 只回显前缀：整串可能很长，且这属于异常输入
            url.chars().take(32).collect::<String>()
        )));
    }
    if let Some(bad) = url.chars().find(|c| is_forbidden(*c)) {
        return Err(AppError::Parse(format!(
            "链接含有不允许的字符 {bad:?}，已阻止打开"
        )));
    }
    Ok(())
}

/// 会干扰命令行解析的字符。
///
/// 控制字符与空格能截断参数；引号、百分号、以及 cmd 的元字符在
/// `cmd /c start "" <url>` 这种调用里都可能被解释。真实的 http 链接
/// 不需要这些字符。
fn is_forbidden(c: char) -> bool {
    c.is_control()
        || c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '`' | '%' | '&' | '|' | '^' | '<' | '>' | '(' | ')' | ';' | '$' | '\\'
        )
}

/// 组装启动命令。与执行分开，便于在不真的打开浏览器的前提下核对参数。
fn command_for(url: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        // 第一个空参数是 start 的窗口标题；不给它一个占位，
        // start 会把带引号的 URL 当成标题而不是目标。
        cmd.args(["/C", "start", "", url]);
        cmd
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(url);
        cmd
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(url);
        cmd
    }
}

/// 用系统默认程序打开链接。
///
/// 不等子进程退出：`xdg-open` 可能一直等到浏览器关闭，等下去会卡住界面。
/// 但也不能放着不管——不回收会留下僵尸进程，因此起一个线程 wait 一下。
pub fn open(url: &str) -> AppResult<()> {
    validate(url)?;
    let mut child = command_for(url).spawn().map_err(|e| {
        AppError::Parse(format!("无法打开链接（{url}）：{e}"))
    })?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
#[path = "../tests/app/open_url.rs"]
mod tests;
