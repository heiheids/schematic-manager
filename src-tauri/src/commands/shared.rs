//! 命令层共用：默认扫描深度、备份目录、方块库与实体贴图库的进程内缓存。

use crate::commands::dto::DirectoryNode;
use crate::error::AppResult;
use std::path::{Path, PathBuf};

/// 递归收集投影文件的默认层数上限。
///
/// 文件列表与重复筛查共用这一个值：两处一旦不同，筛查就会与用户看到的
/// 列表脱节（列表里有、筛查里没有），而那正是「查不出重复」的来源。
pub(crate) const SCAN_MAX_DEPTH: u32 = 6;

/// 程序所在目录。备份目录以它为根，与旧版行为一致。
pub(crate) fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 跳过的目录统计。路径只保留前若干条，避免异常目录结构下无限增长。
#[derive(Default)]
pub(crate) struct SkipReport {
    pub(crate) count: u32,
    pub(crate) paths: Vec<String>,
}

const MAX_SKIP_PATHS: usize = 20;

impl SkipReport {
    fn record(&mut self, dir: &Path) {
        self.count = self.count.saturating_add(1);
        if self.paths.len() < MAX_SKIP_PATHS {
            self.paths.push(dir.to_string_lossy().into_owned());
        }
    }
}

pub(crate) fn collect_litematics(dir: &Path, depth: u32, out: &mut Vec<PathBuf>, skipped: &mut SkipReport) {
    if depth == 0 {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => {
            skipped.record(dir);
            return;
        }
    };
    for e in entries {
        let entry = match e {
            Ok(v) => v,
            Err(_) => {
                skipped.record(dir);
                continue;
            }
        };
        let p = entry.path();
        let ft = match entry.file_type() {
            Ok(v) => v,
            Err(_) => {
                skipped.record(&p);
                continue;
            }
        };
        if ft.is_dir() {
            collect_litematics(&p, depth - 1, out, skipped);
        } else if ft.is_file()
            && p.extension()
                .map(|x| x.eq_ignore_ascii_case("litematic"))
                .unwrap_or(false)
        {
            out.push(p);
        }
    }
}

/// 在一次目录遍历中同时收集投影文件与目录树，避免两条视图各自读取目录后
/// 因文件系统变化或权限错误而得到不一致的内容。
pub(crate) fn collect_directory(
    dir: &Path,
    root: &Path,
    depth: u32,
    out: &mut Vec<PathBuf>,
    skipped: &mut SkipReport,
) -> DirectoryNode {
    let mut node = DirectoryNode {
        path: dir.to_string_lossy().into_owned(),
        name: dir
            .file_name()
            .unwrap_or_else(|| dir.as_os_str())
            .to_string_lossy()
            .into_owned(),
        relative_path: dir
            .strip_prefix(root)
            .unwrap_or(dir)
            .to_string_lossy()
            .into_owned(),
        file_paths: Vec::new(),
        children: Vec::new(),
        truncated: depth == 0,
    };
    if depth == 0 {
        return node;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => {
            skipped.record(dir);
            return node;
        }
    };
    for e in entries {
        let entry = match e {
            Ok(v) => v,
            Err(_) => {
                skipped.record(dir);
                continue;
            }
        };
        let p = entry.path();
        let ft = match entry.file_type() {
            Ok(v) => v,
            Err(_) => {
                skipped.record(&p);
                continue;
            }
        };
        if ft.is_dir() {
            node.children
                .push(collect_directory(&p, root, depth - 1, out, skipped));
        } else if ft.is_file()
            && p.extension()
                .map(|x| x.eq_ignore_ascii_case("litematic"))
                .unwrap_or(false)
        {
            node.file_paths.push(p.to_string_lossy().into_owned());
            out.push(p);
        }
    }
    node.file_paths.sort();
    node.children.sort_by(|a, b| a.path.cmp(&b.path));
    node
}

/// 已载入的实体贴图库。
///
/// 解析并解压像素池是有成本的，而导出/预览会反复查询，因此做进程内缓存。
/// 用 `RwLock<Option<..>>` 而不是 `OnceLock`：用户在「版本库」里补充版本后，
/// 实体贴图库的内容必须随之变化，`OnceLock` 一旦写入就改不了。
static ENTITY_DB: std::sync::OnceLock<
    std::sync::RwLock<Option<std::sync::Arc<crate::block::entity_db::EntityDb>>>,
> = std::sync::OnceLock::new();

fn entity_db_cache(
) -> &'static std::sync::RwLock<Option<std::sync::Arc<crate::block::entity_db::EntityDb>>> {
    ENTITY_DB.get_or_init(|| std::sync::RwLock::new(None))
}

