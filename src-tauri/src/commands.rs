//! Tauri 命令层：前端可调用的全部入口。
//!
//! 约定：命令只做参数校验、调用解析层、把结果转成 DTO，
//! 不在这里承载业务规则，便于解析层独立测试。

use crate::backup;
use crate::blockdb::BlockDb;
use crate::error::{AppError, AppResult};
use crate::dto::{
    AnalysisResult, BackupEntryDto, BaseDbStatusDto, BaseVersionDto, BlockCount, BlockHitDto,
    BlockIconDto, BlockVersionDto, ConvertItemDto, ConvertOutcomeDto, ConvertTargetDto,
    DuplicateGroupDto, DuplicateScanDto, EntityCount, EntityInstanceDto, EntityTextureDto,
    EntityTextureStatusDto, FileOpOutcome, McScanDto, McVersionDto, RenameOutcomeDto,
    ReplaceOutcomeDto, ReplaceRuleDto, SceneDto, SaveOutcome, ScanResult, SchematicSummary,
    SetupOutcomeDto, SetupSource, SetupVersionDto, SetupVersionOutcomeDto, UnreadableFileDto,
    UpdateInfoDto, VersionOptionDto,
};
use crate::litematic::{self, Summary};
use crate::open_url;
use crate::update;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tauri::Manager;

/// 递归收集投影文件的默认层数上限。
///
/// 文件列表与重复筛查共用这一个值：两处一旦不同，筛查就会与用户看到的
/// 列表脱节（列表里有、筛查里没有），而那正是「查不出重复」的来源。
const SCAN_MAX_DEPTH: u32 = 6;

/// 程序所在目录。备份目录以它为根，与旧版行为一致。
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 由内部摘要转换为对前端的契约
fn to_dto(s: Summary) -> SchematicSummary {
    let p = PathBuf::from(&s.path);
    SchematicSummary {
        file_name: p
            .file_name()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_default(),
        dir_name: p
            .parent()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: s.path.clone(),
        file_size: s.file_size,
        error: s.error.clone(),
        format_version: s.format_version,
        data_version: s.data_version,
        mc_version_name: s.mc_version_name.clone(),
        name: s.name.clone(),
        author: s.author.clone(),
        description: s.description.clone(),
        time_created: s.time_created,
        time_modified: s.time_modified,
        enclosing: s.enclosing,
        total_blocks: s.total_blocks,
        total_volume: s.total_volume,
        region_count: s.region_count,
        entity_count: s.entity_count,
        tile_entity_count: s.tile_entity_count,
        pending_ticks: s.pending_ticks,
        has_preview: s.has_preview,
    }
}

/// 只读扫描单个投影文件，不解码方块数据。
#[tauri::command]
pub fn peek_schematic(path: String) -> SchematicSummary {
    to_dto(litematic::Schematic::peek(&path))
}

/// 递归扫描目录下的全部 .litematic 文件。
///
/// depth 限制递归层数，避免误选到盘符根目录时遍历整块磁盘。
/// 不可读的子目录会被跳过并计数，随结果返回给界面提示，
/// 而不是静默给出一个看起来完整、实则缺项的结果。
#[tauri::command]
pub fn scan_litematics(dir: String, max_depth: Option<u32>) -> ScanResult {
    let depth = max_depth.unwrap_or(SCAN_MAX_DEPTH).min(16);
    let mut found: Vec<PathBuf> = Vec::new();
    let mut skipped = SkipReport::default();
    collect_litematics(Path::new(&dir), depth, &mut found, &mut skipped);
    found.sort();

    ScanResult {
        items: found
            .iter()
            .map(|p| {
                let path = p.to_string_lossy().into_owned();
                to_dto(litematic::Schematic::peek(&path))
            })
            .collect(),
        skipped_dirs: skipped.count,
        skipped_paths: skipped.paths,
    }
}

/// 跳过的目录统计。路径只保留前若干条，避免异常目录结构下无限增长。
#[derive(Default)]
struct SkipReport {
    count: u32,
    paths: Vec<String>,
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

fn collect_litematics(dir: &Path, depth: u32, out: &mut Vec<PathBuf>, skipped: &mut SkipReport) {
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

/// 在语言表里查一个键，返回 (中文, 英文)；查不到返回两个空串。
///
/// 语言表按版本保存（`VersionDef::lang_idx` 指向 `db.langs` 的一张表），
/// 因此必须先选对版本，否则查到的名字与投影里的方块对不上。
///
/// **中英文必须分别补齐**，不能一命中就返回：打包时只有一部分版本抓到了
/// 官方中文语言文件（实测 1.13.2~1.21.11 里约一半的表只有英文，
/// 用户那个 1.21.5 对应的表恰恰一条中文都没有）。早期写法在本版本的
/// 条目只有英文时直接返回，于是中文恒为空——这是实现者自己的缺陷，
/// 不是数据缺失。
///
/// 缺的一侧从整个语言表池里找同名的键补上：方块名跨版本相当稳定，
/// 与贴图解析的处理一致（见 `BlockDb::texture_pool_index` 的全局回退）。
pub(crate) fn lookup_lang(db: &BlockDb, version_idx: Option<usize>, key: &str) -> (String, String) {
    let mut zh = String::new();
    let mut en = String::new();

    // 本版本的表优先：它的命名最贴合该版本
    if let Some(vi) = version_idx {
        if let Some(version) = db.versions.get(vi) {
            if let Some(table) = db.langs.get(version.lang_idx as usize) {
                if let Some(e) = table.iter().find(|e| e.block_name == key) {
                    zh = e.zh.clone();
                    en = e.en.clone();
                }
            }
        }
    }
    if !zh.is_empty() && !en.is_empty() {
        return (zh, en);
    }

    // 缺哪一侧就补哪一侧
    for table in &db.langs {
        for e in table.iter().filter(|e| e.block_name == key) {
            if zh.is_empty() && !e.zh.is_empty() {
                zh = e.zh.clone();
            }
            if en.is_empty() && !e.en.is_empty() {
                en = e.en.clone();
            }
            if !zh.is_empty() && !en.is_empty() {
                return (zh, en);
            }
        }
    }
    (zh, en)
}

/// 查方块的中英文名。
///
/// 方块语言表的键有两种写法：老版本直接用方块名（`minecraft:stone`），
/// 新版本用 `block.minecraft.stone`。两种都试，避免因版本差异整列空着。
pub(crate) fn block_display_names(db: &BlockDb, version_idx: Option<usize>, name: &str) -> (String, String) {
    let (zh, en) = lookup_lang(db, version_idx, name);
    if !zh.is_empty() || !en.is_empty() {
        return (zh, en);
    }
    if let Some(short) = name.strip_prefix("minecraft:") {
        let dotted = format!("block.minecraft.{short}");
        let (zh, en) = lookup_lang(db, version_idx, &dotted);
        if !zh.is_empty() || !en.is_empty() {
            return (zh, en);
        }
    }
    (String::new(), String::new())
}

/// 查实体的中英文名。实体 id 形如 `minecraft:zombie`，键是 `entity.minecraft.zombie`。
pub(crate) fn entity_display_names(db: &BlockDb, version_idx: Option<usize>, id: &str) -> (String, String) {
    // 投影里的 id 可能不带命名空间，补上再查
    let full = if id.contains(':') { id.to_string() } else { format!("minecraft:{id}") };
    let dotted = format!("entity.{}", full.replace(':', "."));
    let (zh, en) = lookup_lang(db, version_idx, &dotted);
    if !zh.is_empty() || !en.is_empty() {
        return (zh, en);
    }
    // 有些语言表直接以 id 为键
    lookup_lang(db, version_idx, &full)
}

/// 查方块实体（箱子、告示牌等）的中英文名。
///
/// 它们与实体共用 `entity.*` 命名空间，但方块实体在语言表里也有
/// `block.*` 的别名，因此两种都试。
pub(crate) fn tile_display_names(db: &BlockDb, version_idx: Option<usize>, id: &str) -> (String, String) {
    let full = if id.contains(':') { id.to_string() } else { format!("minecraft:{id}") };
    for key in [
        format!("block.{}", full.replace(':', ".")),
        format!("entity.{}", full.replace(':', ".")),
        full.clone(),
    ] {
        let (zh, en) = lookup_lang(db, version_idx, &key);
        if !zh.is_empty() || !en.is_empty() {
            return (zh, en);
        }
    }
    (String::new(), String::new())
}

/// 完整分析：解码全部方块并统计。
#[tauri::command]
pub fn analyze_schematic(app: tauri::AppHandle, path: String) -> Result<AnalysisResult, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let sc = litematic::Schematic::load(&path).map_err(|e| e.to_string())?;
    let a = sc.analyze();
    // 按投影自己的 dataVersion 选版本，语言表才与投影内容对得上
    let vi = db.pick_version(sc.summary().data_version);

    let mut blocks: Vec<BlockCount> = a
        .blocks_by_state
        .iter()
        .map(|(state, count)| {
            // 按名称聚合的计数用于展示，状态串用于区分具体朝向等
            let name = state.split('[').next().unwrap_or(state).to_string();
            let (zh_name, en_name) = block_display_names(db, vi, &name);
            BlockCount { name, state: state.clone(), count: *count, zh_name, en_name }
        })
        .collect();
    blocks.sort_by(|x, y| y.count.cmp(&x.count).then_with(|| x.state.cmp(&y.state)));

    // 实体 id 形如 `minecraft:zombie`，语言表里的键是 `entity.minecraft.zombie`
    let mut entities: Vec<EntityCount> = a
        .entities_by_id
        .iter()
        .map(|(id, count)| {
            let (zh_name, en_name) = entity_display_names(db, vi, id);
            EntityCount { id: id.clone(), count: *count, zh_name, en_name }
        })
        .collect();
    entities.sort_by(|x, y| y.count.cmp(&x.count).then_with(|| x.id.cmp(&y.id)));

    let mut tiles: Vec<EntityCount> = a
        .tile_entities_by_id
        .iter()
        .map(|(id, count)| {
            let (zh_name, en_name) = tile_display_names(db, vi, id);
            EntityCount { id: id.clone(), count: *count, zh_name, en_name }
        })
        .collect();
    tiles.sort_by(|x, y| y.count.cmp(&x.count).then_with(|| x.id.cmp(&y.id)));

    let mut types = std::collections::BTreeSet::new();
    for b in &blocks {
        types.insert(b.name.clone());
    }

    let entity_instances = a
        .entities
        .iter()
        .map(|e| EntityInstanceDto {
            id: e.id.clone(),
            x: e.pos[0],
            y: e.pos[1],
            z: e.pos[2],
        })
        .collect();

    Ok(AnalysisResult {
        distinct_block_types: types.len(),
        distinct_states: blocks.len(),
        blocks,
        entities,
        tile_entities: tiles,
        entity_instances,
        non_air_total: a.non_air_total,
        air_total: a.air_total,
    })
}

/// 一次最多渲染多少张方块图。
///
/// 前端一页 50 行、按 128 一批请求，正常远低于这个数；
/// 设上限是因为这是跨进程入口，参数由前端给出，不该接受无上限的工作量。
const MAX_ICON_BATCH: usize = 256;

/// 批量渲染方块图标。
///
/// 一次一批而不是一行一次：方块栏一页 50 行，逐行调用就是 50 次跨进程往返，
/// 界面会明显顿一下。
///
/// 单个方块画不出来（模型缺失、贴图缺失）只让那一项返回错误，
/// 不整批失败——某一行没有图标不该让整张表都空着。
#[tauri::command]
pub fn render_block_icons(
    app: tauri::AppHandle,
    data_version: i32,
    states: Vec<String>,
    size: Option<u32>,
) -> Result<Vec<BlockIconDto>, String> {
    if states.len() > MAX_ICON_BATCH {
        return Err(format!(
            "一次最多渲染 {MAX_ICON_BATCH} 张方块图，本次请求了 {} 张",
            states.len()
        ));
    }
    if states.is_empty() {
        return Ok(Vec::new());
    }
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    // 用投影自己的 dataVersion 选版本，语言表与贴图才与投影内容对得上
    let vi = db
        .pick_version(data_version)
        .ok_or_else(|| format!("方块库中没有数据版本 {data_version} 对应的版本"))?;
    let size = crate::block_icon::check_icon_size(size.unwrap_or(crate::block_icon::ICON_SIZE))
        .map_err(|e| e.to_string())?;

    Ok(states
        .into_iter()
        .map(|state| match crate::block_icon::render_data_url(db, vi, &state, size) {
            Ok(data_url) => BlockIconDto { state, data_url, error: String::new() },
            Err(e) => BlockIconDto { state, data_url: String::new(), error: e.to_string() },
        })
        .collect())
}

/// 已载入的实体贴图库。
///
/// 解析并解压像素池是有成本的，而导出/预览会反复查询，因此做进程内缓存。
/// 用 `RwLock<Option<..>>` 而不是 `OnceLock`：用户在「版本库」里补充版本后，
/// 实体贴图库的内容必须随之变化，`OnceLock` 一旦写入就改不了。
static ENTITY_DB: std::sync::OnceLock<
    std::sync::RwLock<Option<std::sync::Arc<crate::entitydb::EntityDb>>>,
> = std::sync::OnceLock::new();

fn entity_db_cache(
) -> &'static std::sync::RwLock<Option<std::sync::Arc<crate::entitydb::EntityDb>>> {
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
fn open_entity_db(app: &tauri::AppHandle) -> Option<std::sync::Arc<crate::entitydb::EntityDb>> {
    {
        let g = entity_db_cache().read().unwrap_or_else(|e| e.into_inner());
        if let Some(db) = g.as_ref() {
            return Some(std::sync::Arc::clone(db));
        }
    }
    let path = crate::base_db::base_entities_path(app).ok()?;
    if !path.is_file() {
        return None;
    }
    let db = match crate::entitydb::EntityDb::load(&path) {
        Ok(db) => std::sync::Arc::new(db),
        Err(e) => {
            // 不静默：贴图库损坏会让实体渲染全部没有贴图，
            // 必须留下线索，否则用户只会看到一堆白模
            crate::run_log::error(&format!("载入实体贴图库失败 {}：{e}", path.display()));
            return None;
        }
    };
    let mut g = entity_db_cache().write().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = Some(std::sync::Arc::clone(&db));
    }
    g.as_ref().map(std::sync::Arc::clone)
}

