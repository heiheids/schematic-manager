//! 启动失败时的可见反馈。
//!
//! 发布版以 `windows_subsystem = "windows"` 编译，**没有控制台**：
//! panic 信息无处可见，用户双击后只会看到「什么都没发生」。
//!
//! 这不是假想的边界情况：实测把 WebView2 的用户数据目录弄成损坏状态后，
//! 应用会立刻退出、不留任何痕迹，连日志都没有。用户无从判断是程序坏了、
//! 缺运行库，还是被杀毒软件拦了。
//!
//! 因此启动失败走两条可见途径：
//!   1. 写一份日志（优先程序目录，不可写时退到临时目录）
//!   2. Windows 上弹一个原生消息框
//!
//! 消息框直接调用 user32 的 MessageBoxW，不引入新依赖：为一个弹窗
//! 增加一个 crate 不划算，而这只需要几个 FFI 声明。

use std::path::PathBuf;

/// 启动失败日志的文件名
const LOG_NAME: &str = "启动失败.log";

/// 写日志并返回实际写入的位置。
///
/// 优先程序所在目录（用户容易找到），但程序可能装在 Program Files 这类
/// 不可写的位置，因此退到临时目录。两处都失败时返回 None——此时只能靠
/// 消息框把错误显示给用户。
fn write_log(text: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
    {
        candidates.push(dir.join(LOG_NAME));
    }
    candidates.push(std::env::temp_dir().join(format!("schematic-manager-{LOG_NAME}")));

    for p in candidates {
        if std::fs::write(&p, text).is_ok() {
            return Some(p);
        }
    }
    None
}

/// 安装 panic 钩子，把崩溃也变成可见反馈。
///
/// 为什么必须有：Tauri 在建立窗口失败时是 **panic**，不是返回 `Err`。
/// 实测它内部报的是
///   `Failed to setup app: runtime error: failed to create webview`
/// 且直接终止进程。若只判断 `run()` 的返回值，这条路径完全捕获不到，
/// 用户依旧看到「双击没反应」。
///
/// 钩子里不显式写出 `PanicHookInfo` 的类型名：该类型在 Rust 1.82 才有，
/// 而本 crate 声明的最低版本是 1.77。不写类型让闭包自行推断，
/// 两边都能编译。
///
/// `identifier` 取自 tauri.conf.json，会被存进钩子闭包：panic 发生时拿不到
/// AppHandle，而 WebView2 失败的提示里要写出用户数据目录。
pub fn install_panic_hook(identifier: String) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut msg = match info.payload().downcast_ref::<&str>() {
            Some(s) => (*s).to_string(),
            None => match info.payload().downcast_ref::<String>() {
                Some(s) => s.clone(),
                None => "未知错误".to_string(),
            },
        };
        if let Some(loc) = info.location() {
            msg.push_str(&format!("\n（{}:{}）", loc.file(), loc.line()));
        }
        // 保留默认钩子：它会把信息打到 stderr，便于从终端启动时排查
        default_hook(info);
        report(&msg, &identifier);
    }));
}

/// 构造要展示给用户的正文。
///
/// 与弹窗和写日志分开，便于直接测试其内容——否则测试只能靠复现同一段
/// 拼接逻辑，测的就不是真实代码了。
///
/// `identifier` 由调用方传入而不是写死在这里：它必须与 tauri.conf.json 里的
/// 一致，写死就会在改标识符时悄悄失准，把用户指向一个不存在的目录。
fn build_body(err: &str, exe: &str, log_path: Option<&str>, identifier: &str) -> String {
    let mut body = String::new();
    body.push_str("投影文件管理器启动失败\n\n");
    body.push_str(&format!("原因：{err}\n\n"));
    body.push_str(&format!("程序位置：{exe}\n"));

    // WebView2 建不起来时，最常见的原因是用户数据目录损坏。
    // 实测：换一个全新的该目录后应用即可正常启动。因此这里给出可操作的
    // 建议，而不是让用户对着一个 HRESULT 发呆。
    if err.to_ascii_lowercase().contains("webview") {
        body.push_str(
            "\n这通常是界面组件（WebView2）的用户数据目录损坏所致。\n\
             可尝试删除下面这个目录后重新启动：\n",
        );
        match std::env::var("LOCALAPPDATA") {
            Ok(local) => body.push_str(&format!("  {local}\\{identifier}\n")),
            Err(_) => body.push_str(&format!("  %LOCALAPPDATA%\\{identifier}\n")),
        }
        body.push_str(
            "\n若仍无法启动，请确认系统已安装 Microsoft Edge WebView2 运行时：\n\
             https://developer.microsoft.com/microsoft-edge/webview2/\n",
        );
    }

    match log_path {
        Some(p) => body.push_str(&format!("\n详细信息已写入：{p}\n")),
        None => body.push_str("\n（无法写入日志文件，请截图本窗口）\n"),
    }
    body
}

