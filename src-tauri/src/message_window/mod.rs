#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::{TASKDIALOG_COMMON_BUTTON_FLAGS, TDCBF_CANCEL_BUTTON, TDCBF_CLOSE_BUTTON, TDCBF_NO_BUTTON, TDCBF_RETRY_BUTTON, TDCBF_YES_BUTTON};
#[cfg(target_os = "windows")]
use windows::core::{HSTRING, PCWSTR};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{BOOL, HWND};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::{
    TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0, TASKDIALOGCONFIG_1,
    TASKDIALOG_BUTTON, TDCBF_OK_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION, TD_INFORMATION_ICON,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::HICON;

#[cfg(not(target_os = "windows"))]
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};

use crate::logger::LEVEL;
use std::panic::Location;


/// 由于 rfd::MessageDialog 不支持太多窗口按钮类型组合，导致能用的只有这么几个 ------ @see <https://docs.rs/rfd/latest/rfd/struct.MessageDialog.html>
/// // rfd::MessageButtons

/// 如果想知道 TaskDialog 的参数，参见下面
/// @see <https://learn.microsoft.com/zh-cn/windows/win32/api/commctrl/ns-commctrl-taskdialogconfig> 

/// TaskDialog 所使用的基本按钮，自定义 ------ @see <https://learn.microsoft.com/zh-cn/windows/win32/api/commctrl/nf-commctrl-taskdialog>
pub enum MessageWindowButton {
    ABORT,
    CANCEL,
    CLOSE,
    CONTINUE,
    HELP,
    IGNORE,
    NO,
    OK,
    RETRY,
    TRYAGAIN,
    YES,
    Custom(i32, String),
}

/// 返回值
pub enum MessageWindowDialog {
    NULL,
    ABORT,
    CANCEL, // 根据 Win32 文档，此处为 Escape 返回
    CLOSE,
    CONTINUE,
    HELP,
    IGNORE,
    NO,
    OK,
    RETRY,
    TRYAGAIN,
    YES,
    Custom(i32),
}

/// 图标类型，基于 Windows 设计，不支持的平台将会转义/抛弃
#[non_exhaustive]
pub enum MessageWindowIcon {
    NULL,
    ERROR,
    SHIELD,
    WARN,
    INFO,
    #[cfg(windows)]
    HICON(HICON), // 不建议使用此标志，它在非 Windows 上没有实现
}

#[derive(Debug)]
pub enum MessageWindowError {
    /// Windows 上 TaskDialog 自身的失败。非 Windows 平台没有 windows crate，故不参与编译。
    #[cfg(target_os = "windows")]
    WindowsSystem(windows::core::Error),    // Win32
    TooManyButtons(usize),           // 非 Windows 上按钮超过 3 个

}

pub struct MessageWindow {
    icon: MessageWindowIcon,
    buttons: Vec<MessageWindowButton>,
    title: String,
    instruction: String,
    content: String,
    expanded_info: String
}

impl std::fmt::Display for MessageWindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WindowsSystem(e) => write!(f, "Windows API 调用失败: {e}"),
            Self::TooManyButtons(n) => write!(f, "按钮数量 {n} 超出非 Windows 平台的上限（最多 3 个）"),
        }
    }
}

impl std::error::Error for MessageWindowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::WindowsSystem(e) => Some(e),
            _ => None,
        }
    }
}

impl MessageWindow {
    pub fn new() -> Self {
        Self {
            icon: MessageWindowIcon::NULL,
            buttons: Vec::with_capacity(3),
            title: String::new(),
            instruction: String::new(),
            content: String::new(),
            expanded_info: String::new(),

        }
    }

    pub fn set_icon(mut self, icon: MessageWindowIcon) -> Self {
        self.icon = icon;
        self
    }

    pub fn set_buttons(mut self, buttons: Vec<MessageWindowButton>) -> Self {
        self.buttons = buttons;
        self
    }

    pub fn set_rfd_buttons(mut self, buttons: rfd::MessageButtons) -> Self {
        self.buttons = Self::buttons_to_button(buttons);
        self
    }