/// 实体贴图库的可用情况。
///
/// 前端据此判断实体预览能否显示贴图，而不是拿到一张空白图去猜原因。
#[tauri::command]
pub fn entity_texture_status(app: tauri::AppHandle) -> EntityTextureStatusDto {
    match open_entity_db(&app) {
        Some(db) => EntityTextureStatusDto {
            available: true,
            versions: db.versions.len(),
            textures: db.textures.len(),
            paths: db.paths.len(),
            error: String::new(),
        },
        None => EntityTextureStatusDto {
            available: false,
            versions: 0,
            textures: 0,
            paths: 0,
            error: "尚未添加任何版本的实体贴图，可在「版本库」里补充".into(),
        },
    }
}

/// 按版本与路径取一张实体贴图的 RGBA 像素。
///
/// 返回宽高与像素，前端直接建纹理。路径形如 `entity/chest/normal`；
/// 查不到时返回错误而不是空图，避免界面上出现一张「看起来是纯黑」的贴图。
#[tauri::command]
pub fn entity_texture_pixels(
    app: tauri::AppHandle,
    data_version: i32,
    path: String,
) -> Result<EntityTextureDto, String> {
    let db = open_entity_db(&app).ok_or_else(|| "实体贴图库不可用".to_string())?;
    let ver = db
        .version_for(data_version)
        .ok_or_else(|| format!("实体贴图库里没有 dataVersion {data_version}"))?;
    let (t, px) = db
        .pixels_of(ver, &path)
        .ok_or_else(|| format!("实体贴图库里没有 {path}（版本 {}）", ver.name))?;
    Ok(EntityTextureDto {
        path,
        width: t.w,
        height: t.h,
        has_alpha: t.has_alpha,
        rgba: px.to_vec(),
    })
}

/// 已载入的方块库。
pub struct LoadedDb {
    pub db: std::sync::Arc<crate::blockdb::BlockDb>,
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
fn invalidate_db() {
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
fn open_db(app: &tauri::AppHandle) -> AppResult<std::sync::Arc<LoadedDb>> {
    {
        let g = db_cache().read().unwrap_or_else(|e| e.into_inner());
        if let Some(db) = g.as_ref() {
            return Ok(std::sync::Arc::clone(db));
        }
    }

    let (db, failures) =
        crate::base_db::load_base_block_db(&crate::base_db::base_blocks_dir(app)?)?;
    for (file, reason) in &failures {
        crate::run_log::error(&format!("基础库的版本包 {file} 未能载入：{reason}"));
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

/// 跨版本检索方块
#[tauri::command]
pub fn search_blocks(
    app: tauri::AppHandle,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<BlockHitDto>, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    Ok(crate::block_search::search(db, &query, limit.unwrap_or(200))
        .into_iter()
        .map(|h| BlockHitDto {
            name: h.name,
            zh: h.zh,
            en: h.en,
            versions: h
                .versions
                .into_iter()
                .map(|v| BlockVersionDto { index: v.index, data_version: v.data_version })
                .collect(),
        })
        .collect())
}

/// 列出方块库中的全部版本
#[tauri::command]
pub fn list_block_versions(app: tauri::AppHandle) -> Result<Vec<VersionOptionDto>, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    Ok(crate::block_search::version_options(db)
        .into_iter()
        .map(|v| VersionOptionDto {
            index: v.index,
            name: v.name,
            data_version: v.data_version,
            is_overlay: v.is_overlay,
            block_count: v.block_count,
        })
        .collect())
}

/// 预计会被替换的方块数量，不写入文件
#[tauri::command]
pub fn preview_replace(
    app: tauri::AppHandle,
    schematic_path: String,
    rule: ReplaceRuleDto,
) -> Result<ReplaceOutcomeDto, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let sc = crate::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;
    let summary = sc.summary();
    let vi = db
        .pick_version(summary.data_version)
        .ok_or_else(|| "方块库中没有可用版本".to_string())?;
    let info = crate::block_model::DbBlockInfo { db, version: &db.versions[vi] };
    let rule: crate::replace::ReplaceRule = rule.into();
    Ok(ReplaceOutcomeDto {
        replaced: crate::replace::count_matches(&sc, &rule, &info),
        palette_changed: 0,
    })
}

/// 执行方块替换并写回文件。写前先备份。
#[tauri::command]
pub fn apply_replace(
    app: tauri::AppHandle,
    schematic_path: String,
    rule: ReplaceRuleDto,
) -> Result<ReplaceOutcomeDto, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let mut sc = crate::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;
    let summary = sc.summary();
    let vi = db
        .pick_version(summary.data_version)
        .ok_or_else(|| "方块库中没有可用版本".to_string())?;
    let info = crate::block_model::DbBlockInfo { db, version: &db.versions[vi] };
    let rule: crate::replace::ReplaceRule = rule.into();

    let out = crate::replace::replace_blocks(&mut sc, &rule, &info, 0);
    if out.replaced == 0 {
        // 没有任何改动就不写文件，避免无谓地产生一次备份
        return Ok(out.into());
    }

    // 改动前先备份：替换是批量改写，出错时这是唯一的回退途径。
    // 备份根目录必须与 list_backups / save_metadata 一致（都是 exe_dir），
    // 否则替换产生的备份在「备份管理」里永远看不到，也无法还原。
    backup::create(&exe_dir(), Path::new(&schematic_path), "replace")
        .map_err(|e| e.to_string())?;
    sc.save_to(&schematic_path).map_err(|e| e.to_string())?;

    let name = Path::new(&schematic_path)
        .file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    crate::run_log::info(&replace_log_line(
        db,
        Some(vi),
        &name,
        &rule.from_name,
        &rule.to_name,
        out.replaced,
    ));
    Ok(out.into())
}

/// 日志里怎么称呼一个方块：有中文名时写成「中文名（方块ID）」，没有就只写 ID。
///
/// 日志是事后翻的：只写 ID 得对着方块库才看得懂替换了什么，只写中文名又对不上
/// 文件里的内容，两个都给才既读得懂又查得准。查不到名字时不能留下空括号。
fn log_block_name(db: &BlockDb, version_idx: Option<usize>, name: &str) -> String {
    let (zh, _) = block_display_names(db, version_idx, name);
    if zh.is_empty() {
        name.to_string()
    } else {
        format!("{zh}（{name}）")
    }
}

/// 方块替换写入操作日志的那一行。
///
/// 单独成函数是为了能直接测格式：真正的命令要 AppHandle 才能取方块库。
fn replace_log_line(
    db: &BlockDb,
    version_idx: Option<usize>,
    file_name: &str,
    from_name: &str,
    to_name: &str,
    replaced: u64,
) -> String {
    format!(
        "替换 {file_name}：{} -> {}，共 {replaced} 个",
        log_block_name(db, version_idx, from_name),
        log_block_name(db, version_idx, to_name),
    )
}

/// 列出可作为转换目标的版本
#[tauri::command]
pub fn convert_targets(app: tauri::AppHandle, schematic_path: String) -> Result<Vec<ConvertTargetDto>, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let sc = crate::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;
    let dv = sc.summary().data_version;
    Ok(crate::convert::targets(db, dv)
        .into_iter()
        .map(|t| ConvertTargetDto { index: t.index, name: t.name, data_version: t.data_version })
        .collect())
}

