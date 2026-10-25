use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{PathBuf};
use std::sync::{Mutex, OnceLock};
use chrono::Local;
use std::panic::Location;
use rfd::MessageDialog;
#[cfg(target_os = "windows")]
use std::os::windows::fs::OpenOptionsExt;

use crate::commands::clear_run_log;



const LOG_FILE_NAME: &str = "latest.log";
const LOG_DEBUG_FILE_NAME: &str = "debug.log";

/// 单条日志最长字符长度
const LOG_ITEM_MAX_CHAR_LENGTH: u16 = 0xFFFF;
const LOG_ARCHIVES_MAX_ITEM: usize = 100;

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum LEVEL {
    FATAL,
    ERROR,
    WARN,
    INFO,
    DEBUG,
    UNKNOWN,
}

impl LEVEL {
    pub fn parse_level_class(name: &str) -> LEVEL {
        match name.trim().to_ascii_uppercase().as_str() {
            "FATAL" => return LEVEL::FATAL,
            "ERROR" => return LEVEL::ERROR,
            "WARN"  => return LEVEL::WARN,
            "INFO"  => return LEVEL::INFO,
            "DEBUG" => return LEVEL::DEBUG,
            _       => return LEVEL::UNKNOWN,
        }
    }

    pub fn get_level_str(name: LEVEL) -> & 'static str {
        match name {
            LEVEL::FATAL    => return "FATAL",
            LEVEL::ERROR    => return "ERROR",
            LEVEL::WARN     => return "WARN" ,
            LEVEL::INFO     => return "INFO" ,
            LEVEL::DEBUG    => return "DEBUG",
            LEVEL::UNKNOWN  => return "UNKNOWN",
        }
    }
}

/// 取到当前的可读时间戳
pub fn get_now_time() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}

fn get_now_date() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub struct LogFileObject {
    latest_dir: PathBuf,
    debug_dir: PathBuf,
    latest_writer: Mutex<BufWriter<File>>,
    debug_writer: Mutex<BufWriter<File>>,
}

