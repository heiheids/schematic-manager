//! 投影浏览与分析：扫描目录、摘要、方块统计、场景构建、另存与元数据保存。

use crate::app::backup;
use crate::block::names::{block_display_names, entity_display_names, tile_display_names};
use crate::schematic::litematic::{self, Summary};
use super::dto::{SchematicSummary, BlockCount, EntityCount, EntityInstanceDto, RegionSummary, ScanResult, SceneDto, SaveOutcome, AnalysisResult};
use super::shared::{SCAN_MAX_DEPTH, SkipReport, collect_directory, exe_dir, open_db};
use std::path::{Path, PathBuf};

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
    let root_path = Path::new(&dir);
    let root = collect_directory(root_path, root_path, depth, &mut found, &mut skipped);
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
        root,
    }
}

/// 完整分析：解码全部方块并统计。
#[tauri::command]
pub fn analyze_schematic(app: tauri::AppHandle, path: String) -> Result<AnalysisResult, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let sc = litematic::Schematic::load(&path).map_err(|e| e.to_string())?;
    Ok(analyze_loaded(&sc, db))
}

/// 对已载入的投影生成分析结果，便于在不创建应用句柄的情况下验证区域与统计。
fn analyze_loaded(sc: &litematic::Schematic, db: &crate::block::db::BlockDb) -> AnalysisResult {
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

    AnalysisResult {
        distinct_block_types: types.len(),
        distinct_states: blocks.len(),
        blocks,
        entities,
        tile_entities: tiles,
        regions: sc
            .regions
            .iter()
            .map(|r| RegionSummary {
                name: r.name.clone(),
                position: r.pos,
                size: r.size,
            })
            .collect(),
        entity_instances,
        non_air_total: a.non_air_total,
        air_total: a.air_total,
    }
}

/// 组装前端 Three.js 渲染所需的场景。
///
/// 方块库作为应用资源随包分发，路径由这里解析，
/// 前端不必知道它在哪个目录，也不必关心各平台的资源布局差异。
#[tauri::command]
pub fn build_scene(
    app: tauri::AppHandle,
    schematic_path: String,
    region_name: Option<String>,
) -> Result<SceneDto, String> {
    let loaded = open_db(&app).map_err(|e| e.to_string())?;
    let db = &loaded.db;
    let scene = match region_name {
        Some(name) => crate::render::scene::build_region_with_db(&schematic_path, db, &name),
        None => crate::render::scene::build_with_db(&schematic_path, db),
    };
    scene.map(SceneDto::from).map_err(|e| e.to_string())
}

/// 把当前投影另存为一个新文件，并把未保存的元数据改动一并写进去。
///
/// 与 save_metadata 的区别：那个写回原文件并先备份原件；这个的产物是一个
/// 全新文件，原文件一个字节都不动，因此既不需要备份、也不该产生备份——
/// 「另存为」本就是在保留原稿的前提下再存一份。
///
/// 目标路径已存在时直接报错而不是覆盖：用户是拿它来选新名字的，
/// 覆盖掉一个已有投影属于不可接受的意外。
#[tauri::command]
pub fn save_schematic_as(
    source_path: String,
    target_path: String,
    name: Option<String>,
    author: Option<String>,
    description: Option<String>,
) -> Result<String, String> {
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

    let src = PathBuf::from(&source_path);
    if !src.is_file() {
        return Err(format!("文件不存在：{source_path}"));
    }
    let dst = PathBuf::from(&target_path);
    if dst.exists() {
        return Err(format!("目标文件已存在：{target_path}"));
    }
    // 目标目录必须已在：保存对话框允许用户新建目录，但若那一步失败，
    // 这里要给出明确原因，而不是一句笼统的写入失败
    if let Some(dir) = dst.parent() {
        if !dir.is_dir() {
            return Err(format!("目标目录不存在：{}", dir.display()));
        }
    }

    let mut sc = litematic::Schematic::load(&source_path).map_err(|e| e.to_string())?;
    if let Some(v) = name {
        sc.set_meta_name(&v);
    }
    if let Some(v) = author {
        sc.set_meta_author(&v);
    }
    if let Some(v) = description {
        sc.set_meta_description(&v);
    }
    sc.save_to(&target_path).map_err(|e| format!("写入失败：{e}"))?;

    let file_name = dst
        .file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    crate::app::run_log::info(&format!("另存为 {file_name}：{source_path} -> {target_path}"));

    Ok(target_path)
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
    crate::mc::ver::version_name_from_data_version(data_version)
}

#[cfg(test)]
#[path = "../tests/commands/schematic.rs"]
mod tests;