/// 分析：投影中用到但目标版本没有的方块
#[tauri::command]
pub fn analyze_conversion(
    app: tauri::AppHandle,
    schematic_path: String,
    target_index: usize,
) -> Result<Vec<ConvertItemDto>, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let sc = crate::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;

    // 源方块的中文名要按**投影自己的版本**查：这些方块在目标版本里已经
    // 没有了，只有原版本的语言表里才有它们的名字。
    //
    // pick_version 取的是「最接近投影版本的基础版本」，库里只要有一个基础
    // 版本就不会落空；真要落到 unwrap_or 上（库里没有任何基础版本），
    // 那时用户也选不出目标版本，这段回退实际上走不到——留着只是为了不 panic。
    // target_index 来自前端、可能越界，但 lang_index 取不到版本时返回空表。
    let source_idx = db.pick_version(sc.summary().data_version).unwrap_or(target_index);

    Ok(convert_items_with_zh(
        crate::convert::analyze(&sc, db, target_index),
        &crate::block_search::lang_index(db, source_idx),
        &crate::block_search::lang_index(db, target_index),
    ))
}

/// 给转换条目补上中英文名。
///
/// 单独成函数是为了能直接测：真正的命令要 AppHandle 才能取方块库，
/// 而这里只依赖两张语言表，用构造出来的表就能覆盖各级回退。
fn convert_items_with_zh(
    items: Vec<crate::convert::ConvertItem>,
    source_langs: &std::collections::HashMap<String, (String, String)>,
    target_langs: &std::collections::HashMap<String, (String, String)>,
) -> Vec<ConvertItemDto> {
    items
        .into_iter()
        .map(|i| ConvertItemDto {
            // 源方块先查原版本的表，再退到目标版本的表：它在原版本里一定存在，
            // 但投影用的版本号不一定能在库里对上，那时只能靠目标版本的表
            name_zh: name_from_tables(&i.name, &[source_langs, target_langs], false),
            name_en: name_from_tables(&i.name, &[source_langs, target_langs], true),
            // 替代品是目标版本里的方块，只查目标版本的表
            suggested_zh: name_from_tables(&i.suggested, &[target_langs], false),
            suggested_en: name_from_tables(&i.suggested, &[target_langs], true),
            name: i.name,
            count: i.count,
            suggested: i.suggested,
            chosen: i.chosen,
        })
        .collect()
}

/// 按给定顺序在各语言表里查名字，全都没查到时退回不带命名空间的方块名。
///
/// 中英两套都要给：界面按当前语言取一套，缺哪套就退回另一套并标注
/// 「语言表中没有这一项」，与方块表、实体表的行为一致——两边不一致时，
/// 同一个方块在「方块」页签标灰、在版本转换窗口却不标，用户无从判断。
///
/// 顺序表而不是单个表：源方块在两个版本的语言表里可能只有一边有条目，
/// 因此**必须把表找完**再决定要不要退回空串。
///
/// 只有整张表里都没有这个方块时才退回不带命名空间的方块名：那种情况下
/// 界面无从判断它是缺名字还是坏了，给一个能认的 ID 比留白有用。
fn name_from_tables(
    name: &str,
    tables: &[&std::collections::HashMap<String, (String, String)>],
    want_en: bool,
) -> String {
    if name.is_empty() {
        return String::new();
    }
    // 认得出这个方块、但缺想要的那种语言的名字
    let mut known_without_wanted = false;
    for t in tables {
        if let Some((zh, en)) = t.get(name) {
            let (first, second) = if want_en { (en, zh) } else { (zh, en) };
            if !first.is_empty() {
                return first.clone();
            }
            if !second.is_empty() {
                known_without_wanted = true;
            }
        }
    }
    if known_without_wanted {
        return String::new();
    }
    name.split_once(':').map(|(_, n)| n.to_string()).unwrap_or_else(|| name.to_string())
}

/// 执行转换，结果另存为新文件，原文件不动。
#[tauri::command]
pub fn apply_conversion(
    app: tauri::AppHandle,
    schematic_path: String,
    target_index: usize,
    items: Vec<ConvertItemDto>,
) -> Result<ConvertOutcomeDto, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let mut sc = crate::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;

    let source_idx = db
        .pick_version(sc.summary().data_version)
        .ok_or_else(|| "方块库中找不到与投影匹配的版本".to_string())?;
    let target = db
        .versions
        .get(target_index)
        .ok_or_else(|| "目标版本不存在".to_string())?;

    // 另存为新文件：转换会大量改写方块，直接覆盖原文件风险太大
    let src = Path::new(&schematic_path);
    let dir = src.parent().unwrap_or_else(|| Path::new("."));
    let stem = src
        .file_stem()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_else(|| "projection".into());
    // 版本名可能含路径分隔符或 `..`（基础库的版本名取自 jar 里的
    // version.json，用户也能自己往 base/blocks 放包），必须清理成一段
    // 安全文件名再拼接，否则写出路径会离开投影所在目录。
    let out_path = dir.join(format!(
        "{stem}_{}.litematic",
        crate::convert::safe_file_component(&target.name)
    ));

    let items: Vec<crate::convert::ConvertItem> = items
        .into_iter()
        .map(|i| crate::convert::ConvertItem {
            name: i.name,
            count: i.count,
            suggested: i.suggested,
            chosen: i.chosen,
        })
        .collect();

    let out = crate::convert::convert(&mut sc, db, source_idx, target_index, &items);
    // 只有真的替换过方块才需要重编码 BlockStates 与调色板；
    // 若仅改版本号（方块都没变），save_to 仍会序列化整个 root，
    // 版本号照样写入，无需无谓地重写全部区域数据。
    sc.blocks_modified = out.replaced_blocks > 0;
    sc.save_to(&out_path.to_string_lossy()).map_err(|e| e.to_string())?;

    let name = out_path
        .file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    crate::run_log::info(&format!(
        "转换 {name}：替换 {} 种方块，{} 种未指定",
        out.replaced_kinds, out.skipped_kinds
    ));

    Ok(ConvertOutcomeDto {
        replaced_kinds: out.replaced_kinds,
        replaced_blocks: out.replaced_blocks,
        skipped_kinds: out.skipped_kinds,
        output_path: out_path.to_string_lossy().into_owned(),
    })
}

/// 在一个或多个目录下筛查内容重复的投影文件。
///
/// 递归遍历子目录，用的是文件列表那套收集逻辑与同一个层数上限，因此
/// 用户在列表里看到的文件都会参与筛查。原先只读目录第一层：投影按分类
/// 放进子目录时，名单里明明有成对的副本，筛查却永远报「没有发现重复文件」。
///
/// 多个目录一次收齐再统一分组，所以跨目录的副本也在同一组里——原先逐目录
/// 筛查再拼接，跨目录的同一份内容会被拆开、只含单个成员的那一组还会被丢掉。
///
/// 声明为 async 是有意的：Tauri 里同步命令跑在主线程上，把目录下所有候选
/// 文件逐字节读一遍会把窗口连同界面一起冻住；交给阻塞线程池后，筛查期间
/// 界面照常响应，按钮上的加载态也转得起来。
#[tauri::command]
pub async fn scan_duplicates(dirs: Vec<String>) -> Result<DuplicateScanDto, String> {
    // 只可能因为阻塞任务本身没能跑完（线程池关闭、任务 panic）而失败；
    // 文件级、目录级的失败都在结果里逐条上报。
    tauri::async_runtime::spawn_blocking(move || scan_duplicates_in(&dirs))
        .await
        .map_err(|e| format!("筛查任务未能完成：{e}"))
}

/// 筛查的同步实现。单独成函数是为了能在测试里直接调用，
/// 不必为了一条用例把整个异步运行时拉起来。
///
/// 不返回 Result：所有失败都按文件折进 unreadable、按目录折进 skipped_dirs，
/// 一次筛查本身没有「整体失败」这种结局。
fn scan_duplicates_in(dirs: &[String]) -> DuplicateScanDto {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut candidates: Vec<crate::duplicate::Candidate> = Vec::new();
    let mut unreadable: Vec<(PathBuf, String)> = Vec::new();
    let mut skipped = SkipReport::default();
    let mut scanned = 0usize;

    for dir in dirs {
        let mut found: Vec<PathBuf> = Vec::new();
        collect_litematics(Path::new(dir), SCAN_MAX_DEPTH, &mut found, &mut skipped);
        for path in found {
            // 目录可以叠加加载（父目录与它的子目录同时选中），同一个文件
            // 只能算一份候选：重复计入会让组里出现同一个路径、
            // 「多余的副本」也会被多算一份。
            if !seen.insert(path.clone()) {
                continue;
            }
            match std::fs::metadata(&path) {
                Ok(m) => {
                    // 只有真的进入比对的文件才算「已筛查」：拿不到大小的文件
                    // 没有参与判定，算进去会让界面上的数字与实际情况不符。
                    scanned += 1;
                    candidates.push(crate::duplicate::Candidate { path, size: m.len() });
                }
                // 拿不到大小就无法参与「先按大小分组」。如实上报，而不是
                // 当成 0 字节：按 0 分组会让读不出来的文件与真正的空文件混作一堆。
                Err(e) => unreadable.push((path, e.to_string())),
            }
        }
    }

    let r = crate::duplicate::find(&candidates);
    unreadable.extend(r.unreadable);
    unreadable.sort();

    DuplicateScanDto {
        scanned,
        groups: r
            .groups
            .into_iter()
            .map(|g| DuplicateGroupDto {
                size: g.size,
                paths: g.paths.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
            })
            .collect(),
        unreadable: unreadable
            .into_iter()
            .map(|(p, reason)| UnreadableFileDto {
                path: p.to_string_lossy().into_owned(),
                reason,
            })
            .collect(),
        skipped_dirs: skipped.count,
    }
}

/// 列出备份。stem 非空时只返回该文件的备份。
#[tauri::command]
pub fn list_backups(stem: Option<String>) -> Vec<BackupEntryDto> {
    backup::list(&backup::backup_root(&exe_dir()))
        .into_iter()
        .filter(|e| stem.as_deref().map_or(true, |s| s == e.stem))
        .map(|e| BackupEntryDto {
            path: e.path.to_string_lossy().into_owned(),
            stem: e.stem,
            op: e.op,
            created_ns: e.created_ns.to_string(),
            size: e.size,
        })
        .collect()
}

/// 把备份还原到目标文件。
///
/// 还原是覆盖写，因此先校验备份与目标同源：跨文件还原很容易把 A 的备份
/// 盖到 B 上，而覆盖后 B 的原内容只剩它自己的备份可寻。
#[tauri::command]
pub fn restore_backup(backup_path: String, target_path: String) -> Result<(), String> {
    let b = PathBuf::from(&backup_path);
    let t = PathBuf::from(&target_path);
    if !backup::matches_target(&b, &t) {
        return Err(format!(
            "备份与目标文件不同源，已中止还原：{}",
            t.file_name().map(|v| v.to_string_lossy().into_owned()).unwrap_or_default()
        ));
    }
    // 还原是覆盖写：先把当前内容备份一份，否则一旦还原错了，
    // 目标文件的现有内容就只剩更早的备份可寻。
    if t.is_file() {
        backup::create(&exe_dir(), &t, "restore-before").map_err(|e| e.to_string())?;
    }
    backup::restore(&b, &t).map_err(|e| e.to_string())?;
    let name = t.file_name().map(|v| v.to_string_lossy().into_owned()).unwrap_or_default();
    crate::run_log::info(&format!("还原 {name} <- {backup_path}"));
    Ok(())
}