/// 抑制弹窗的环境变量名。
///
/// 给自动化场景用：模态弹窗会一直阻塞直到有人点「确定」，在无人值守的
/// 环境（CI、远程会话）里会让进程永久挂起——这正是它存在的理由，
/// 而不是为了图方便。
///
/// 设置后错误仍会写日志，只是不弹窗。**默认不设置**，因此真实用户的
/// 行为不受影响。
pub const NO_DIALOG_ENV: &str = "SCHEMATIC_MANAGER_NO_ERROR_DIALOG";

/// 是否应当弹窗。
///
/// 只在环境变量明确取 1/true 时抑制，避免因变量被设成空串或 "0"
/// 而意外改变用户可见行为。
fn dialog_allowed() -> bool {
    match std::env::var(NO_DIALOG_ENV) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !(v == "1" || v == "true" || v == "yes")
        }
        Err(_) => true,
    }
}

/// 把启动失败报告给用户：写日志 + 弹窗，并返回展示过的文本。
pub fn report(err: &str, identifier: &str) -> String {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "(未知)".into());

    // 先把日志写掉，再把路径写进正文——顺序不能反，否则正文里的
    // 路径与实际写入位置可能不一致
    let probe = build_body(err, &exe, None, identifier);
    let log = write_log(&probe);
    let body = build_body(
        err,
        &exe,
        log.as_ref().map(|p| p.display().to_string()).as_deref(),
        identifier,
    );
    if let Some(p) = &log {
        // 用最终正文覆盖，使日志内容与弹窗完全一致
        let _ = std::fs::write(p, &body);
    }

    if dialog_allowed() {
        show_message(&body);
    } else {
        // 抑制弹窗时把正文写到 stderr，便于从终端或 CI 日志里看到
        eprintln!("{body}");
    }
    body
}

#[cfg(windows)]
pub fn show_message(text: &str) {
    // 直接声明 user32 的 MessageBoxW：只需一个函数，不值得引入依赖。
    // 参数字符串必须是 UTF-16 且以 0 结尾。
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(
            hwnd: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            utype: u32,
        ) -> i32;
    }
    const MB_OK: u32 = 0x0000_0000;
    const MB_ICONERROR: u32 = 0x0000_0010;
    const MB_TOPMOST: u32 = 0x0004_0000;

    let to_wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let text_w = to_wide(text);
    let cap_w = to_wide("投影文件管理器");
    // SAFETY: 两个字符串都是本函数内的局部量、以 0 结尾、在调用期间存活；
    // 窗口句柄为 null 表示不指定父窗口，是 MessageBoxW 允许的取值。
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text_w.as_ptr(),
            cap_w.as_ptr(),
            MB_OK | MB_ICONERROR | MB_TOPMOST,
        );
    }
}

/// 非 Windows 平台没有统一的原生弹窗，写到 stderr。
///
/// Linux/macOS 上从终端启动时 stderr 可见；桌面环境下用户可从日志文件看到。
#[cfg(not(windows))]
pub fn show_message(text: &str) {
    eprintln!("{text}");
}

#[cfg(test)]
#[path = "../tests/app/startup.rs"]
mod tests;
