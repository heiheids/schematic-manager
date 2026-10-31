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
    CANECL, // 根据 Win32 文档，此处为 Escape 返回
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
    HICON(HICON),
}

pub struct MessageWindow {
    icon: MessageWindowIcon,
    buttons: Vec<MessageWindowButton>,
    title: String,
    instruction: String,
    content: String,
    expanded_info: String


    // Default::default()
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
    pub fn show(self) -> windows::core::Result<MessageWindowDialog> { 
        use windows::Win32::UI::Controls::{TD_ERROR_ICON, TD_SHIELD_ICON, TD_WARNING_ICON};
        use windows::Win32::UI::Controls::{TDCBF_ABORT_BUTTON, TDCBF_CONTINUE_BUTTON, TDCBF_HELP_BUTTON, TDCBF_IGNORE_BUTTON, TDCBF_TRYAGAIN_BUTTON};
        use windows::Win32::UI::WindowsAndMessaging::{MESSAGEBOX_RESULT, IDABORT, IDCANCEL, IDNO, IDCLOSE, IDCONTINUE, IDHELP, IDIGNORE, IDOK, IDRETRY, IDTRYAGAIN, IDYES};

        let mut p_buttons = vec![];
        let mut c_buttons = TASKDIALOG_COMMON_BUTTON_FLAGS(0);
        let mut button_texts: Vec<HSTRING> = Vec::new(); // 文本指针指向，目的是为了防止提前释放内存导致的非预期行为

        let content = HSTRING::from(self.content);
        let title = HSTRING::from(self.title);
        let instruction = HSTRING::from(self.instruction);
        let expanded_info = HSTRING::from(self.expanded_info);

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
                    let text = HSTRING::from(content.clone());

                    p_buttons.push(TASKDIALOG_BUTTON {
                        nButtonID: int_var,
                        pszButtonText: PCWSTR(text.as_ptr()),
                    });

                    button_texts.push(text);
                }
            }
        }

        let icon = match self.icon {
            MessageWindowIcon::ERROR              => TASKDIALOGCONFIG_0 { pszMainIcon: TD_ERROR_ICON },
            MessageWindowIcon::INFO               => TASKDIALOGCONFIG_0 { pszMainIcon: TD_INFORMATION_ICON },
            MessageWindowIcon::SHIELD             => TASKDIALOGCONFIG_0 { pszMainIcon: TD_SHIELD_ICON },
            MessageWindowIcon::WARN               => TASKDIALOGCONFIG_0 { pszMainIcon: TD_WARNING_ICON },
            MessageWindowIcon::NULL               => TASKDIALOGCONFIG_0 { pszMainIcon: PCWSTR::null()},
            MessageWindowIcon::HICON(icon) => TASKDIALOGCONFIG_0 { hMainIcon: icon },
            _                                     => TASKDIALOGCONFIG_0 { pszMainIcon: PCWSTR::null()},
        };
        
        let config = TASKDIALOGCONFIG {
            cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
            hwndParent: HWND::default(),
            dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,

            dwCommonButtons: c_buttons,
            
            pszWindowTitle: PCWSTR(title.as_ptr()),
            pszMainInstruction: PCWSTR(instruction.as_ptr()),
            pszContent: PCWSTR(content.as_ptr()),

            cButtons: p_buttons.len() as u32,
            pButtons: p_buttons.as_ptr(),

            Anonymous1: icon,
            Anonymous2: TASKDIALOGCONFIG_1::default(),

            pszExpandedInformation: PCWSTR(expanded_info.as_ptr()),

            ..Default::default()

        };

        let mut button_pressed = 0i32;         
        let mut radio_pressed = 0i32;          
        let mut verification_checked = BOOL(0);
        
        unsafe {
            TaskDialogIndirect(                 
                &config,                        
                Some(&mut button_pressed),      
                Some(&mut radio_pressed),       
                Some(&mut verification_checked),
            )?; // 后两个参数保留备用
        }

        let button_pressed_box = MESSAGEBOX_RESULT(button_pressed);

        return Ok(match button_pressed_box {
            IDABORT      => MessageWindowDialog::ABORT   ,
            IDCANCEL     => MessageWindowDialog::CANECL  ,
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

    #[cfg(not(target_os = "windows"))]
    pub fn show(self: Self) -> MessageWindowDialog {
        // 暂失，等下写
    }
    
}


/// 调试垃圾场，后续删除

#[cfg(test)]
mod test {
    use crate::logger::{init_log_object}; // 我不认为 Never Use，因为删了报错

    #[test]
    #[cfg(target_os = "windows")]
    fn sdsdsdsd() {
        use rfd::MessageDialog;
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