/// 删除一条备份。
#[tauri::command]
pub fn delete_backup(backup_path: String) -> Result<(), String> {
    let p = PathBuf::from(&backup_path);
    // 参数来自前端，必须限定在备份根目录内：删除是不可逆操作，
    // 不能允许任意路径被删掉。
    let root = backup::backup_root(&exe_dir());
    if !backup::is_inside(&root, &p) {
        return Err(format!("拒绝删除备份目录之外的路径：{}", p.display()));
    }
    backup::delete(&p).map_err(|e| e.to_string())?;
    let name = p.file_name().map(|v| v.to_string_lossy().into_owned()).unwrap_or_default();
    crate::run_log::info(&format!("删除备份 {name}"));
    Ok(())
}

/// 读取运行日志，由新到旧。
#[tauri::command]
pub fn read_run_log(max_lines: Option<usize>) -> Vec<String> {
    crate::run_log::read(max_lines.unwrap_or(200).min(5000))
}

/// 清空运行日志。
///
/// 只清空内容、不删文件（理由见 `RunLog::clear`）。这是用户显式要求的
/// 动作，失败必须报出来——不像写日志那样可以悄悄放过。
#[tauri::command]
pub fn clear_run_log() -> Result<(), String> {
    crate::run_log::clear().map_err(|e| e.to_string())
}

/// 在文件管理器里打开运行日志所在的目录。
///
/// 路径在后端算：日志位置是后端的约定（`run_log` 决定），前端再拼一遍
/// 就成了两处规则，改一处必然对不上。
#[tauri::command]
pub fn open_run_log_dir() -> Result<(), String> {
    let Some(log) = crate::run_log::path() else {
        return Err("运行日志尚未启用，无法定位所在目录".to_string());
    };
    // 日志存在就定位到它：资源管理器会打开所在目录并选中该文件。
    // 从没写过日志时它还不存在，那就先建出日志目录再打开目录本身——
    // 用户此刻想知道的正是「日志放在哪」。
    if log.exists() {
        return crate::file_ops::reveal_in_file_manager(&log).map_err(|e| e.to_string());
    }
    let dir = log.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("创建目录 {} 失败：{e}", dir.display()))?;
    crate::file_ops::open_directory(&dir).map_err(|e| e.to_string())
}

/// 界面把自身遇到的警告与错误交给后端记录。
///
/// 界面侧的问题（界面状态读写失败、未捕获异常等）只有界面知道，不交给后端就
/// 永远不会出现在运行日志里，用户报「界面不对劲」时也就无据可查。
/// 等级非法时直接报错：把界面拼错的等级静默当成默认等级，等于让日志悄悄丢内容。
#[tauri::command]
pub fn log_ui_event(level: String, message: String) -> Result<(), String> {
    let parsed = crate::run_log::Level::parse(&level)
        .ok_or_else(|| format!("无法识别的日志等级：{level}"))?;
    // 前缀标明来源：日志里界面与后端的行混在一起，不标就分不清是谁写的
    crate::run_log::write(parsed, &format!("界面 {message}"));
    Ok(())
}

/// 运行日志所在目录；日志没启用时返回 None。
///
/// 目录由后端算（`run_log` 决定），界面拿来显示给用户看。让界面自己拼一遍
/// 应用数据目录就成了两处规则，identifier 一变就会指向不存在的位置——
/// 这个项目已经改过一次 identifier（见 `data_dir`）。
#[tauri::command]
pub fn run_log_dir() -> Option<String> {
    crate::run_log::path()
        .and_then(|p| p.parent().map(|d| d.display().to_string()))
}

/// 界面把日志等级与本地日期交给后端。
/// 等级由界面持久化（界面状态文件），这里只负责落地：等级决定写入哪些行，
/// 本地日期只用于归档文件名——Rust 侧不存本地时间，避免为文件名引入时区依赖。
/// 界面每次改等级、每次打开日志窗口都会调用，因此日期跟着刷新。
#[tauri::command]
pub fn configure_run_log(level: String, local_date: String) -> Result<(), String> {
    let parsed = crate::run_log::Level::parse(&level)
        .ok_or_else(|| format!("无法识别的日志等级：{level}"))?;
    // 日期会被拼进归档文件名，因此按约定形状校验：只认 YYYY-MM-DD。
    // 界面那边固定这么产出，这里是防「拼进去一个奇怪的路径片段」这类意外。
    if !is_local_date(&local_date) {
        return Err(format!("本地日期格式应为 YYYY-MM-DD：{local_date}"));
    }
    let changed =
        crate::run_log::configure(parsed, &local_date).map_err(|e| e.to_string())?;
    // 只在真的变了时记一行：界面每开一次日志窗口都会同步一次同样的设置，
    // 每次都记就成了噪音
    if changed {
        crate::run_log::info(&format!(
            "日志等级设为 {}，归档日期 {}",
            parsed.token(),
            local_date
        ));
    }
    Ok(())
}

/// 是不是 `YYYY-MM-DD` 形状的本地日期。
fn is_local_date(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
}

/// 启动时按界面存下的等级取值；没存过或认不出时用 info（默认等级）。
///
/// 之所以从界面状态文件里读而不是等界面推过来：debug 用户要的是**从启动第一行**
/// 起的完整日志，如果等事件循环里界面来设置，启动阶段早就记完了。
///
/// 读的是前端 `uiState.ts` 写下的 `logLevel` 键：界面是这份状态的唯一作者，
/// 因此键改名要两边一起改（见 src/store/uiState.ts 的 UiState）。
pub(crate) fn saved_log_level(app: &tauri::AppHandle) -> crate::run_log::Level {
    let path = match ui_state_path(app) {
        Ok(p) => p,
        Err(e) => {
            crate::run_log::warn(&format!("读取界面状态路径失败，日志等级按默认：{e}"));
            return crate::run_log::Level::Info;
        }
    };
    match read_ui_state_file(&path) {
        Ok(Some(json)) => log_level_from_ui_state(&json),
        // 从未保存过界面状态：正常情况，用默认等级
        Ok(None) => crate::run_log::Level::Info,
        Err(e) => {
            crate::run_log::warn(&format!("读取界面状态失败，日志等级按默认：{e}"));
            crate::run_log::Level::Info
        }
    }
}

/// 从界面状态 JSON 里取日志等级；取不到或认不出时用默认的 info。
fn log_level_from_ui_state(json: &str) -> crate::run_log::Level {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| {
            v.get("logLevel")
                .and_then(|l| l.as_str())
                .and_then(crate::run_log::Level::parse)
        })
        .unwrap_or(crate::run_log::Level::Info)
}

/// 组装前端 Three.js 渲染所需的场景。
///
/// 方块库作为应用资源随包分发，路径由这里解析，
/// 前端不必知道它在哪个目录，也不必关心各平台的资源布局差异。
#[tauri::command]
pub fn build_scene(app: tauri::AppHandle, schematic_path: String) -> Result<SceneDto, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    crate::scene::build_with_db(&schematic_path, db)
        .map(SceneDto::from)
        .map_err(|e| e.to_string())
}

/// 保存元数据（投影名称 / 作者 / 描述）。
///
/// 写入前先在 backups/ 下留一份原件：这几项都是用户手动输入的内容，
/// 出错时必须有可回退的原件。备份失败则中止保存，不做「没备份就改」。
#[tauri::command]
pub fn save_metadata(
    path: String,
    name: Option<String>,
    author: Option<String>,
    description: Option<String>,
) -> Result<SaveOutcome, String> {
    // NBT 的字符串长度字段是 u16，写入时超限只能报错。在碰文件之前先校验：
    // 一是让用户拿到「哪一项太长」的可读提示，而不是等写盘阶段才失败
    // （那时的错误来自编码深层，容易被误当成文件损坏）；二是这类校验
    // 不该依赖磁盘状态。
    //
    // 这里按字符数粗筛，精确的字节数由 nbt 层按 modified UTF-8 判定。
    // 中文每字 3 字节；用 20000 字符做上限，留出足够余量又远低于 65535 字节。
    const MAX_META_CHARS: usize = 20000;
    for (label, v) in [
        ("名称", &name),
        ("作者", &author),
        ("描述", &description),
    ] {
        if let Some(s) = v {
            let n = s.chars().count();
            if n > MAX_META_CHARS {
                return Err(format!("{label}过长：{n} 个字符，上限 {MAX_META_CHARS} 个字符"));
            }
        }
    }

    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(format!("文件不存在：{path}"));
    }

    // 先载入并判断是否真的有变化，再决定要不要备份。
    // 顺序反过来的话，重复保存相同内容也会产出无用备份。
    let mut sc = litematic::Schematic::load(&path).map_err(|e| e.to_string())?;

    let mut changed = false;
    if let Some(v) = name {
        changed |= sc.set_meta_name(&v);
    }
    if let Some(v) = author {
        changed |= sc.set_meta_author(&v);
    }
    if let Some(v) = description {
        changed |= sc.set_meta_description(&v);
    }

    if !changed {
        // 可能是值本身没有变化，也可能是该投影缺少这几项字段。
        // 两种都不应备份、也不应写盘；后者需要让用户知道。
        let md = sc.root.get("Metadata");
        let has_fields = md.is_some()
            && ["Name", "Author", "Description"]
                .iter()
                .any(|k| md.and_then(|m| m.get(k)).is_some());
        if !has_fields {
            return Err("该投影缺少可编辑的元数据字段".into());
        }
        return Err("内容没有变化，未做保存".into());
    }

    let bak = backup::create(&exe_dir(), &p, "metadata")
        .map_err(|e| format!("备份失败，已中止保存：{e}"))?;

    sc.save_to(&path).map_err(|e| format!("写入失败：{e}"))?;

    Ok(SaveOutcome {
        path,
        backup_path: bak.backup_path.to_string_lossy().into_owned(),
    })
}