    fn buttons_to_button(buttons: rfd::MessageButtons) -> Vec<MessageWindowButton> {
        match buttons {
            rfd::MessageButtons::Ok          => vec![MessageWindowButton::OK],
            rfd::MessageButtons::OkCancel    => vec![MessageWindowButton::OK, MessageWindowButton::CANCEL],
            rfd::MessageButtons::YesNo       => vec![MessageWindowButton::YES, MessageWindowButton::NO],
            rfd::MessageButtons::YesNoCancel => vec![MessageWindowButton::YES, MessageWindowButton::NO, MessageWindowButton::CANCEL],

            rfd::MessageButtons::OkCustom(ok_string)                                                     => vec![MessageWindowButton::Custom(100, ok_string)],
            rfd::MessageButtons::OkCancelCustom(ok_string, cancel_string)                        => vec![MessageWindowButton::Custom(100, ok_string), MessageWindowButton::Custom(101, cancel_string)],
            rfd::MessageButtons::YesNoCancelCustom(yes_string, no_string, cancel_string) => vec![MessageWindowButton::Custom(100, yes_string), MessageWindowButton::Custom(101, no_string), MessageWindowButton::Custom(102, cancel_string)],
        }
    }

    pub fn set_title(mut self, title: &str) -> Self {
        self.title = title.to_string();
        self
    }

    pub fn set_instruction(mut self, instruction: &str) -> Self {
        self.instruction = instruction.to_string();
        self
    }

    pub fn set_content(mut self, content: &str) -> Self {
        self.content = content.to_string();
        self
    }

    pub fn set_expanded_info(mut self, expanded_info: &str) -> Self {
        self.expanded_info = expanded_info.to_string();
        self
    }