impl LogFileObject {
    /// 初始化新的日志
    pub fn init(run_path: &PathBuf) -> Self {
        fn show_error(text: &str, title: &str) {
            println!("[{}][ERROR][::logger] {} - {}", get_now_time(), title, text);
            if cfg!(target_os = "windows") {
                use windows::core::{HSTRING, PCWSTR};
                use windows::Win32::Foundation::{BOOL, HWND};
                use windows::Win32::UI::Controls::{
                    TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0, TASKDIALOGCONFIG_1,
                    TASKDIALOG_BUTTON, TDCBF_OK_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION, TD_INFORMATION_ICON,
                };

                let title       = HSTRING::from(title);
                let instruction = HSTRING::from("Error when the log system initialization: The log output file cannot be initialized.");
                let content     = HSTRING::from(text);
                // let ok_text     = HSTRING::from("确定(&O)");
                let cancel_text = HSTRING::from("终止(&A)");
                let buttons = [
                    // TASKDIALOG_BUTTON { nButtonID: 100, pszButtonText: PCWSTR(ok_text.as_ptr()) },
                    TASKDIALOG_BUTTON { nButtonID: 100, pszButtonText: PCWSTR(cancel_text.as_ptr()) },
                ];

                let config = TASKDIALOGCONFIG {
                    cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
                    hwndParent: HWND::default(),
                    dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,
                    dwCommonButtons: TDCBF_OK_BUTTON,
                    pszWindowTitle:     PCWSTR(title.as_ptr()),
                    pszMainInstruction: PCWSTR(instruction.as_ptr()),
                    pszContent:         PCWSTR(content.as_ptr()),
                    cButtons: buttons.len() as u32,
                    pButtons: buttons.as_ptr(),
                    Anonymous1: TASKDIALOGCONFIG_0 { pszMainIcon: TD_INFORMATION_ICON }, // ✅ 字段名修正
                    Anonymous2: TASKDIALOGCONFIG_1::default(),
                    ..Default::default()
                };

                let mut button_pressed = 0i32;
                let mut radio_pressed = 0i32;
                let mut verification_checked = BOOL(0);
                unsafe {
                    let _ = TaskDialogIndirect(
                        &config,
                        Some(&mut button_pressed),        // ✅
                        Some(&mut radio_pressed),         // ✅ 或 None
                        Some(&mut verification_checked),  // ✅ 或 None
                    );
                }
            } else {
                let _ = MessageDialog::new()
                .set_title(title)
                .set_description(text)
                .set_level(rfd::MessageLevel::Error)
                .show();
            }
        }

        let latest_dir = run_path.clone();
        let debug_dir = run_path.clone();

        // 线保证日志目录一定存在
        if let Err(e) = std::fs::create_dir_all(&latest_dir) {
            show_error(&format!("Failed to create log directory: \n{}", e), "Log System Error");
            std::process::exit(0x6);
        }

        // 然后再去吧日志滚走
        match LogFileObject::roll_old_log_file(&latest_dir) {
            Ok(()) => println!("Log system initialization: archived old log files."),
            Err(e) => {
                println!("[{}][ERROR][::logger] {}", get_now_time(), e);
                let _ = MessageDialog::new()
                    .set_title("Log System Error")
                    .set_description(format!("An error occurred while archiving the log.\nYou can still choose to continue running the program, but the archive log is lost.\n\n {}", e))
                    .set_level(rfd::MessageLevel::Warning)
                    .show();
            },
        };

        LogFileObject::clean_old_archives(&latest_dir, "latest", LOG_ARCHIVES_MAX_ITEM.into());
        LogFileObject::clean_old_archives(&latest_dir, "debug", LOG_ARCHIVES_MAX_ITEM.into());

        let mut file_opts = OpenOptions::new();
        file_opts.create(true).append(true);

        #[cfg(target_os = "windows")]
        {file_opts.share_mode(0x1 | 0x2);}

        // 再去创建文件
        let latest_file = match file_opts.open(&latest_dir.join(LOG_FILE_NAME)) {
            Ok(file) => file,
            Err(e) => {
                show_error(&format!("The latest.log initialization failed and the log file could not be saved: \n{}", e), "Log System Error");
                std::process::exit(0x6);
            },
        };
        let debug_file  = match file_opts.open(&latest_dir.join(LOG_DEBUG_FILE_NAME)) {
            Ok(file) => file,
            Err(e) => {
                show_error(&format!("The debug.log initialization failed and the log file could not be saved: \n{}", e), "Log System Error");
                std::process::exit(0x6);
            },
        };

        // 然后再把自己搞回去
        let out_self = Self {
            latest_dir: run_path.clone(),
            debug_dir: run_path.clone(),
            latest_writer: Mutex::new(BufWriter::new(latest_file)),
            debug_writer: Mutex::new(BufWriter::new(debug_file)),
        };

        
        return out_self;
    }

    /// 将上次的日志压缩归档
    fn roll_old_log_file(dir: &PathBuf) -> Result<(), std::io::Error> {
        fn roll_file(name: &str, base_name: &str, dir: &PathBuf) -> Result<(), std::io::Error> {
            /// 返回下一个归档文件名：`<日期>-<次数>-<类型>.log.gz`
            /// AIGC
            fn get_next_archive_name(base_name: &str, dir: &PathBuf) -> PathBuf {
                let mut n: u64 = get_max_archive_index(base_name, &dir) + 1;
                loop {
                    let candidate = dir.join(format!("{}-{n}-{}.log.gz", get_now_date(), base_name));
                    if !candidate.exists() {
                        return candidate;
                    }
                    n += 1;
                }        
            }
        
            /// 取到目录里已有归档的最大次数；没有归档时返回 0。
            /// AIGC
            fn get_max_archive_index(base_name: &str, dir: &PathBuf) -> u64 {
                let Ok(entries) = std::fs::read_dir(dir) else {
                    return 0;
                };
                entries
                    .flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                    
                        if !name.contains(base_name) {return None;}
                    
                        let stem = name.strip_suffix(".log.gz")?;
                        let (_, index) = stem.rsplit_once('-')?;
                        index.parse::<u64>().ok()
                    })
                    .max()
                    .unwrap_or(0)
            }