/// 查询 dataVersion 对应的游戏版本名。
#[tauri::command]
pub fn mc_version_name(data_version: i32) -> String {
    crate::mcver::version_name_from_data_version(data_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 补语言表时只请求 jar 里没有的那些。
    ///
    /// 资源索引里没有 `minecraft/lang/en_us.json`（官方把它留在客户端 jar 里），
    /// 若把 `en_us` 也算作「本地缺、要去下」，库完全正常时也会报「未取到中文
    /// 语言表」，用户会以为中文名没生效。
    #[test]
    fn only_langs_absent_from_the_jar_are_downloaded() {
        // 本机 .minecraft/assets 为空（从没启动过游戏）——这是最常见的场景
        assert_eq!(langs_to_download(&[]), vec!["zh_cn"]);

        // 本机已经有中文表（用户启动过游戏）时什么都不用下
        let local = vec![(
            "assets/minecraft/lang/zh_cn.json".to_string(),
            b"{}".to_vec(),
        )];
        assert!(langs_to_download(&local).is_empty());

        // 只有英文表（客户端 jar 里那份）不算缺中文
        let en_only = vec![(
            "assets/minecraft/lang/en_us.json".to_string(),
            b"{}".to_vec(),
        )];
        assert_eq!(langs_to_download(&en_only), vec!["zh_cn"]);
    }

    /// 每个用例用独立的临时目录，避免并行执行时互相覆盖
    fn temp_state_dir(tag: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!(
            "sm-ui-state-{tag}-{}-{}",
            std::process::id(),
            // 用纳秒时间戳区分同一进程内的多次调用
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|v| v.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).expect("建临时目录失败");
        d
    }

    /// 重复筛查必须走进子目录。
    ///
    /// 原先只读目录第一层：投影按分类放进子目录时，列表里明明有成对的副本，
    /// 筛查却一律报「没有发现重复文件」。用户的投影正是按版本分类放在
    /// 子目录里的，这条守住那个场景。
    #[test]
    fn duplicate_scan_recurses_into_subdirectories() {
        let root = temp_state_dir("dup-recurse");
        let sub = root.join("1.21.1");
        std::fs::create_dir_all(&sub).expect("建子目录失败");
        std::fs::write(sub.join("a.litematic"), b"same bytes").unwrap();
        std::fs::write(sub.join("b.litematic"), b"same bytes").unwrap();
        std::fs::write(root.join("c.litematic"), b"different!!").unwrap();

        let r = scan_duplicates_in(&[root.to_string_lossy().into_owned()]);

        assert_eq!(r.scanned, 3, "子目录里的文件也要计入筛查范围");
        assert_eq!(r.groups.len(), 1, "子目录里的一对副本应被识别");
        assert_eq!(r.groups[0].paths.len(), 2);
        assert!(r.unreadable.is_empty(), "不应有读取失败：{:?}", r.unreadable);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 跨目录的同一份内容必须归到同一组。
    ///
    /// 原先按目录分别筛查再拼接：跨目录的副本各自只有一个成员，
    /// 而只有一个成员的组会被丢掉，于是「同一份投影复制到两个目录」
    /// 这种最常见的重复反而查不出来。
    #[test]
    fn duplicate_scan_groups_copies_across_directories() {
        let a = temp_state_dir("dup-cross-a");
        let b = temp_state_dir("dup-cross-b");
        std::fs::write(a.join("x.litematic"), b"copy of one").unwrap();
        std::fs::write(b.join("y.litematic"), b"copy of one").unwrap();
        std::fs::write(b.join("z.litematic"), b"something else").unwrap();

        let dirs = vec![
            a.to_string_lossy().into_owned(),
            b.to_string_lossy().into_owned(),
        ];
        let r = scan_duplicates_in(&dirs);

        assert_eq!(r.scanned, 3);
        assert_eq!(r.groups.len(), 1, "两个目录里的同一份内容应合成一组");
        assert_eq!(r.groups[0].paths.len(), 2, "组内应含两个目录各一份");

        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    /// 父子目录同时被加载时，同一个文件只能算一份候选。
    ///
    /// 否则组里会出现同一个路径两次，「多余的副本」会被多算，
    /// 默认勾选也就跟着出错。
    #[test]
    fn duplicate_scan_counts_each_file_once() {
        let root = temp_state_dir("dup-overlap");
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).expect("建子目录失败");
        std::fs::write(sub.join("a.litematic"), b"only one").unwrap();

        let dirs = vec![
            root.to_string_lossy().into_owned(),
            sub.to_string_lossy().into_owned(),
        ];
        let r = scan_duplicates_in(&dirs);

        assert_eq!(r.scanned, 1, "父子目录叠加不应把同一个文件算两次");
        assert!(r.groups.is_empty(), "只有一份的文件不该成组");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 异步命令外壳本身也要能真的出结果。
    ///
    /// 上面几条直接调同步实现，覆盖不到命令外壳：`spawn_blocking` 要 await
    /// 才拿得到值、错误要经 JoinHandle 再映射一次，写成 `?` 而不是 `.await?`
    /// 这类问题只有真跑一遍才暴露。
    #[test]
    fn duplicate_scan_command_runs_on_the_async_runtime() {
        let root = temp_state_dir("dup-cmd");
        std::fs::write(root.join("a.litematic"), b"same").unwrap();
        std::fs::write(root.join("b.litematic"), b"same").unwrap();

        let dirs = vec![root.to_string_lossy().into_owned()];
        let r = tauri::async_runtime::block_on(scan_duplicates(dirs)).expect("命令应返回结果");

        assert_eq!(r.scanned, 2);
        assert_eq!(r.groups.len(), 1, "命令外壳拿到的结果应与同步实现一致");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 转换条目必须带上中文名，各级回退都要对。
    ///
    /// 界面上凡是出现方块 ID 的地方都要同时显示中文名，因此这条盯的是
    /// 「名字从哪来」：源方块查原版本的表（它在目标版本里已经不存在），
    /// 替代品只查目标版本的表，都查不到时退回不带命名空间的方块名，
    /// 而不是留一个空串让界面空白。
    #[test]
    fn convert_items_carry_chinese_names_with_fallbacks() {
        let mut source = std::collections::HashMap::new();
        source.insert(
            "minecraft:oak_stairs".to_string(),
            ("橡木楼梯".to_string(), "Oak Stairs".to_string()),
        );
        // 只有英文名：缺中文名时不能留白
        source.insert(
            "minecraft:old_block".to_string(),
            (String::new(), "Old Block".to_string()),
        );

        let mut target = std::collections::HashMap::new();
        target.insert(
            "minecraft:pale_oak_stairs".to_string(),
            ("苍白橡木楼梯".to_string(), "Pale Oak Stairs".to_string()),
        );

        let items = vec![
            crate::convert::ConvertItem {
                name: "minecraft:oak_stairs".into(),
                count: 12,
                suggested: "minecraft:pale_oak_stairs".into(),
                chosen: String::new(),
            },
            // 没有推荐：中文名也必须是空，不能凭空编一个
            crate::convert::ConvertItem {
                name: "minecraft:old_block".into(),
                count: 3,
                suggested: String::new(),
                chosen: String::new(),
            },
            // 两张表都没有：退回方块名本身
            crate::convert::ConvertItem {
                name: "mymod:unknown_thing".into(),
                count: 1,
                suggested: String::new(),
                chosen: String::new(),
            },
            // 推荐品只存在于原版本的表里：替代品不该借用源表的名字
            crate::convert::ConvertItem {
                name: "minecraft:old_block".into(),
                count: 1,
                suggested: "minecraft:oak_stairs".into(),
                chosen: String::new(),
            },
            // 源方块在源表里没有、只在目标表里有：必须退到第二张表去找
            crate::convert::ConvertItem {
                name: "minecraft:pale_oak_stairs".into(),
                count: 2,
                suggested: String::new(),
                chosen: String::new(),
            },
            // 空名字：两个字段都应是空串，不能拼出去命名空间的分支
            crate::convert::ConvertItem {
                name: String::new(),
                count: 0,
                suggested: String::new(),
                chosen: String::new(),
            },
        ];

        let out = convert_items_with_zh(items, &source, &target);

        assert_eq!(out[0].name_zh, "橡木楼梯");
        assert_eq!(out[0].name_en, "Oak Stairs", "英文名也要一并给出，供英语界面用");
        assert_eq!(out[0].suggested_zh, "苍白橡木楼梯");
        assert_eq!(out[0].suggested_en, "Pale Oak Stairs");
        assert_eq!(
            out[1].name_zh, "",
            "缺中文名时留空，由界面退回英文名并标注缺项，与方块表一致"
        );
        assert_eq!(out[1].name_en, "Old Block", "这一套名字本身是有的");
        assert_eq!(out[1].suggested_zh, "", "没有推荐时中文名必须为空");
        assert_eq!(out[1].suggested_en, "", "没有推荐时英文名必须为空");
        assert_eq!(out[2].name_zh, "unknown_thing", "查不到应退回方块名而不是空串");
        assert_eq!(out[2].name_en, "unknown_thing");
        assert_eq!(
            out[3].suggested_zh, "oak_stairs",
            "替代品只能在目标版本的表里查，不能借用源版本的名字"
        );
        assert_eq!(out[3].suggested_en, "oak_stairs");
        assert_eq!(
            out[4].name_zh, "苍白橡木楼梯",
            "源表里没有时应退到目标表去找"
        );
        assert_eq!(out[4].name_en, "Pale Oak Stairs");
        assert_eq!(out[5].name_zh, "", "空方块名不该拼出一个名字");
        assert_eq!(out[5].name_en, "");
        assert_eq!(out[5].suggested_zh, "");
        assert_eq!(out[5].suggested_en, "");
        assert_eq!(out[0].name, "minecraft:oak_stairs", "原字段不应被改动");
        assert_eq!(out[0].count, 12);
    }

    /// 替换日志要带上方块中文名。
    ///
    /// 日志是事后翻的：只写 `minecraft:stone` 得对着方块库才看得懂替换了什么，
    /// 只写中文名又对不上文件里的内容。查不到名字时必须只写 ID，
    /// 不能留下空括号。
    #[test]
    fn replace_log_line_carries_chinese_names() {
        let mut db = sample_db("pack", 100, "minecraft:stone", "tex", 0);
        db.langs = vec![vec![
            LangEntry {
                block_name: "minecraft:stone".into(),
                en: "Stone".into(),
                zh: "石头".into(),
            },
            LangEntry {
                block_name: "minecraft:dirt".into(),
                en: "Dirt".into(),
                zh: "泥土".into(),
            },
        ]];

        let line = replace_log_line(&db, Some(0), "demo.litematic", "minecraft:stone", "minecraft:dirt", 12);
        assert!(line.starts_with("替换 demo.litematic："), "应保留原有结构：{line}");
        assert!(line.contains("石头（minecraft:stone）"), "源方块应带中文名：{line}");
        assert!(line.contains("泥土（minecraft:dirt）"), "目标方块应带中文名：{line}");
        assert!(line.ends_with("共 12 个"), "应保留数量：{line}");

        // 方块库里查不到名字（模组方块）：只写 ID，不留空括号
        let unknown = replace_log_line(&db, Some(0), "demo.litematic", "mod:unknown", "minecraft:dirt", 1);
        assert!(unknown.contains("mod:unknown ->"), "查不到名字时应只写 ID：{unknown}");
        assert!(!unknown.contains("（）"), "不该出现空括号：{unknown}");
    }

    #[test]
    fn ui_state_round_trips_through_the_file() {
        let dir = temp_state_dir("rt");
        let path = dir.join(UI_STATE_FILE);
        let json = r#"{"version":1,"sidebarWidth":820,"dirs":["C:/D1"]}"#;

        write_ui_state_file(&path, json).expect("写入失败");
        let got = read_ui_state_file(&path).expect("读取失败");
        assert_eq!(got.as_deref(), Some(json), "写进去的应能原样读回来");

        // 覆盖写：第二次必须替换掉第一次，而不是追加或残留
        let json2 = r#"{"version":1,"sidebarWidth":666,"dirs":[]}"#;
        write_ui_state_file(&path, json2).expect("覆盖写入失败");
        assert_eq!(
            read_ui_state_file(&path).expect("读取失败").as_deref(),
            Some(json2),
            "第二次保存应替换旧内容"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ui_state_missing_file_is_not_an_error() {
        let dir = temp_state_dir("missing");
        let path = dir.join(UI_STATE_FILE);
        // 首次启动：文件还不存在。这是正常情况，必须返回 None 而不是错误，
        // 否则每次首启都会弹出一个「读取界面状态失败」的告警。
        let got = read_ui_state_file(&path).expect("文件不存在不应视为错误");
        assert!(got.is_none(), "未保存过时应返回 None");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ui_state_write_leaves_no_temp_file_behind() {
        let dir = temp_state_dir("tmp");
        let path = dir.join(UI_STATE_FILE);
        write_ui_state_file(&path, "{}").expect("写入失败");
        // 临时文件必须已被改名带走：残留会在下次写入时产生
        // 「目标已存在」之类的意外，也会让用户目录里留下垃圾
        assert!(!path.with_extension("json.tmp").exists(), "不应残留临时文件");
        let names: Vec<String> = std::fs::read_dir(&dir)
            .expect("列目录失败")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![UI_STATE_FILE.to_string()], "目录里应只剩状态文件本身");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ui_state_failed_write_keeps_previous_content() {
        let dir = temp_state_dir("fail");
        let path = dir.join(UI_STATE_FILE);
        let good = r#"{"version":1,"sidebarWidth":700}"#;
        write_ui_state_file(&path, good).expect("首次写入失败");

        // 用一个必然失败的目标模拟「写盘中途失败」：
        // 把临时文件的位置占成一个目录，写入就会失败。
        // 关键是**原有内容必须完好**——否则用户所有界面设置会被清零。
        std::fs::create_dir_all(path.with_extension("json.tmp")).expect("占位失败");
        let r = write_ui_state_file(&path, r#"{"version":1,"sidebarWidth":1}"#);
        assert!(r.is_err(), "临时文件写不进去时应报错，而不是静默成功");
        assert_eq!(
            read_ui_state_file(&path).expect("读取失败").as_deref(),
            Some(good),
            "写入失败后原有内容必须保持不变"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // 注意：这里的路径必须与平台无关。    // std::path::Path 按各平台的分隔符切分，在 Unix 上反斜杠只是普通字符，
    // 因此 Windows 风格的 "Z:\\x.litematic" 在 Linux 上会被整体当作文件名。
    // 测试只关心「文件不存在时的行为」，用纯文件名即可。
    #[test]
    fn scan_missing_dir_returns_empty() {
        let r = scan_litematics("__sm_not_exist_dir__".into(), Some(2));
        assert!(r.items.is_empty());
        assert!(r.skipped_dirs > 0, "不存在的目录应计入跳过数");
    }

    #[test]
    fn peek_missing_file_reports_error() {
        let s = peek_schematic("__sm_not_exist__.litematic".into());
        assert!(!s.error.is_empty(), "文件不存在时应给出错误原因");
        assert_eq!(s.file_name, "__sm_not_exist__.litematic");
    }

    #[test]
    fn scan_finds_litematic_files_only() {
        let dir = std::env::temp_dir().join("sm_scan_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.litematic"), b"x").unwrap();
        std::fs::write(dir.join("b.txt"), b"x").unwrap();
        std::fs::write(dir.join("sub/c.LITEMATIC"), b"x").unwrap();

        let r = scan_litematics(dir.to_string_lossy().into_owned(), Some(4));
        let names: Vec<&str> = r.items.iter().map(|s| s.file_name.as_str()).collect();
        assert_eq!(names.len(), 2, "应只收录 .litematic，实际 {names:?}");
        assert!(names.contains(&"a.litematic"));
        assert!(names.contains(&"c.LITEMATIC"), "扩展名匹配应忽略大小写");
        assert_eq!(r.skipped_dirs, 0, "正常目录不应产生跳过");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mc_version_command_proxies_table() {
        assert!(!mc_version_name(3465).is_empty());
        assert_eq!(mc_version_name(-1), "");
    }

    /// 超长元数据必须在写入之前被拒绝。
    ///
    /// NBT 的字符串长度字段是 u16：超限时若只截断长度字段，写出的文件
    /// 记录长度与实际字节数不符，后续字段全部错位，而写入路径的终点是
    /// 覆盖用户的 .litematic。因此这里要求「明确报错」而不是「写出坏文件」。
    #[test]
    fn oversized_metadata_is_rejected_before_writing() {
        let too_long = "啊".repeat(20_001);
        let err = save_metadata(
            "__sm_not_exist__.litematic".into(),
            None,
            None,
            Some(too_long),
        )
        .expect_err("超长描述应被拒绝");
        assert!(
            err.contains("过长"),
            "错误信息应指出超长，实际：{err}"
        );
    }

    /// 三个字段都要校验，不能只检查描述。
    #[test]
    fn oversized_name_and_author_are_rejected_too() {
        let long = "x".repeat(20_001);
        for (label, name, author) in [
            ("名称", Some(long.clone()), None),
            ("作者", None, Some(long.clone())),
        ] {
            let err = save_metadata(
                "__sm_not_exist__.litematic".into(),
                name,
                author,
                None,
            )
            .expect_err(&format!("超长{label}应被拒绝"));
            assert!(
                err.contains("过长") && err.contains(label),
                "错误信息应指明是哪一项超长，实际：{err}"
            );
        }
    }

    /// 恰好等于上限时不应因长度被拒。
    ///
    /// 路径不存在，因此会以「文件不存在」失败——关键是不能再报「过长」，
    /// 否则说明上限被判成了 >=。
    #[test]
    fn metadata_at_exact_limit_is_not_rejected_for_length() {
        let ok = "x".repeat(20_000);
        let err = save_metadata(
            "__sm_not_exist__.litematic".into(),
            None,
            None,
            Some(ok),
        )
        .expect_err("文件不存在应报错");
        assert!(
            !err.contains("过长"),
            "恰好达上限不应被判超长，实际：{err}"
        );
        assert!(err.contains("不存在"), "应报文件不存在，实际：{err}");
    }

    use crate::blockdb::{BlockDb, BlockDef, LangEntry, Model, ModelElement, ModelFace, TextureInfo, Variant, VersionDef};
    use std::collections::HashMap;

    /// 造一个可序列化、可读回的最小方块库
    fn sample_db(pack: &str, dv: i32, block: &str, tex: &str, pixel: u8) -> BlockDb {
        let strings = vec![pack.to_string(), tex.to_string()];
        let model = Model {
            translucent: false,
            elements: vec![ModelElement {
                from: [0.0, 0.0, 0.0],
                to: [16.0, 16.0, 16.0],
                has_rotation: false,
                rot_kind: 0,
                origin: [0.0; 3],
                axis: 0,
                angle: 0.0,
                rescale: false,
                rot_vec: [0.0; 3],
                face_mask: 0b1,
                faces: vec![ModelFace {
                    texture: 1,
                    uv: [0.0, 0.0, 16.0, 16.0],
                    cullface: 0xFF,
                    tintindex: -1,
                    rotation: 0,
                }],
            }],
        };
        let version = VersionDef {
            name: pack.into(),
            data_version: dv,
            lang_idx: 0,
            texture_by_path: HashMap::from([(tex.to_string(), 0u32)]),
            blocks: vec![BlockDef {
                name: block.into(),
                variants: vec![Variant {
                    when: String::new(),
                    model_idx: 0,
                    rot_x_div90: 0,
                    rot_y_div90: 0,
                    uvlock: false,
                }],
                multipart: Vec::new(),
            }],
            langs: Vec::new(),
            is_overlay: false,
        };
        BlockDb {
            strings,
            langs: vec![vec![LangEntry {
                block_name: block.into(),
                en: "EN".into(),
                zh: "中文名".into(),
            }]],
            textures: vec![TextureInfo { w: 1, h: 1, offset: 0, len: 4, has_alpha: false }],
            models: vec![model],
            versions: vec![version],
            pixels: vec![pixel, pixel, pixel, 255],
        }
    }

    /// 界面状态里记下的日志等级要被启动流程采纳，认不出或缺键时回落 info。
    ///
    /// 这条守住的是「debug 用户从启动第一行起就有完整日志」：等级存在界面状态里，
    /// 键名或取值一变，这段就只会静默回落，用户再也拿不到启动阶段的调试日志。
    #[test]
    fn saved_log_level_comes_from_ui_state_and_falls_back_to_info() {
        assert_eq!(
            log_level_from_ui_state(r#"{"version":1,"logLevel":"debug"}"#),
            crate::run_log::Level::Debug
        );
        assert_eq!(
            log_level_from_ui_state(r#"{"version":1,"sidebarWidth":419}"#),
            crate::run_log::Level::Info,
            "没记过等级时用默认等级"
        );
        assert_eq!(
            log_level_from_ui_state(r#"{"version":1,"logLevel":"verbose"}"#),
            crate::run_log::Level::Info,
            "认不出的取值不该当成某个等级"
        );
        assert_eq!(
            log_level_from_ui_state("这不是 JSON"),
            crate::run_log::Level::Info,
            "界面状态坏了也不该影响启动"
        );
    }

    /// 仅接受 `YYYY-MM-DD`：这个值会被拼进归档文件名。
    #[test]
    fn local_date_must_be_a_plain_calendar_date() {
        assert!(is_local_date("2026-10-19"));
        assert!(!is_local_date("2026-1-9"), "位数不对就不是约定形状");
        assert!(!is_local_date("2026/10/19"), "分隔符必须是短横线");
        assert!(!is_local_date("../2026-10-19"), "路径片段不能被当成日期");
        assert!(!is_local_date(""), "空串不该通过");
    }

    /// 界面送来的等级/日期不合约定时要如实报错，不能悄悄按默认处理。
    ///
    /// 静默按默认的后果是：用户在界面上选了「调试」，日志却仍按 info 记，
    /// 而他以为已经开了——这类「以为开了其实没开」最难排查。
    #[test]
    fn log_commands_reject_values_that_do_not_match_the_contract() {
        let bad_level = configure_run_log("verbose".to_string(), "2026-10-19".to_string());
        assert!(bad_level.is_err(), "认不出的等级要报错");
        assert!(
            bad_level.unwrap_err().contains("verbose"),
            "报错要说清是哪个取值不认"
        );
        assert!(
            configure_run_log("info".to_string(), "2026/10/19".to_string()).is_err(),
            "日期形状不对要报错"
        );
        assert!(
            log_ui_event("loud".to_string(), "随便一条".to_string()).is_err(),
            "界面报的事件等级同样要校验"
        );
    }
}

// ---------------------------------------------------------------- 目录栏文件操作
//
// 这些命令都作用于用户在目录栏里勾选的文件。共同点：
// 路径来自前端，属于不可信输入，因此统一先经 file_ops::check_files
// 校验存在性与类型，再执行具体动作。
//
// 文件由用户在任意目录中选择，因此不能限制目录，但删除一律走回收站，
// 把「误删」的代价限制在可还原范围内。

/// 删除选中的文件（移入系统回收站，不永久删除）。
#[tauri::command]
pub fn delete_files(paths: Vec<String>) -> Result<FileOpOutcome, String> {
    let files = crate::file_ops::check_files(&paths).map_err(|e| e.to_string())?;
    crate::file_ops::delete_to_trash(&files).map_err(|e| e.to_string())?;
    let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
    crate::run_log::info(&format!(
        "删除到回收站 {} 个文件：{}",
        names.len(),
        names.join("、")
    ));
    Ok(FileOpOutcome {
        total_bytes: files.iter().map(|f| f.size).sum(),
        names,
        written: Vec::new(),
        dest_dir: String::new(),
    })
}

/// 把选中的文件复制到系统剪贴板，供用户在资源管理器里粘贴。
#[tauri::command]
pub fn copy_files_to_clipboard(paths: Vec<String>) -> Result<FileOpOutcome, String> {
    let files = crate::file_ops::check_files(&paths).map_err(|e| e.to_string())?;
    crate::file_ops::copy_paths_to_clipboard(&files).map_err(|e| e.to_string())?;
    Ok(FileOpOutcome {
        total_bytes: files.iter().map(|f| f.size).sum(),
        names: files.iter().map(|f| f.name.clone()).collect(),
        written: Vec::new(),
        dest_dir: String::new(),
    })
}

/// 把选中的文件复制到指定目录。同名不覆盖，改为追加序号。
#[tauri::command]
pub fn copy_files_to_dir(paths: Vec<String>, dest_dir: String) -> Result<FileOpOutcome, String> {
    let files = crate::file_ops::check_files(&paths).map_err(|e| e.to_string())?;
    let dest = PathBuf::from(&dest_dir);
    let written = crate::file_ops::copy_into(&files, &dest).map_err(|e| e.to_string())?;
    let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
    crate::run_log::info(&format!(
        "复制 {} 个文件到 {}：{}",
        names.len(),
        dest.display(),
        names.join("、")
    ));
    Ok(FileOpOutcome {
        total_bytes: files.iter().map(|f| f.size).sum(),
        names,
        written: written
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        dest_dir: dest.to_string_lossy().into_owned(),
    })
}

/// 在系统文件管理器中定位到一个文件（打开其所在目录并选中该文件）。
///
/// 只处理**一个**文件：资源管理器的「选中」语义只对一个目标有意义，
/// 传多个时行为不确定，因此这里明确取第一个由前端决定。
#[tauri::command]
pub fn reveal_in_file_manager(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    crate::file_ops::reveal_in_file_manager(p).map_err(|e| e.to_string())
}

/// 重命名投影文件，并把它的备份一并重新链接到新名字。
///
/// 只接受文件名本身（不含目录），校验见 `file_ops::normalize_file_name`。
/// 改名本身不改动内容，因此不需要为此新建备份；但要动的是备份的**关联**，
/// 否则改完名字后这个文件的备份会在「备份管理」里消失、也无法还原。
#[tauri::command]
pub fn rename_schematic(path: String, new_name: String) -> Result<RenameOutcomeDto, String> {
    let old = Path::new(&path);
    let name = crate::file_ops::normalize_file_name(&new_name).map_err(|e| e.to_string())?;
    let dest = crate::file_ops::rename_file(old, &name).map_err(|e| e.to_string())?;

    let root = crate::backup::backup_root(&exe_dir());
    // 重新链接失败时如实记录原因：只回一个失败提示而不落日志，
    // 事后完全无法判断备份到底动没动、动到哪一步
    let (relinked, relink_error) = match crate::backup::relink(&root, old, &dest) {
        Ok(n) => (n, None),
        Err(e) => (0, Some(e.to_string())),
    };
    let old_name = old
        .file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file_name = dest
        .file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.clone());
    let summary = match &relink_error {
        Some(e) => format!("重命名 {old_name} -> {file_name}（备份重新链接失败：{e}）"),
        None => format!("重命名 {old_name} -> {file_name}（重新链接备份 {relinked} 组）"),
    };
    crate::run_log::info(&summary);
    Ok(RenameOutcomeDto {
        path: dest.to_string_lossy().into_owned(),
        file_name,
        relinked,
        relink_error,
    })
}

// ---------------------------------------------------------------- 界面状态记忆

/// 界面状态文件名。内容是一段前端自己解释的 JSON。
const UI_STATE_FILE: &str = "ui-state.json";

/// 读取界面状态文件；文件不存在返回 `None`（首次启动的正常情况，不是错误）。
///
/// 与定位路径分开：路径依赖运行时的 AppHandle，而读写逻辑本身
/// （尤其是「不存在」与「读失败」的区分）是可以独立验证的。
fn read_ui_state_file(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取界面状态 {} 失败：{e}", path.display())),
    }
}

/// 原子写入界面状态文件。
///
/// 先写同目录的临时文件再改名：直接覆盖时若中途失败（磁盘满、进程被杀）
/// 会留下半截 JSON，下次启动解析失败就等于把用户所有界面设置清零。
/// 改成同目录内的改名在主流文件系统上是原子的，因此读到的要么是旧内容、
/// 要么是完整的新内容。
fn write_ui_state_file(path: &Path, json: &str) -> Result<(), String> {
    // 临时文件必须与目标同目录：跨目录改名不保证原子，甚至可能失败
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes())
        .map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| format!("替换 {} 失败：{e}", path.display()))
}

/// 应用配置目录。
///
/// 放在系统的应用配置目录（`%APPDATA%\<bundle-id>` 等）而不是程序目录：
/// 程序可能装在 Program Files 这类只读位置，而「记住界面」失败不该影响使用。
/// 用 Tauri 核心的 PathResolver 取，不需要额外插件。
fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("无法定位配置目录：{e}"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("无法创建配置目录 {}：{e}", dir.display()))?;
    Ok(dir)
}

/// 界面状态的存放路径。
fn ui_state_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join(UI_STATE_FILE))
}

/// 读取上次保存的界面状态；从未保存过时返回 None（不是错误）。
#[tauri::command]
pub fn load_ui_state(app: tauri::AppHandle) -> Result<Option<String>, String> {
    read_ui_state_file(&ui_state_path(&app)?)
}

/// 保存界面状态。
#[tauri::command]
pub fn save_ui_state(app: tauri::AppHandle, json: String) -> Result<(), String> {
    write_ui_state_file(&ui_state_path(&app)?, &json)
}

// ---------------------------------------------------------------- 版本库（基础库补全）

/// 「版本库」界面状态文件名。内容是前端自己解释的 JSON
/// （记住上次的目录与下载来源），与界面形态状态分文件存放。
const SETUP_STATE_FILE: &str = "setup-state.json";

/// 进度事件名，前端 `listen` 用。
const BASE_SETUP_PROGRESS_EVENT: &str = "base-setup-progress";

/// 「版本库」进度事件的有效载荷。
///
/// 阶段切换（下载→构建）本身也是信息：下载有字节数可画进度条，
/// 构建没有总量可报，只能靠阶段名让前端切换成不定进度形态。
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BaseSetupProgress {
    id: String,
    /// download / build / done
    stage: &'static str,
    received: u64,
    total: u64,
    /// 该版本失败时的原因；正常进展时为空串
    error: String,
}

fn emit_setup_progress(app: &tauri::AppHandle, p: BaseSetupProgress) {
    use tauri::Emitter;
    // 发送失败只说明窗口已关闭或前端还没监听，本身不该中断补版本流程
    if let Err(e) = app.emit(BASE_SETUP_PROGRESS_EVENT, p) {
        crate::run_log::warn(&format!("发送版本库进度事件失败：{e}"));
    }
}

/// 基础库的当前状态：已就绪的版本清单与实体贴图库是否可用。
///
/// 前端据此决定要不要自动弹出「版本库」界面——库里一个版本都没有时
/// 方块检索和 3D 预览都没有数据可显示，必须先引导用户补版本。
#[tauri::command]
pub fn base_db_status(app: tauri::AppHandle) -> Result<BaseDbStatusDto, String> {
    let dir = crate::base_db::base_blocks_dir(&app).map_err(|e| e.to_string())?;
    let (base, _failures) = crate::base_db::load_base_block_db(&dir).map_err(|e| e.to_string())?;
    // 单个包损坏不在这里上报：损坏的版本包不会被合成进来，于是它只表现为
    // 「这个版本未就绪」，用户重新构建即可；具体原因由 open_db 记进日志。
    // 状态查询只回答「哪些版本就绪」，避免一个坏文件把整个版本列表都拖成错误。
    let versions = base
        .versions
        .iter()
        .map(|v| BaseVersionDto {
            name: v.name.clone(),
            data_version: v.data_version,
        })
        .collect();
    let entity_available = open_entity_db(&app).is_some();
    Ok(BaseDbStatusDto {
        versions,
        entity_available,
        supported: crate::base_db::SUPPORTED_RELEASES
            .iter()
            .map(|(dv, id)| BaseVersionDto {
                name: id.to_string(),
                data_version: *dv,
            })
            .collect(),
    })
}

/// 官方启动器的默认 .minecraft 路径；目录不存在时返回 None。
///
/// 只认真实存在的目录：给前端预填一个不存在的路径，用户一点「扫描」
/// 就是错误，不如让输入框空着等手选。
#[tauri::command]
pub fn default_minecraft_dir() -> Option<String> {
    crate::base_db::default_minecraft_dir()
        .filter(|p| p.is_dir())
        .map(|p| p.to_string_lossy().into_owned())
}

/// 扫描任意目录（含子目录）里所有能识别的客户端 jar（只读）。
///
/// 结果按数据版本升序；看着像客户端却读不出版本信息的文件逐条带原因返回，
/// 由前端决定怎么展示。模组一类没有 `version.json` 的 jar 会被静默跳过。
#[tauri::command]
pub fn scan_minecraft(dir: String) -> Result<McScanDto, String> {
    let scan = crate::base_db::scan_minecraft(Path::new(&dir)).map_err(|e| e.to_string())?;
    // 读不出版本信息的目录会被跳过，用户在清单里少看到一个版本时要能查到原因
    for w in &scan.warnings {
        crate::run_log::warn(&format!("扫描 Minecraft 目录时跳过：{w}"));
    }
    Ok(McScanDto {
        versions: scan
            .versions
            .into_iter()
            .map(|v| McVersionDto {
                id: v.id,
                data_version: v.data_version,
                jar_path: v.jar_path.to_string_lossy().into_owned(),
            })
            .collect(),
        warnings: scan.warnings,
    })
}

/// 准备基础库版本：有本地 jar 的直接用，没有的按来源下载，
/// 然后构建方块库单版本包、把实体贴图并入贴图库。
///
/// 下载与构建都是耗时的 IO/CPU 操作，放进阻塞线程池，
/// 不占用异步运行时；进度经 `base-setup-progress` 事件逐版本汇报。
#[tauri::command]
pub async fn setup_add_versions(
    app: tauri::AppHandle,
    source: SetupSource,
    versions: Vec<SetupVersionDto>,
) -> Result<SetupOutcomeDto, String> {
    tauri::async_runtime::spawn_blocking(move || SetupOutcomeDto {
        outcomes: run_setup(&app, source, versions),
    })
    .await
    .map_err(|e| format!("版本库任务执行失败：{e}"))
}

/// 逐版本执行补库，一个版本失败不影响其它版本。
///
/// 下载失败往往只影响单个版本（网络抖动、该版本在清单里不存在），
/// 整批中止会让已完成的版本也显得「没动静」，因此逐版本记结果。
fn run_setup(
    app: &tauri::AppHandle,
    source: SetupSource,
    versions: Vec<SetupVersionDto>,
) -> Vec<SetupVersionOutcomeDto> {
    // 全局前提失败（数据目录不可写等）对每个版本都是一样的，
    // 逐版本如实上报比只弹一个笼统错误更能说明影响面
    let dirs = (|| {
        AppResult::Ok((
            crate::base_db::base_blocks_dir(app)?,
            crate::base_db::base_jars_dir(app)?,
            crate::base_db::base_entities_path(app)?,
        ))
    })();
    let (blocks_dir, jars_dir, entities_path) = match dirs {
        Ok(d) => d,
        Err(e) => {
            let msg = e.to_string();
            return versions
                .iter()
                .map(|v| SetupVersionOutcomeDto {
                    id: v.id.clone(),
                    ok: false,
                    error: msg.clone(),
                    warning: None,
                })
                .collect();
        }
    };

    // 官方接口的清单只拉一次：同批版本共用，避免每版本重复下载几 MB 清单。
    // 清单拉不到时，需要下载的版本逐个报同一个错，有本地 jar 的照常处理。
    let need_download = versions.iter().any(|v| v.jar_path.is_none());
    let manifest: Result<Option<String>, String> =
        if need_download && matches!(source, SetupSource::Official) {
            crate::mc_download::fetch_manifest()
                .map(Some)
                .map_err(|e| e.to_string())
        } else {
            Ok(None)
        };

    let mut outcomes = Vec::with_capacity(versions.len());
    for v in &versions {
        let r = match &manifest {
            Ok(m) => setup_one(
                app,
                v,
                m.as_deref(),
                source,
                &blocks_dir,
                &jars_dir,
                &entities_path,
            ),
            Err(e) if v.jar_path.is_none() => Err(AppError::Parse(e.clone())),
            // 有本地 jar 的版本不受清单失败影响
            Err(_) => setup_one(app, v, None, source, &blocks_dir, &jars_dir, &entities_path),
        };
        outcomes.push(match r {
            Ok(warning) => {
                if let Some(code) = warning.as_deref() {
                    // 提示码本身是给界面做翻译的，日志里要写清楚是哪一步没成
                    crate::run_log::warn(&format!(
                        "准备 {} 完成，但有提示：{code}",
                        v.id
                    ));
                }
                crate::run_log::info(&format!("准备 {} 完成", v.id));
                SetupVersionOutcomeDto {
                    id: v.id.clone(),
                    ok: true,
                    error: String::new(),
                    warning,
                }
            }
            Err(e) => {
                // 用户报「某个版本准备不了」时，日志里得有版本号与原因
                crate::run_log::error(&format!("准备 {} 失败：{e}", v.id));
                SetupVersionOutcomeDto {
                    id: v.id.clone(),
                    ok: false,
                    error: e.to_string(),
                    warning: None,
                }
            }
        });
    }
    // 库内容变了，清掉缓存让下一次打开投影按新库重建
    invalidate_db();
    invalidate_entity_db();
    outcomes
}

/// 准备单个版本：取 jar（本地或下载）→ 构建方块包 → 并入实体贴图。
///
/// 返回需要提醒用户的提示码（见 [`fill_missing_langs`]）；正常的成功是 `Ok(None)`。
fn setup_one(
    app: &tauri::AppHandle,
    v: &SetupVersionDto,
    manifest: Option<&str>,
    source: SetupSource,
    blocks_dir: &Path,
    jars_dir: &Path,
    entities_path: &Path,
) -> AppResult<Option<String>> {
    let jar_path = match &v.jar_path {
        Some(p) => PathBuf::from(p),
        None => {
            let dest = jars_dir.join(format!("{}.jar", v.id));
            // 改名落盘的 jar 一定完整，已存在就直接复用
            if !dest.is_file() {
                let url = match manifest {
                    Some(m) => crate::mc_download::official_client_url(m, &v.id)?,
                    None => crate::mc_download::bmcl_client_url(&v.id),
                };
                let id = v.id.clone();
                crate::mc_download::download_to(&url, &dest, &mut |received, total| {
                    emit_setup_progress(
                        app,
                        BaseSetupProgress {
                            id: id.clone(),
                            stage: "download",
                            received,
                            total,
                            error: String::new(),
                        },
                    );
                })?;
            }
            dest
        }
    };

    emit_setup_progress(
        app,
        BaseSetupProgress {
            id: v.id.clone(),
            stage: "build",
            received: 0,
            total: 0,
            error: String::new(),
        },
    );
    let out = blocks_dir.join(format!("{}-{}.dat.z", v.data_version, v.id));
    // 语言表要一并喂给构建侧：客户端 jar 里只有 en_us.json（官方从 1.6 起把
    // 多语言放进了资源库），缺了它方块名整列没有中文，也就是界面只能显示英文。
    let mc_dir = v
        .mc_dir
        .as_ref()
        .map(PathBuf::from)
        .or_else(crate::base_db::default_minecraft_dir);
    let mut langs = match &mc_dir {
        Some(dir) => crate::base_db::find_lang_files(dir, &v.id),
        None => Vec::new(),
    };
    let warning = fill_missing_langs(v, manifest, mc_dir.as_deref(), source, &mut langs);
    crate::jar_file::build_pack_to_with_langs(
        &jar_path.to_string_lossy(),
        &out.to_string_lossy(),
        &v.id,
        v.data_version,
        false,
        langs,
    )?;

    let jar = crate::jar_file::JarFile::open(&jar_path.to_string_lossy())?;
    crate::entitydb_write::add_version(entities_path, &jar, v.data_version, &v.id)?;

    emit_setup_progress(
        app,
        BaseSetupProgress {
            id: v.id.clone(),
            stage: "done",
            received: 0,
            total: 0,
            error: String::new(),
        },
    );
    Ok(warning)
}

/// 客户端 jar 里自带的语言表键。
///
/// 这些既不需要、也无法从资源索引下载：官方把 `en_us` 留在 jar 里，
/// 索引里根本没有 `minecraft/lang/en_us.json` 这一条。
const LANGS_IN_JAR: [&str; 1] = ["en_us"];

/// 判断某语言的本地文件是否已经齐了。
fn has_lang(langs: &[(String, Vec<u8>)], lang: &str) -> bool {
    let want = format!("assets/minecraft/lang/{lang}.json");
    langs.iter().any(|(p, _)| p == &want)
}

/// 还需要联网补哪些语言表：**只算 jar 里没有的那些**。
///
/// 把 `en_us` 也算进来会让库完全正常时报出误报——它在 jar 里，资源索引里
/// 没有对应条目，永远补不上。
fn langs_to_download(langs: &[(String, Vec<u8>)]) -> Vec<&'static str> {
    crate::base_db::LANG_KEYS
        .iter()
        .copied()
        .filter(|lang| !LANGS_IN_JAR.contains(lang))
        .filter(|lang| !has_lang(langs, lang))
        .collect()
}

/// 把本机资源库里没有的语言表补上；返回需要提醒用户的提示码。
///
/// 客户端 jar 自带 `en_us.json`，**其余语言**（中文即在其中）只能从资源库取：
/// 本机 `.minecraft/assets` 为空时（例如从没启动过游戏）就得上网下载。
///
/// 取不到不算失败——库照样建得出来，只是名字回落成英文，因此返回提示码而不是
/// 错误，让界面如实说明原因，而不是让用户以为界面坏了。
fn fill_missing_langs(
    v: &SetupVersionDto,
    manifest: Option<&str>,
    mc_dir: Option<&Path>,
    source: SetupSource,
    langs: &mut Vec<(String, Vec<u8>)>,
) -> Option<String> {
    let missing = langs_to_download(langs);
    if missing.is_empty() {
        return None;
    }
    let req = crate::mc_download::LangDownload {
        version_id: &v.id,
        source: match source {
            SetupSource::Official => crate::mc_download::AssetSource::Official,
            SetupSource::Bmcl => crate::mc_download::AssetSource::Bmcl,
        },
        mc_dir,
        manifest,
        langs: &missing,
    };
    match crate::mc_download::download_lang_files(&req) {
        Ok(extra) => {
            langs.extend(extra);
            // 索引里没有请求的语言时也值得说一声：那说明这个版本确实不带它，
            // 用户以后不必再重试
            if missing.iter().all(|lang| has_lang(langs, lang)) {
                None
            } else {
                Some("lang-unavailable".to_string())
            }
        }
        Err(e) => {
            // 不能让失败消失：用户看到英文名时，得能查到是因为没取到语言表
            crate::run_log::warn(&format!(
                "版本 {} 的语言表未能取得，界面将显示英文名：{e}",
                v.id
            ));
            Some("lang-unavailable".to_string())
        }
    }
}

/// 读取「版本库」界面状态；从未保存过时返回 None（不是错误）。
#[tauri::command]
pub fn load_setup_state(app: tauri::AppHandle) -> Result<Option<String>, String> {
    read_ui_state_file(&config_dir(&app)?.join(SETUP_STATE_FILE))
}

/// 保存「版本库」界面状态。
#[tauri::command]
pub fn save_setup_state(app: tauri::AppHandle, json: String) -> Result<(), String> {
    write_ui_state_file(&config_dir(&app)?.join(SETUP_STATE_FILE), &json)
}

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

