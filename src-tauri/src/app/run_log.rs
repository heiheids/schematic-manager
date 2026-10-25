//! 运行日志：按等级记录、按大小与启动次数滚动归档。
//!
//! 文件放在应用数据目录的 `logs/latest.log`；滚动时把上一份压成
//! `YYYY-MM-dd-<滚动次数>.log.gz` 留在同目录，因此归档名里的次数是全局递增的，
//! 同一天滚两次也不会互相覆盖。
//!
//! 为什么等级是**写入门控**而不是显示过滤：debug 等级要记网络请求与响应，
//! 报文动辄几十 KB 到几 MB，无条件写盘会让 5MB 的上限形同虚设，也会让用户
//! 为日常使用付出无谓的磁盘写。界面看到的因此就是文件里真实有的内容。
//!
//! 为什么归档名里的日期由界面给：Rust 侧一律只存纳秒时间戳（备份文件名与
//! 本模块的日志行都是如此），本地时间由界面来排版，
//! 这样后端不必为了一个文件名引入时区数据库依赖。
//!
//! 启动滚动为什么要等界面给日期：滚动必须发生在「本次会话的日志」写入之前，
//! 否则本次启动的几行会被误归档。界面挂载后第一时间调用 `configure`，
//! 在此之前写入的行先留在内存缓冲里，滚动完成后按顺序落盘。

use crate::error::AppResult;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 当前日志文件名
const LOG_NAME: &str = "latest.log";

/// 超过这个大小就滚动一次
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// 启动滚动之前最多缓冲多少行。
///
/// 界面挂载（几十毫秒）之前正常只会写进个位数行；给到 500 行是为了界面
/// 起不来时仍不丢失关键诊断，写满就放弃启动滚动、改为直接续写现有文件
/// （没有本地日期时宁可少滚动一次，也不编造一个可能是错日期的归档名）。
const MAX_PENDING_LINES: usize = 500;

/// 写进日志的报文正文上限。一条响应可能是几十 MB 的二进制，日志不该被它撑爆。
const MAX_BODY_BYTES: usize = 8 * 1024;

/// 日志等级。数值越大越详细，写入时按「当前等级 >= 该行等级」判断。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
}

impl Level {
    /// 解析等级名。大小写不敏感：日志行里写的是 `ERROR`，界面传来的是 `error`。
    pub fn parse(name: &str) -> Option<Level> {
        match name.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Level::Error),
            "warn" => Some(Level::Warn),
            "info" => Some(Level::Info),
            "debug" => Some(Level::Debug),
            _ => None,
        }
    }

    /// 写进日志行第二列的等级标记
    pub fn token(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
        }
    }
}

/// 纳秒时间戳。与备份文件名的取法一致：纳秒精度足够区分同一毫秒内的多行，
/// 而界面按本地时间把它格式化成可读日期。
fn timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}