/// 让实体贴图库缓存失效。补充或替换版本后必须调用，否则改动要等下次启动才生效。
pub(crate) fn invalidate_entity_db() {
    // 锁中毒说明此前有线程在持锁时 panic；缓存只是可重建的派生数据，
    // 取回内部值继续用即可，不必让整个程序跟着崩
    let mut g = entity_db_cache().write().unwrap_or_else(|e| e.into_inner());
    *g = None;
}

/// 取实体贴图库：app 数据目录下的基础库文件。
///
/// 库文件不存在（还没添加任何版本）返回 None 而不是错误——
/// 首次运行就是这个状态，由前端引导用户去「版本库」里补。
pub(crate) fn open_entity_db(app: &tauri::AppHandle) -> Option<std::sync::Arc<crate::block::entity_db::EntityDb>> {
    {
        let g = entity_db_cache().read().unwrap_or_else(|e| e.into_inner());
        if let Some(db) = g.as_ref() {
            return Some(std::sync::Arc::clone(db));
        }
    }
    let path = crate::pack::base::base_entities_path(app).ok()?;
    if !path.is_file() {
        return None;
    }
    let db = match crate::block::entity_db::EntityDb::load(&path) {
        Ok(db) => std::sync::Arc::new(db),
        Err(e) => {
            // 不静默：贴图库损坏会让实体渲染全部没有贴图，
            // 必须留下线索，否则用户只会看到一堆白模
            crate::app::run_log::error(&format!("载入实体贴图库失败 {}：{e}", path.display()));
            return None;
        }
    };
    let mut g = entity_db_cache().write().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = Some(std::sync::Arc::clone(&db));
    }
    g.as_ref().map(std::sync::Arc::clone)
}

/// 已载入的方块库。
pub struct LoadedDb {
    pub db: std::sync::Arc<crate::block::db::BlockDb>,
}

/// 进程内缓存的方块库。
///
/// 方块库解压后有数 MB、含数万条方块定义，而方块检索是随输入逐键
/// 触发的；每次重新读取与解析会让输入明显卡顿。基础库在 app 数据目录下
/// 按版本合成，载入一次即可长期复用，直到用户改动基础库。
///
/// 用 `RwLock<Option<..>>` 而不是 `OnceLock`：基础库补齐或删除版本后，
/// 方块库的内容必须随之变化。`OnceLock` 一旦写入就再也改不了，会让
/// 「刚补的版本要重启程序才生效」——而用户没有任何理由知道这一点。
static BLOCK_DB: std::sync::OnceLock<std::sync::RwLock<Option<std::sync::Arc<LoadedDb>>>> =
    std::sync::OnceLock::new();

fn db_cache() -> &'static std::sync::RwLock<Option<std::sync::Arc<LoadedDb>>> {
    BLOCK_DB.get_or_init(|| std::sync::RwLock::new(None))
}

/// 让方块库缓存失效。
///
/// 任何会改变基础库内容的操作之后都必须调用，否则改动要等
/// 下次启动才生效。
pub(crate) fn invalidate_db() {
    // 锁中毒说明此前有线程在持锁时 panic；缓存只是可重建的派生数据，
    // 取回内部值继续用即可，不必让整个程序跟着崩
    let mut g = db_cache().write().unwrap_or_else(|e| e.into_inner());
    *g = None;
}

/// 取方块库：app 数据目录下按版本合成的基础库。
///
/// 基础库为空并不是错误：首次运行还没添加任何版本时就是这个状态，
/// 由前端引导用户去「版本库」里补。基础库里损坏的单版本文件只记入
/// 日志，不中断整库——损坏的那一个版本会在「版本库」里显示为未就绪，
/// 用户重新构建即可；具体原因写进日志，便于排查「明明建过却用不上」。
pub(crate) fn open_db(app: &tauri::AppHandle) -> AppResult<std::sync::Arc<LoadedDb>> {
    {
        let g = db_cache().read().unwrap_or_else(|e| e.into_inner());
        if let Some(db) = g.as_ref() {
            return Ok(std::sync::Arc::clone(db));
        }
    }

    let (db, failures) =
        crate::pack::base::load_base_block_db(&crate::pack::base::base_blocks_dir(app)?)?;
    for (file, reason) in &failures {
        crate::app::run_log::error(&format!("基础库的版本包 {file} 未能载入：{reason}"));
    }

    let loaded = std::sync::Arc::new(LoadedDb { db: std::sync::Arc::new(db) });
    let mut g = db_cache().write().unwrap_or_else(|e| e.into_inner());
    // 并发下可能被别的线程抢先设置，此时沿用已设置的即可
    if g.is_none() {
        *g = Some(std::sync::Arc::clone(&loaded));
    }
    Ok(match g.as_ref() {
        Some(existing) => std::sync::Arc::clone(existing),
        None => loaded,
    })
}
