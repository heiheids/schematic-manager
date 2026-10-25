//! 方块检索、图标渲染、实体贴图读取与方块替换。

use crate::app::backup;
use crate::block::db::BlockDb;
use crate::block::names::block_display_names;
use super::dto::{EntityTextureStatusDto, EntityTextureDto, ReplaceRuleDto, ReplaceOutcomeDto, BlockHitDto, BlockVersionDto, VersionOptionDto, BlockIconDto};
use super::shared::{exe_dir, open_db, open_entity_db};
use std::path::Path;

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
    let size = crate::render::icon::check_icon_size(size.unwrap_or(crate::render::icon::ICON_SIZE))
        .map_err(|e| e.to_string())?;

    Ok(states
        .into_iter()
        .map(|state| match crate::render::icon::render_data_url(db, vi, &state, size) {
            Ok(data_url) => BlockIconDto { state, data_url, error: String::new() },
            Err(e) => BlockIconDto { state, data_url: String::new(), error: e.to_string() },
        })
        .collect())
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

/// 检索结果转前端契约。检索与「列出全部」共用，两处各写一份字段搬运迟早会漏。
fn hit_dto(h: crate::block::search::BlockHit) -> BlockHitDto {
    BlockHitDto {
        name: h.name,
        zh: h.zh,
        en: h.en,
        versions: h
            .versions
            .into_iter()
            .map(|v| BlockVersionDto { index: v.index, data_version: v.data_version })
            .collect(),
    }
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
    Ok(crate::block::search::search(db, &query, limit.unwrap_or(200))
        .into_iter()
        .map(hit_dto)
        .collect())
}

/// 列出方块库中的全部方块，按方块 ID 升序。
///
/// 供方块选择列表在用户还没输入关键字时使用：那里需要一个默认可选范围。
#[tauri::command]
pub fn list_blocks(app: tauri::AppHandle) -> Result<Vec<BlockHitDto>, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    Ok(crate::block::search::list_all(&loaded.db)
        .into_iter()
        .map(hit_dto)
        .collect())
}

/// 列出方块库中的全部版本
#[tauri::command]
pub fn list_block_versions(app: tauri::AppHandle) -> Result<Vec<VersionOptionDto>, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    Ok(crate::block::search::version_options(db)
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
    let sc = crate::schematic::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;
    let summary = sc.summary();
    let vi = db
        .pick_version(summary.data_version)
        .ok_or_else(|| "方块库中没有可用版本".to_string())?;
    let info = crate::schematic::model::DbBlockInfo { db, version: &db.versions[vi] };
    let rule: crate::schematic::replace::ReplaceRule = rule.into();
    Ok(ReplaceOutcomeDto {
        replaced: crate::schematic::replace::count_matches(&sc, &rule, &info),
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
    let mut sc = crate::schematic::litematic::Schematic::load(&schematic_path).map_err(|e| e.to_string())?;
    let summary = sc.summary();
    let vi = db
        .pick_version(summary.data_version)
        .ok_or_else(|| "方块库中没有可用版本".to_string())?;
    let info = crate::schematic::model::DbBlockInfo { db, version: &db.versions[vi] };
    let rule: crate::schematic::replace::ReplaceRule = rule.into();

    let out = crate::schematic::replace::replace_blocks(&mut sc, &rule, &info, 0);
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
    crate::app::run_log::info(&replace_log_line(
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

#[cfg(test)]
#[path = "../tests/commands/blocks.rs"]
mod tests;