/// 把一条说明压成一行。
///
/// 日志按行解析，说明里若带换行，一条记录会被读成多条、且内容看起来像
/// 独立的日志；制表符则会让第二列之后的字段错位。
fn escape(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    for c in msg.chars() {
        match c {
            '\r' => {}
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// 报文正文写进日志前的截断。
///
/// 只留前 `MAX_BODY_BYTES` 字节，并在末尾如实标注被截断——静默截断会让
/// 读日志的人以为响应本来就这么短。截断按字符边界切，避免把 UTF-8 截成乱码。
pub fn clip_body(body: &str) -> String {
    if body.len() <= MAX_BODY_BYTES {
        return body.to_string();
    }
    let mut cut = MAX_BODY_BYTES;
    while cut > 0 && !body.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…（正文共 {} 字节，已截断）", &body[..cut], body.len())
}

/// 一份日志文件。做成结构体而不是直接全局状态，是为了让滚动、缓冲这些
/// 逻辑能对着临时目录单独验证（全局单例见文件末尾的便捷函数）。
pub struct RunLog {
    dir: PathBuf,
    level: Level,
    /// 最近一次已知的本地日期，归档命名用
    date: String,
    /// 待滚动期间缓冲的行；`None` 表示启动滚动已经处理完（或不需要）
    pending: Option<Vec<String>>,
}

impl RunLog {
    /// 在 `dir` 下开一份日志。
    ///
    /// 已有非空 `latest.log` 时记下「等界面给出本地日期后再滚动」：滚动要
    /// 归档的是上一份日志，必须发生在本次会话写入之前。
    pub fn open(dir: &Path) -> AppResult<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(LOG_NAME);
        let has_content = std::fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false);
        Ok(Self {
            dir: dir.to_path_buf(),
            level: Level::Info,
            date: String::new(),
            // 空文件（例如用户刚清空过）没什么可归档的，不必产生一个空归档
            pending: if has_content { Some(Vec::new()) } else { None },
        })
    }

    pub fn level(&self) -> Level {
        self.level
    }

    /// 只改等级，不动日期。启动时先按界面存下的等级记录，让 debug 用户
    /// 从第一行起就拿到完整日志。
    pub fn set_level(&mut self, level: Level) {
        self.level = level;
    }

    /// 界面给出等级与本地日期：落实启动滚动，再把缓冲的行落盘。
    ///
    /// 界面每次改等级、每次打开日志窗口都会调用它，因此日期跟着刷新，
    /// 长时间使用时按大小滚动也能拿到当天的日期。
    ///
    /// 返回等级或日期**是否真的变了**：界面只是重复同步同一份设置时不值得
    /// 在日志里留一行，否则每开一次日志窗口就多一条噪音。
    pub fn configure(&mut self, level: Level, local_date: &str) -> AppResult<bool> {
        let mut changed = self.level != level;
        self.level = level;
        let date = local_date.trim();
        if !date.is_empty() && self.date != date {
            self.date = date.to_string();
            changed = true;
        }
        self.finish_pending_roll()?;
        Ok(changed)
    }

    /// 待滚动期间：先缓冲，缓冲写满就放弃滚动、直接续写（见 `MAX_PENDING_LINES`）。
    fn finish_pending_roll(&mut self) -> AppResult<()> {
        let Some(buffered) = self.pending.take() else {
            return Ok(());
        };
        if self.date.is_empty() {
            // 拿不到本地日期就不滚动：宁可少一次归档，也不编一个可能错日期的名字。
            // 缓冲的行按原顺序接着写进现有文件，内容不丢。
            for line in buffered {
                self.append_line(&line)?;
            }
            return Ok(());
        }
        let archived = self.roll()?;
        for line in buffered {
            self.append_line(&line)?;
        }
        let name = archived
            .file_name()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.write(
            Level::Info,
            &format!("日志已滚动：上一份归档为 {name}"),
        );
        Ok(())
    }

    /// 写一行。低于当前等级的整行丢弃。
    pub fn write(&mut self, level: Level, msg: &str) {
        if level > self.level {
            return;
        }
        let line = format!(
            "{}\t{}\t{}",
            timestamp(),
            level.token(),
            escape(msg.trim_end())
        );
        if let Some(buffered) = self.pending.as_mut() {
            if buffered.len() < MAX_PENDING_LINES {
                buffered.push(line);
                return;
            }
            // 缓冲写满说明界面迟迟没起来。此时没有本地日期可用，
            // 直接落盘续写现有文件，至少保证诊断信息不丢。
            let flushed = std::mem::take(buffered);
            self.pending = None;
            for old in flushed {
                self.append_or_report(&old);
            }
        }
        self.append_or_report(&line);
        self.rotate_if_too_big();
    }

    fn append_line(&self, line: &str) -> AppResult<()> {
        let path = self.dir.join(LOG_NAME);
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        f.write_all(line.as_bytes())?;
        f.write_all(b"\n")?;
        Ok(())
    }

    /// 写一行并在失败时报到标准错误。
    ///
    /// 写日志失败不能打断主流程（调用方都在业务路径上，日志只是副产物），
    /// 但也不能吞掉：这里必须用 `eprintln!` 而不是 `run_log::error`——
    /// 写日志时已经持有全局锁，回调入口会自我死锁。
    fn append_or_report(&self, line: &str) {
        if let Err(e) = self.append_line(line) {
            eprintln!("[运行日志] 写盘失败 {}：{e}", self.path().display());
        }
    }

    /// 文件超过上限时滚动一次。滚动失败只影响归档，不该反过来影响主流程，
    /// 因此这里把原因写进日志本身（写不进去时报到标准错误，见 `append_or_report`）。
    fn rotate_if_too_big(&mut self) {
        let path = self.dir.join(LOG_NAME);
        let too_big = std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false);
        if !too_big {
            return;
        }
        if self.date.is_empty() {
            return;
        }
        match self.roll() {
            Ok(archived) => {
                let name = archived
                    .file_name()
                    .map(|v| v.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.append_or_report(&format!(
                    "{}\t{}\t日志超过 {}MB，已滚动为 {}",
                    timestamp(),
                    Level::Info.token(),
                    MAX_BYTES / 1024 / 1024,
                    name
                ));
            }
            Err(e) => {
                self.append_or_report(&format!(
                    "{}\t{}\t滚动日志失败：{e}",
                    timestamp(),
                    Level::Error.token()
                ));
            }
        }
    }

    /// 把当前日志压成归档并清空当前文件，返回归档路径。
    fn roll(&self) -> AppResult<PathBuf> {
        let current = self.dir.join(LOG_NAME);
        let data = std::fs::read(&current)?;
        let dest = self.next_archive_path();
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&data)?;
        let packed = enc.finish()?;
        // 先落归档再清空当前文件：反过来的话，压缩失败就把日志内容弄丢了
        std::fs::write(&dest, packed)?;
        std::fs::write(&current, b"")?;
        Ok(dest)
    }

    /// 下一个归档名：`<日期>-<次数>.log.gz`，次数取已有归档的最大值 +1。
    ///
    /// 同名已存在时继续往后找：名字冲突时覆盖会把更早的一份日志吃掉。
    fn next_archive_path(&self) -> PathBuf {
        let mut n = self.max_archive_index() + 1;
        loop {
            let candidate = self.dir.join(format!("{}-{n}.log.gz", self.date));
            if !candidate.exists() {
                return candidate;
            }
            n += 1;
        }
    }

    /// 目录里已有归档的最大次数；没有归档时返回 0。
    fn max_archive_index(&self) -> u64 {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return 0;
        };
        entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let stem = name.strip_suffix(".log.gz")?;
                let (_, index) = stem.rsplit_once('-')?;
                index.parse::<u64>().ok()
            })
            .max()
            .unwrap_or(0)
    }

    /// 读取最近若干行，返回顺序为由新到旧。
    pub fn read(&self, max_lines: usize) -> Vec<String> {
        let path = self.dir.join(LOG_NAME);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            // 文件不存在就是「还没有日志」，不是问题；其它读失败（权限、非 UTF-8）
            // 会让界面显示成「暂无日志」，与真的没有日志分不清，因此报到标准错误
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(e) => {
                eprintln!("[运行日志] 读取 {} 失败：{e}", path.display());
                return Vec::new();
            }
        };
        let mut lines: Vec<String> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.to_string())
            .collect();
        lines.reverse();
        lines.truncate(max_lines);
        lines
    }

    /// 清空日志内容但保留文件，并丢弃待滚动的缓冲。
    ///
    /// 保留文件而不是删掉：日志位置是外部约定，用户很可能正开着那个目录在看，
    /// 让文件凭空消失比留下一个空文件更让人困惑，「打开日志目录」也因此仍能定位。
    pub fn clear(&mut self) -> AppResult<()> {
        self.pending = None;
        let path = self.dir.join(LOG_NAME);
        std::fs::write(&path, b"")?;
        Ok(())
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(LOG_NAME)
    }
}

