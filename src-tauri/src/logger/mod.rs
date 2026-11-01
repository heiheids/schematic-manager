use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{PathBuf};
use std::sync::{Mutex, OnceLock};
use chrono::{Local, NaiveDate};
use std::panic::Location;
use rfd::MessageDialog;
#[cfg(target_os = "windows")]
use std::os::windows::fs::OpenOptionsExt;



const LOG_FILE_NAME: &str = "_latest.log";
const LOG_DEBUG_FILE_NAME: &str = "debug.log";

/// 单条日志最长字符长度
const LOG_ITEM_MAX_CHAR_LENGTH: usize = 0xFFFF;
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

/// 归档文件名：`<日期>-<序号>-<类型>.log.gz`。
///
/// 序号在同一类归档里全局递增（跨天继续累加），因此文件名本身就能确定新旧。
fn archive_file_name(date: &str, index: u64, base_name: &str) -> String {
    format!("{}-{}-{}.log.gz", date, index, base_name)
}

/// 解析本类归档的文件名，取出日期与序号；不是本类归档时返回 `None`。
///
/// 日期必须能被解析成 `YYYY-MM-DD`：清理会真的删文件，不能只凭
/// 「以 `-latest.log.gz` 结尾」就把目录里无关的文件当成归档删掉。
fn parse_archive_name(name: &str, base_name: &str) -> Option<(NaiveDate, u64)> {
    let stem = name.strip_suffix(".log.gz")?;
    let stem = stem.strip_suffix(&format!("-{}", base_name))?;
    let (date, index) = stem.rsplit_once('-')?;
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    Some((date, index.parse().ok()?))
}

/// 目录里已有本类归档的最大序号；没有归档时返回 0。
fn get_max_archive_index(base_name: &str, dir: &PathBuf) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            parse_archive_name(&name, base_name).map(|(_, index)| index)
        })
        .max()
        .unwrap_or(0)
}