    #[cfg(target_os = "windows")]
    pub fn show(self) -> Result<MessageWindowDialog, MessageWindowError> {
        use windows::Win32::UI::Controls::{TD_ERROR_ICON, TD_SHIELD_ICON, TD_WARNING_ICON};
        use windows::Win32::UI::Controls::{TDCBF_ABORT_BUTTON, TDF_USE_HICON_MAIN, TDCBF_CONTINUE_BUTTON, TDCBF_HELP_BUTTON, TDCBF_IGNORE_BUTTON, TDCBF_TRYAGAIN_BUTTON};
        use windows::Win32::UI::WindowsAndMessaging::{MESSAGEBOX_RESULT, IDABORT, IDCANCEL, IDNO, IDCLOSE, IDCONTINUE, IDHELP, IDIGNORE, IDOK, IDRETRY, IDTRYAGAIN, IDYES};

        let mut p_buttons = vec![];
        let mut c_buttons = TASKDIALOG_COMMON_BUTTON_FLAGS(0);
        let mut button_texts: Vec<HSTRING> = Vec::new(); // 文本指针指向，目的是为了防止提前释放内存导致的非预期行为
        let mut flags = TDF_ALLOW_DIALOG_CANCELLATION;

        let content = HSTRING::from(self.content);
        let title = HSTRING::from(self.title);
        let instruction = HSTRING::from(self.instruction);
        let expanded_info = if self.expanded_info.is_empty() {None} else {Some(HSTRING::from(self.expanded_info))};

        if self.buttons.len() > 3 {
            if let Some(logger) = crate::logger::LOGGER.get() {
                logger.write_log_caller(LEVEL::WARN, "message_windows", &format!("Unexpected behavior that may occur on non-Windows systems: the number of buttons exceeds 3, and the number of incoming buttons is {}.", self.buttons.len()), Location::caller());
            }
        };

        for item in self.buttons {
            match item {
                MessageWindowButton::ABORT      => c_buttons = c_buttons | TDCBF_ABORT_BUTTON,
                MessageWindowButton::CANCEL     => c_buttons = c_buttons | TDCBF_CANCEL_BUTTON,
                MessageWindowButton::CLOSE      => c_buttons = c_buttons | TDCBF_CLOSE_BUTTON,
                MessageWindowButton::CONTINUE   => c_buttons = c_buttons | TDCBF_CONTINUE_BUTTON,
                MessageWindowButton::HELP       => c_buttons = c_buttons | TDCBF_HELP_BUTTON,
                MessageWindowButton::IGNORE     => c_buttons = c_buttons | TDCBF_IGNORE_BUTTON,
                MessageWindowButton::NO         => c_buttons = c_buttons | TDCBF_NO_BUTTON,
                MessageWindowButton::OK         => c_buttons = c_buttons | TDCBF_OK_BUTTON,
                MessageWindowButton::RETRY      => c_buttons = c_buttons | TDCBF_RETRY_BUTTON,
                MessageWindowButton::TRYAGAIN   => c_buttons = c_buttons | TDCBF_TRYAGAIN_BUTTON,
                MessageWindowButton::YES        => c_buttons = c_buttons | TDCBF_YES_BUTTON,
                
                MessageWindowButton::Custom(int_var, content) => {
                    let text = HSTRING::from(content);

                    p_buttons.push(TASKDIALOG_BUTTON {
                        nButtonID: int_var,
                        pszButtonText: PCWSTR(text.as_ptr()),
                    });

                    button_texts.push(text);
                }
            }
        }

        let (icon, use_h_icon) = match self.icon {
            MessageWindowIcon::ERROR              => (TASKDIALOGCONFIG_0 { pszMainIcon: TD_ERROR_ICON }, false),
            MessageWindowIcon::INFO               => (TASKDIALOGCONFIG_0 { pszMainIcon: TD_INFORMATION_ICON }, false),
            MessageWindowIcon::SHIELD             => (TASKDIALOGCONFIG_0 { pszMainIcon: TD_SHIELD_ICON }, false),
            MessageWindowIcon::WARN               => (TASKDIALOGCONFIG_0 { pszMainIcon: TD_WARNING_ICON }, false),
            MessageWindowIcon::NULL               => (TASKDIALOGCONFIG_0 { pszMainIcon: PCWSTR::null()}, false),
            MessageWindowIcon::HICON(icon) => (TASKDIALOGCONFIG_0 { hMainIcon: icon }, true),
        };
        if use_h_icon { flags |= TDF_USE_HICON_MAIN; }; // 根据 Win32 文档，此处需要设置标志

        let config = TASKDIALOGCONFIG {
            cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
            hwndParent: HWND::default(),
            dwFlags: flags,

            dwCommonButtons: c_buttons,
            
            pszWindowTitle: PCWSTR(title.as_ptr()),
            pszMainInstruction: PCWSTR(instruction.as_ptr()),
            pszContent: PCWSTR(content.as_ptr()),

            cButtons: p_buttons.len() as u32,
            pButtons: p_buttons.as_ptr(),

            Anonymous1: icon,
            Anonymous2: TASKDIALOGCONFIG_1::default(),

            pszExpandedInformation: expanded_info.as_ref().map_or(PCWSTR::null(), |h| PCWSTR(h.as_ptr())),

            ..Default::default()

        };

        let mut button_pressed = 0i32;         
        let mut radio_pressed = 0i32;          
        let mut verification_checked = BOOL(0);
        
        unsafe {
            match TaskDialogIndirect(
                &config,                        
                Some(&mut button_pressed),      
                Some(&mut radio_pressed),       
                Some(&mut verification_checked),
            ) { // 后两个参数保留备用
                Err(error) => {return Err(MessageWindowError::WindowsSystem(error))},
                Ok(()) => {},
            }
        }

        let button_pressed_box = MESSAGEBOX_RESULT(button_pressed);

        return Ok(match button_pressed_box {
            IDABORT      => MessageWindowDialog::ABORT   ,
            IDCANCEL     => MessageWindowDialog::CANCEL  ,
            IDCLOSE      => MessageWindowDialog::CLOSE   ,
            IDCONTINUE   => MessageWindowDialog::CONTINUE,
            IDHELP       => MessageWindowDialog::HELP    ,
            IDIGNORE     => MessageWindowDialog::IGNORE  ,
            IDNO         => MessageWindowDialog::NO      ,
            IDOK         => MessageWindowDialog::OK      ,
            IDRETRY      => MessageWindowDialog::RETRY   ,
            IDTRYAGAIN   => MessageWindowDialog::TRYAGAIN,
            IDYES        => MessageWindowDialog::YES     ,
            _            => MessageWindowDialog::Custom(button_pressed),
        });
    }

    /// 非 Windows 平台用 rfd 的对话框显示消息。
    ///
    /// 参数含义与 Windows 版一致：title 是窗口标题，instruction 与 content 依次拼成正文，
    /// icon 映射到 rfd 的等级。rfd 的 `MessageButtons` 只定义了 OK、OK+取消、是+否、
    /// 是+否+取消四种固定组合，因此按钮组合与之不符或超过 3 个时见 [`to_rfd_buttons`]。
    /// AIGC
    /// 尚未人工审核和 TEST
    #[cfg(not(target_os = "windows"))]
    pub fn show(self) -> Result<MessageWindowDialog, MessageWindowError> {
        let rfd_buttons = to_rfd_buttons(&self.buttons)?;

        let result = MessageDialog::new()
            .set_title(self.title)
            .set_description(join_description(
                &self.instruction,
                &self.content,
                &self.expanded_info,
            ))
            .set_level(to_rfd_level(&self.icon))
            .set_buttons(rfd_buttons)
            .show();

        Ok(to_dialog(result, &self.buttons))
    }
    
}