            let file_current = dir.join(name);
            if  !file_current.exists() {return Ok(());} // 忽略 ~~404~~ NotFound
            let file_data = std::fs::read(&file_current)?;
            let file_packed_name = get_next_archive_name(base_name, dir);

            let mut out_packed_encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            out_packed_encoder.write_all(&file_data)?;
            let out_packed = out_packed_encoder.finish()?;

            std::fs::write(&file_packed_name, out_packed)?;
            std::fs::File::create(&file_current)?; // 创建文件
            Ok(())
        }
        roll_file( LOG_FILE_NAME, "latest", dir)?;
        roll_file( LOG_DEBUG_FILE_NAME, "debug", dir)?;
        

        Ok(())
    }

    /// 清理旧的归档日志，保留最新的 `keep_count` 个
    /// AIGC
    fn clean_old_archives(dir: &PathBuf, base_name: &str, keep_count: usize) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };

        // 1. 收集所有匹配的归档文件路径
        let mut archives: Vec<PathBuf> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                // 只匹配类似 "-latest.log.gz" 或 "-debug.log.gz" 的文件
                if name.ends_with(&format!("-{}.log.gz", base_name)) {
                    Some(entry.path())
                } else {
                    None
                }
            })
            .collect();

        // 2. 按文件名排序（因为你的格式是 YYYY-MM-DD-N-xxx，字典序就是时间序）
        archives.sort();

        // 3. 如果数量超过上限，删除最老的
        if archives.len() > keep_count {
            let to_delete = archives.len() - keep_count;
            for path in archives.iter().take(to_delete) {
                if let Err(e) = std::fs::remove_file(path) {
                    println!("[WARN] 无法删除旧日志归档 {:?}: {}", path, e);
                } else {
                    println!("Cleaned up old archive: {:?}", path);
                }
            }
        }
    }

    

    #[track_caller]
    pub fn write_log(&self, level: LEVEL, class: &str, message: &[&str]) {
        if message.is_empty() {return;}

        let max_char_len = LOG_ITEM_MAX_CHAR_LENGTH as usize;
        let out_message = replace_template_placeholders_manual(message[0], &message[1..]);
        
        let log_length = out_message.chars().count();
        let out_message = if log_length > max_char_len {
            out_message.chars().take(max_char_len).collect::<String>()
        } else {
            out_message
        };
        let caller = Location::caller();
        let out_text = format!(
            "[{}][{}][{}:{}][{}] {}",
             get_now_time(),
             LEVEL::get_level_str(level),
             caller.file(),
             caller.line(),
             class,
             out_message,
        );
        
        println!("{}", out_text);
        if log_length > max_char_len {
            println!(
                "[{}][WARN][{}:{}][LogSystem][Log Long] ----- The log is too long and has been forced to truncate. The maximum value is {}, the length of this log {}. Note: Try to dump the content you want to print instead of exporting it to the log."
                , get_now_time()
                , caller.file()
                , caller.line()
                , max_char_len
                , log_length
            )
        }

        if LEVEL::DEBUG != level {
            let mut writer_latest = self.latest_writer.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(e) = writeln!(writer_latest, "{}", out_text) {
                eprintln!(
                    "[{}][ERROR][{}:{}][LogSystem][Log Write] ----- An error occurred while trying to write to latest.log: {}"
                    , get_now_time()
                    , file!()
                    , line!()
                    , e
                )
            }
            let _ = writer_latest.flush();
        }

        let mut writer_debug = self.debug_writer.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = writeln!(writer_debug, "{}", out_text) {
            eprintln!(
                "[{}][ERROR][{}:{}][LogSystem][Log Write] ----- An error occurred while trying to write to debug.log: {}"
                , get_now_time()
                , file!()
                , line!()
                , e
            )
        }
        let _ = writer_debug.flush(); // 刷新

    }
}

////////////////////////////////////////////
/// -----------------------------------  ///
////////////////////////////////////////////