// ---------------------------------------------------------------- 全局单例

/// 进程内唯一的一份日志。后端各处（命令、网络层）都直接写日志，
/// 不必把 AppHandle 或日志目录一路传下去。
static CURRENT: OnceLock<Mutex<Option<RunLog>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<RunLog>> {
    CURRENT.get_or_init(|| Mutex::new(None))
}

/// 打开全局日志。重复调用会替换掉上一份（热重载与测试里都可能发生）。
pub fn open(dir: &Path) -> AppResult<()> {
    let log = RunLog::open(dir)?;
    *slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(log);
    Ok(())
}

/// 写出日志文件路径；日志还没打开时返回 None。
pub fn path() -> Option<PathBuf> {
    slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|l| l.path())
}

/// 该等级当前会不会被记录。
///
/// 调用方据此避免构造昂贵的说明（网络报文的格式化、正文读取）。
pub fn enabled(level: Level) -> bool {
    slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|l| level <= l.level())
        .unwrap_or(false)
}

/// 启动时按界面存下的等级先设定，让 debug 用户从第一行起就有完整日志。
pub fn set_level(level: Level) {
    if let Some(log) = slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
    {
        log.set_level(level);
    }
}

/// 界面给出等级与本地日期；返回设置是否真的变了（见 `RunLog::configure`）。
pub fn configure(level: Level, local_date: &str) -> AppResult<bool> {
    let mut guard = slot().lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_mut() {
        Some(log) => log.configure(level, local_date),
        // 日志没打开时不报错（这不是用户能处理的问题），但也谈不上「变了」
        None => Ok(false),
    }
}

/// 写一行日志。
///
/// 日志没打开或写盘失败时退到标准错误：日志本身绝不能让主流程失败，
/// 但也不能悄悄丢掉——写盘失败时用户至少能在控制台看到。
pub fn write(level: Level, msg: &str) {
    let mut guard = slot().lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_mut() {
        Some(log) => log.write(level, msg),
        None => eprintln!("[日志未初始化] {}", escape(msg)),
    }
}

pub fn info(msg: &str) {
    write(Level::Info, msg);
}

pub fn warn(msg: &str) {
    write(Level::Warn, msg);
}

pub fn error(msg: &str) {
    write(Level::Error, msg);
}

pub fn debug(msg: &str) {
    write(Level::Debug, msg);
}

/// 读取最近若干行，由新到旧。
pub fn read(max_lines: usize) -> Vec<String> {
    slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|l| l.read(max_lines))
        .unwrap_or_default()
}

/// 清空日志内容，保留文件。
pub fn clear() -> AppResult<()> {
    let mut guard = slot().lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_mut() {
        Some(log) => log.clear(),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "../tests/app/run_log.rs"]
mod tests;