/// rfd 只定义了 OK、Yes、No、取消四种固定按钮，其余按钮必须以文本形式传入，
/// 这里给出每种按钮的固定显示文案。
///
/// AIGC
#[cfg(not(target_os = "windows"))]
fn button_text(button: &MessageWindowButton) -> &str {
    match button {
        MessageWindowButton::ABORT      => "Abort",
        MessageWindowButton::CANCEL     => "Cancel",
        MessageWindowButton::CLOSE      => "Close",
        MessageWindowButton::CONTINUE   => "Continue",
        MessageWindowButton::HELP       => "Help",
        MessageWindowButton::IGNORE     => "Ignore",
        MessageWindowButton::NO         => "No",
        MessageWindowButton::OK         => "OK",
        MessageWindowButton::RETRY      => "Retry",
        MessageWindowButton::TRYAGAIN   => "Tryagnin",
        MessageWindowButton::YES        => "Yes",
        MessageWindowButton::Custom(_, text) => text.as_str(),
    }
}

/// 把按钮列表翻译成 rfd 的按钮组合。
///
/// 按钮组合正好是 rfd 的四种固定组合之一时直接用固定组合，让对话框使用系统自己的按钮文案；
/// 其余组合退回对应的 `...Custom` 变体，按钮文案取自 [`button_text`]。
/// rfd 最多接受 3 个按钮，超出时报 [`MessageWindowError::TooManyButtons`]。
///
/// 自定义组合里最后一个按钮在 GTK 与 macOS 上同时承担 Esc 的角色，这与 TaskDialog
/// 的「Esc 一律返回取消」不完全一致，是 rfd 没有按钮身份概念的必然结果。
/// AIGC
#[cfg(not(target_os = "windows"))]
fn to_rfd_buttons(buttons: &[MessageWindowButton]) -> Result<MessageButtons, MessageWindowError> {
    match buttons {
        [MessageWindowButton::OK] => Ok(MessageButtons::Ok),
        [MessageWindowButton::OK, MessageWindowButton::CANCEL] => Ok(MessageButtons::OkCancel),
        [MessageWindowButton::YES, MessageWindowButton::NO] => Ok(MessageButtons::YesNo),
        [MessageWindowButton::YES, MessageWindowButton::NO, MessageWindowButton::CANCEL] => {
            Ok(MessageButtons::YesNoCancel)
        }

        // rfd 没有零按钮的对话框；空列表按单个「确定」显示，至少让提示能被关掉
        [] => Ok(MessageButtons::Ok),
        [button] => Ok(MessageButtons::OkCustom(button_text(button).to_string())),
        [first, second] => Ok(MessageButtons::OkCancelCustom(
            button_text(first).to_string(),
            button_text(second).to_string(),
        )),
        [first, second, third] => Ok(MessageButtons::YesNoCancelCustom(
            button_text(first).to_string(),
            button_text(second).to_string(),
            button_text(third).to_string(),
        )),

        _ => Err(MessageWindowError::TooManyButtons(buttons.len())),
    }
}

/// 把 rfd 的返回值翻译回本模块的返回值。
///
/// rfd 对固定组合回报 OK、Yes、No、取消，对自定义组合回报按钮文本；但 zenity 后端
/// （Linux 未启用 gtk3 时的默认实现，且 XDG portal 也转交给它）对单按钮的自定义组合
/// 只回报 OK。因此 OK 一律按「第一个按钮」解释：固定组合的第一个按钮本来就是 OK，
/// 自定义组合的第一个按钮也正好落在 rfd 的 OK 位上。
///
/// 按文本反查时，文本重复就取声明顺序里的第一个：rfd 不回报按钮身份，无从区分。
/// 找不到对应按钮（例如对话框被直接关闭）返回 NULL。
/// AIGC
#[cfg(not(target_os = "windows"))]
fn to_dialog(result: MessageDialogResult, buttons: &[MessageWindowButton]) -> MessageWindowDialog {
    match result {
        MessageDialogResult::Ok => buttons
            .first()
            .map_or(MessageWindowDialog::OK, dialog_for_button),
        MessageDialogResult::Yes    => MessageWindowDialog::YES,
        MessageDialogResult::No     => MessageWindowDialog::NO,
        MessageDialogResult::Cancel => MessageWindowDialog::CANCEL,
        MessageDialogResult::Custom(text) => buttons
            .iter()
            .find(|button| button_text(button) == text.as_str())
            .map(dialog_for_button)
            .unwrap_or(MessageWindowDialog::NULL),
    }
}