/// 替换字符串中的 `{}` 和 `%s`
/// AICG
/// 参考代码
pub fn replace_template_placeholders_manual(template: &str, args: &[&str]) -> String {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    let mut arg_index = 0;

    while let Some(c) = chars.next() {
        if c == '{' {
            if let Some(&'}') = chars.peek() {
                chars.next(); // 消耗掉 '}'
                if arg_index < args.len() {
                    result.push_str(args[arg_index]);
                    arg_index += 1;
                } else {
                    result.push_str("{}"); // 参数不够，原样保留
                }
            } else {
                result.push(c); // 不是 {}，原样保留
            }
        } else if c == '%' {
            if let Some(&'s') = chars.peek() {
                chars.next(); // 消耗掉 's'
                if arg_index < args.len() {
                    result.push_str(args[arg_index]);
                    arg_index += 1;
                } else {
                    result.push_str("%s"); // 参数不够，原样保留
                }
            } else {
                result.push(c);
            }
        } else {
            result.push(c);
        }
    }
    result
}


/// 入口函数
pub fn init_log_object(dir: &PathBuf) -> () {
    let log = LogFileObject::init(dir);
    let _ = LOGGER.set(log);
    // *slot_log_object().lock().unwrap_or_else(|e| e.into_inner()) = Some(log);
    ()
}

// fn slot_log_object() -> &'static Mutex<Option<LogFileObject>> {
//     LOGGER.get_or_init(|| Mutex::new(None))
// }

////////////////////////////////////////////
/// -----------------------------------  ///
////////////////////////////////////////////

// 创建一个全局唯一并且还互斥还不会是空的元素
pub static LOGGER: OnceLock<LogFileObject> = OnceLock::new();

// 通用逻辑
#[macro_export]
macro_rules! logger_impl {
    // $level: 日志级别, $target: 标签, $fmt: 格式字符串, $($arg:expr),* : 可变
    ($level:expr, $target:expr, $fmt:expr $(, $arg:expr)*) => {
        if let Some(logger) = $crate::logger::LOGGER.get() {
            logger.write_log($level, $target, &[$fmt $(, $arg)*]);
        }
    };
}

// 对外的具体宏，按级别封装
#[macro_export]
macro_rules! logger_debug {
    ($target:expr, $fmt:expr $(, $arg:expr)*) => {
        $crate::logger_impl!($crate::logger::LEVEL::DEBUG, $target, $fmt $(, $arg)*)
    };
}

#[macro_export]
macro_rules! logger_info {
    ($target:expr, $fmt:expr $(, $arg:expr)*) => {
        $crate::logger_impl!($crate::logger::LEVEL::INFO, $target, $fmt $(, $arg)*)
    };
}

#[macro_export]
macro_rules! logger_warn {
    ($target:expr, $fmt:expr $(, $arg:expr)*) => {
        $crate::logger_impl!($crate::logger::LEVEL::WARN, $target, $fmt $(, $arg)*)
    };
}

#[macro_export]
macro_rules! logger_error {
    ($target:expr, $fmt:expr $(, $arg:expr)*) => {
        $crate::logger_impl!($crate::logger::LEVEL::ERROR, $target, $fmt $(, $arg)*)
    };
}

#[macro_export]
macro_rules! logger_fatal {
    ($target:expr, $fmt:expr $(, $arg:expr)*) => {
        $crate::logger_impl!($crate::logger::LEVEL::FATAL, $target, $fmt $(, $arg)*)
    };
}

#[cfg(test)]
mod test {
    use tauri::menu::NativeIcon::Path;
    use crate::logger::{LogFileObject, init_log_object}; // 我不认为 Never Use，因为删了报错

    #[test]
    #[cfg(target_os = "windows")]
    fn sdsdsdsd() {
        use rfd::MessageDialog;
        use std::path::PathBuf;
        use crate::logger::LOGGER;
        use crate::logger::LEVEL;

        init_log_object(&PathBuf::from("H:\\"));

        if let Some(logger) = LOGGER.get() {
            logger.write_log(LEVEL::INFO, "TEST", &["测试"]);
        }
        
        
        let _ = MessageDialog::new()
            .set_title("Log System Error")
            .set_description(format!("Log system initialization: archived old log files.\nYou can still choose to continue running the program, but the archive log is lost.\n\n {}", "sss"))
            .set_level(rfd::MessageLevel::Error)
            .show();
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