/// 下一个归档路径：序号取已有归档的最大值加一。
///
/// 同名已存在时继续往后找：覆盖会把更早的一份日志吃掉。
fn get_next_archive_name(base_name: &str, dir: &PathBuf) -> PathBuf {
    let mut n = get_max_archive_index(base_name, dir) + 1;
    loop {
        let candidate = dir.join(archive_file_name(&get_now_date(), n, base_name));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

pub struct LogFileObject {
    log_dir: PathBuf,
    latest_writer: Mutex<BufWriter<File>>,
    debug_writer: Mutex<BufWriter<File>>,
}

impl LogFileObject {
    /// 初始化新的日志
    pub fn init(run_path: &PathBuf) -> Self {
        fn show_error(text: &str, title: &str) {
            println!("[{}][ERROR][::logger] {} - {}", get_now_time(), title, text);
            #[cfg(target_os = "windows")] {
                use windows::core::{HSTRING, PCWSTR};
                use windows::Win32::Foundation::{BOOL, HWND};
                use windows::Win32::UI::Controls::{
                    TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0, TASKDIALOGCONFIG_1,
                    TASKDIALOG_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION, TD_ERROR_ICON,
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
                    // dwCommonButtons: TDCBF_OK_BUTTON,
                    pszWindowTitle:     PCWSTR(title.as_ptr()),
                    pszMainInstruction: PCWSTR(instruction.as_ptr()),
                    pszContent:         PCWSTR(content.as_ptr()),
                    cButtons: buttons.len() as u32,
                    pButtons: buttons.as_ptr(),
                    Anonymous1: TASKDIALOGCONFIG_0 { pszMainIcon: TD_ERROR_ICON }, // ✅ 字段名修正
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
            
            }
            
            #[cfg(not(target_os = "windows"))]
            {
                let _ = MessageDialog::new()
                .set_title(title)
                .set_description(text)
                .set_level(rfd::MessageLevel::Error)
                .show();
            }
        }

        let log_dir = run_path.clone();

        // 线保证日志目录一定存在
        if let Err(e) = std::fs::create_dir_all(&log_dir) {
            show_error(&format!("Failed to create log directory: \n{}", e), "Log System Error");
            std::process::exit(0x6);
        }

        // 然后再去吧日志滚走
        match LogFileObject::roll_old_log_file(&log_dir) {
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

        LogFileObject::clean_old_archives(&log_dir, "latest", LOG_ARCHIVES_MAX_ITEM.into());
        LogFileObject::clean_old_archives(&log_dir, "debug", LOG_ARCHIVES_MAX_ITEM.into());

        let mut file_opts = OpenOptions::new();
        file_opts.create(true).append(true);

        #[cfg(target_os = "windows")]
        {file_opts.share_mode(0x1 | 0x2);}

        // 再去创建文件
        let latest_file = match file_opts.open(&log_dir.join(LOG_FILE_NAME)) {
            Ok(file) => file,
            Err(e) => {
                show_error(&format!("The latest.log initialization failed and the log file could not be saved: \n{}", e), "Log System Error");
                std::process::exit(0x6);
            },
        };
        let debug_file  = match file_opts.open(&log_dir.join(LOG_DEBUG_FILE_NAME)) {
            Ok(file) => file,
            Err(e) => {
                show_error(&format!("The debug.log initialization failed and the log file could not be saved: \n{}", e), "Log System Error");
                std::process::exit(0x6);
            },
        };

        // 然后再把自己搞回去
        let out_self = Self {
            log_dir: run_path.clone(),
            latest_writer: Mutex::new(BufWriter::new(latest_file)),
            debug_writer: Mutex::new(BufWriter::new(debug_file)),
        };

        
        return out_self;
    }

    /// 将上次的日志压缩归档
    fn roll_old_log_file(dir: &PathBuf) -> Result<(), std::io::Error> {
        fn roll_file(name: &str, base_name: &str, dir: &PathBuf) -> Result<(), std::io::Error> {
            let file_current = dir.join(name);
            if !file_current.exists() { return Ok(()); } // 忽略 ~~404~~ NotFound
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

    /// 清理旧的归档日志，只保留最新的 `keep_count` 个
    fn clean_old_archives(dir: &PathBuf, base_name: &str, keep_count: usize) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };

        // 1. 收集所有本类归档的日期、序号与路径
        let mut archives: Vec<(NaiveDate, u64, PathBuf)> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let (date, index) = parse_archive_name(&name, base_name)?;
                Some((date, index, entry.path()))
            })
            .collect();

        // 2. 按「日期、序号」排序就是从旧到新。
        // 序号是全局递增的，但只按文件名字典序排会把 -10 排到 -2 前面，
        // 结果删掉的反而是较新的归档。
        archives.sort_by_key(|(date, index, _)| (*date, *index));

        // 3. 如果数量超过上限，删除最老的
        if archives.len() > keep_count {
            let to_delete = archives.len() - keep_count;
            for (_, _, path) in archives.iter().take(to_delete) {
                if let Err(e) = std::fs::remove_file(path) {
                    println!("[WARN] 无法删除旧日志归档 {}: {}", path.display(), e);
                }
            }
        }
    }

    #[track_caller]
    pub fn write_log(&self, level: LEVEL, class: &str, message: &str) {
        Self::write_log_caller(&self, level, class, message, Location::caller());
    }

    #[track_caller]
    pub fn write_log_caller(&self, level: LEVEL, class: &str, message: &str, caller: &'static Location<'static>) {
        if message.is_empty() {return;}

        let out_message = message;
        let time = get_now_time();
        
        let log_length = out_message.chars().count();
        let out_message = if log_length > LOG_ITEM_MAX_CHAR_LENGTH {
            out_message.chars().take(LOG_ITEM_MAX_CHAR_LENGTH).collect::<String>()
        } else {
            String::from(out_message)
        };
        let mut out_text = format!(
            "[{}][{}][{}:{}][{}] {}",
             time,
             LEVEL::get_level_str(level),
             caller.file(),
             caller.line(),
             class,
             out_message,
        );
        
        println!("{}", out_text);
        if log_length > LOG_ITEM_MAX_CHAR_LENGTH {
            let log = format!(
                "[{}][WARN][{}:{}][LogSystem][Log Long] ----- The log is too long and has been forced to truncate. The maximum value is {}, the length of this log {}. Note: Try to dump the content you want to print instead of exporting it to the log."
                , time
                , caller.file()
                , caller.line()
                , LOG_ITEM_MAX_CHAR_LENGTH
                , log_length
            );
            println!("{}", log);
            out_text = out_text + "\n" + &log
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

    #[track_caller]
    pub fn debug(&self, class: &str, message: &str) {
        Self::write_log_caller(&self, LEVEL::DEBUG, class, message, Location::caller());
    }

    #[track_caller]
    pub fn info(&self, class: &str, message: &str) {
        Self::write_log_caller(&self, LEVEL::INFO, class, message, Location::caller());
    }

    #[track_caller]
    pub fn warn(&self, class: &str, message: &str) {
        Self::write_log_caller(&self, LEVEL::WARN, class, message, Location::caller());
    }

    #[track_caller]
    pub fn error(&self, class: &str, message: &str) {
        Self::write_log_caller(&self, LEVEL::ERROR, class, message, Location::caller());
    }

    #[track_caller]
    pub fn fatal(&self, class: &str, message: &str) {
        Self::write_log_caller(&self, LEVEL::FATAL, class, message, Location::caller());
    }
}

/////////////////////////////////////////
// ----------------------------------- //
/////////////////////////////////////////

/// 替换字符串中的 `{}` 和 `%s`
/// AICG
/// 参考代码
/// 保留代码，用作 `%s` 的处理上，并且不应当替代 `format` 宏
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
/// 入口
pub fn init_log_object(dir: &PathBuf) -> () {
    let _ = LOGGER.get_or_init(|| LogFileObject::init(dir));
    ()
}

////////////////////////////////////////////
/// -----------------------------------  ///
////////////////////////////////////////////

// 创建一个全局唯一并且还互斥还不会是空的元素
pub static LOGGER: OnceLock<LogFileObject> = OnceLock::new();

// 通用逻辑
#[macro_export]
macro_rules! logger_impl {
    // $level: 日志级别, $target: 标签, $fmt: 格式字符串, $($arg:expr),* : 可变
    ($level:expr, $target:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {
        if let Some(logger) = $crate::logger::LOGGER.get() {
            logger.write_log($level, $target, &format!($fmt $(, $arg)*));
        }
    };
}

#[macro_export]
macro_rules! logger_caller_impl {
    // $level: 日志级别, $target: 标签, $fmt: 格式字符串, $($arg:expr),* : 可变
    ($level:expr, $target:expr, $caller:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {
        if let Some(logger) = $crate::logger::LOGGER.get() {
            logger.write_log_caller($level, $target, &format!($fmt $(, $arg)*), $caller);
        }
    };
}

// 对外的具体宏，按级别封装
#[macro_export]
macro_rules! logger_debug {
    ($target:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {
        $crate::logger_impl!($crate::logger::LEVEL::DEBUG, $target, $fmt $(, $arg)*)
    };
}

#[macro_export]
macro_rules! logger_info {
    ($target:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {
        $crate::logger_impl!($crate::logger::LEVEL::INFO, $target, $fmt $(, $arg)*)
    };
}

#[macro_export]
macro_rules! logger_warn {
    ($target:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {
        $crate::logger_impl!($crate::logger::LEVEL::WARN, $target, $fmt $(, $arg)*)
    };
}

#[macro_export]
macro_rules! logger_error {
    ($target:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {
        $crate::logger_impl!($crate::logger::LEVEL::ERROR, $target, $fmt $(, $arg)*)
    };
}

#[macro_export]
macro_rules! logger_fatal {
    ($target:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {
        $crate::logger_impl!($crate::logger::LEVEL::FATAL, $target, $fmt $(, $arg)*)
    };
}

#[cfg(test)]
#[path = "../tests/logger/mod.rs"]
mod tests;