/// 单个按钮对应的返回值。
/// AIGC
#[cfg(not(target_os = "windows"))]
fn dialog_for_button(button: &MessageWindowButton) -> MessageWindowDialog {
    match button {
        MessageWindowButton::ABORT      => MessageWindowDialog::ABORT,
        MessageWindowButton::CANCEL     => MessageWindowDialog::CANCEL,
        MessageWindowButton::CLOSE      => MessageWindowDialog::CLOSE,
        MessageWindowButton::CONTINUE   => MessageWindowDialog::CONTINUE,
        MessageWindowButton::HELP       => MessageWindowDialog::HELP,
        MessageWindowButton::IGNORE     => MessageWindowDialog::IGNORE,
        MessageWindowButton::NO         => MessageWindowDialog::NO,
        MessageWindowButton::OK         => MessageWindowDialog::OK,
        MessageWindowButton::RETRY      => MessageWindowDialog::RETRY,
        MessageWindowButton::TRYAGAIN   => MessageWindowDialog::TRYAGAIN,
        MessageWindowButton::YES        => MessageWindowDialog::YES,
        MessageWindowButton::Custom(id, _) => MessageWindowDialog::Custom(*id),
    }
}

/// TaskDialog 的图标在 rfd 里只有信息、警告、错误三档，没有对应物的类型降级为信息。
/// AIGC
#[cfg(not(target_os = "windows"))]
fn to_rfd_level(icon: &MessageWindowIcon) -> MessageLevel {
    match icon {
        MessageWindowIcon::ERROR  => MessageLevel::Error,
        MessageWindowIcon::WARN   => MessageLevel::Warning,
        MessageWindowIcon::NULL | MessageWindowIcon::SHIELD | MessageWindowIcon::INFO => {
            MessageLevel::Info
        }
    }
}

/// rfd 的对话框只有标题和一段正文，没有 TaskDialog 的指令区与可折叠的详情区，
/// 因此把非空的指令、正文、详情按顺序拼进正文。
/// AIGC
#[cfg(not(target_os = "windows"))]
fn join_description(instruction: &str, content: &str, expanded_info: &str) -> String {
    let parts: Vec<&str> = [instruction, content, expanded_info]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    parts.join("\n\n")
}



/// 调试垃圾场，后续删除

#[cfg(test)]
mod test {
    use crate::logger::{init_log_object}; // 我不认为 Never Use，因为删了报错

    #[test]
    #[cfg(target_os = "windows")]
    fn sdsdsdsd() {
        use std::path::PathBuf;
        use crate::logger::LOGGER;
        use crate::logger::LEVEL;

        init_log_object(&PathBuf::from("H:\\"));

        if let Some(logger) = LOGGER.get() {
            logger.write_log(LEVEL::INFO, "TEST", "测试");
            logger.info("SSS", "");
        }
        
        // let _ = MessageWindow::new()

        // let _ = MessageWindow::new()
        //     .set_title("Log System Error")
        //     .set_content(format!("Log system initialization: archived old log files.\nYou can still choose to continue running the program, but the archive log is lost.\n\n {}", "sss"))
        //     .show();
    }

    #[test]
    fn fnffff() {
        // LogFileObject::write_log(&self, super::LEVEL::ERROR, "ssds {} wdwsd {} wdrwr %s", "12", "2353253wr23rt23", "3253265fefdfds5g4d5g5d4g5");
        
        #[cfg(target_os = "windows")]
        pub fn show_message_box(message: &str, title: &str) {
            // use windows::core::w;
            use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_OK, MB_ICONERROR};
        
            // windows crate 提供的 w! 宏可以方便地将字符串字面量转为宽字符串，
            // 但变量的宽字符串转换需要我们自己处理。
            let message_wide: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
            let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        
            unsafe {
                MessageBoxW(
                    None, // 父窗口句柄，None 表示没有父窗口
                    windows::core::PCWSTR(message_wide.as_ptr()),
                    windows::core::PCWSTR(title_wide.as_ptr()),
                    MB_OK | MB_ICONERROR, // 按钮和图标标志
                );
            }
        }

        // 调用示例
        show_message_box("无法打开日志文件！", "错误");
        
        return;
    }
}


// 参考代码，后续删除

// fn maince() -> windows::core::Result<()> {
//     // 1. 字符串先绑定到变量，保证生命周期覆盖整个调用
//     let title       = HSTRING::from("提示");
//     let instruction = HSTRING::from("这是一个 TaskDialog 示例");
//     let content     = HSTRING::from("日志系统初始化是出现错误");
//     let ok_text     = HSTRING::from("确定(&O)");
//     let cancel_text = HSTRING::from("终止(&B)");
//     let buttons = [
//         TASKDIALOG_BUTTON { nButtonID: 100, pszButtonText: PCWSTR(ok_text.as_ptr()) },
//         TASKDIALOG_BUTTON { nButtonID: 200, pszButtonText: PCWSTR(cancel_text.as_ptr()) },
//     ];
//     // 2. 配置
//     let config = TASKDIALOGCONFIG {
//         cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
//         hwndParent: HWND::default(),
//         dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,
//         dwCommonButtons: TDCBF_OK_BUTTON,
//         pszWindowTitle:     PCWSTR(title.as_ptr()),
//         pszMainInstruction: PCWSTR(instruction.as_ptr()),
//         pszContent:         PCWSTR(content.as_ptr()),
//         cButtons: buttons.len() as u32,
//         pButtons: buttons.as_ptr(),
//         Anonymous1: TASKDIALOGCONFIG_0 { pszMainIcon: TD_INFORMATION_ICON }, // ✅ 字段名修正
//         Anonymous2: TASKDIALOGCONFIG_1::default(),
//         ..Default::default()
//     };
//     // 3. 调用：输出参数用 Some(...) 包裹
//     let mut button_pressed = 0i32;
//     let mut radio_pressed = 0i32;
//     let mut verification_checked = BOOL(0);
//     unsafe {
//         TaskDialogIndirect(
//             &config,
//             Some(&mut button_pressed),        // ✅
//             Some(&mut radio_pressed),         // ✅ 或 None
//             Some(&mut verification_checked),  // ✅ 或 None
//         )?;
//     }
//     println!("用户点击的按钮 ID: {}", button_pressed);
//     Ok(())
// }

// pub fn message_box_info(title: &str, instruction: &str, content: &str) {
//     logger_info!("MessageBox", &format!("Pop-up message box: Information | Title: {} | Instruction: {} | Content: {}", title, instruction, content));

//     if cfg!(target_os = "windows") {
//         #[cfg(target_os = "windows")]
//         {
//             let title = HSTRING::from(title);
//             let instruction = HSTRING::from(instruction);
//             let content = HSTRING::from(content);
//             let config = TASKDIALOGCONFIG {
//                 cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
//                 hwndParent: HWND::default(),
//                 dwCommonButtons: TDCBF_OK_BUTTON,
//                 // pszWindowTitle: PSWSTR(title.as_ptr()),
//                 pszMainInstruction: PCWSTR(instruction.as_ptr()),
//                 pszContent: PCWSTR(content.as_ptr()),
//                 Anonymous1: TASKDIALOGCONFIG_0 { pszMainIcon: TD_INFORMATION_ICON },
//                 Anonymous2: TASKDIALOGCONFIG_1::default(),
//                 ..Default::default(),
//             };
//         }
//     }

// }

// pub fn message_box(title: &str, instruction: &str, content: &str, icon: i32, button_class: i32) {

// }

//
// @see <https://docs.rs/rfd/latest/rfd/struct.MessageDialog.html>
// fn message_box_message_dialog(title: &str, content: &str, icon: i32, button_class: i32) -> rfd::MessageDialogResult {
//     // let icon_level: rfd::MessageButtons = match icon {
//     //     0x00000000 => rfd::MessageButtons::OkCancel,
//     //     0x00000001 => rfd::MessageButtons::OkCancel,

//     // };

//     MessageDialog::new()
//         .set_title(title)
//         .set_description(content)
//         .set_level(rfd::MessageLevel::Error)
//         .show()
// }

