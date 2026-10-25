//! 目录栏文件操作与备份管理。

use crate::app::backup;
use super::dto::{BackupEntryDto, RenameOutcomeDto, FileOpOutcome};
use super::shared::exe_dir;
use std::path::{Path, PathBuf};

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
    let files = crate::app::file_ops::check_files(&paths).map_err(|e| e.to_string())?;
    crate::app::file_ops::delete_to_trash(&files).map_err(|e| e.to_string())?;
    let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
    crate::app::run_log::info(&format!(
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
    let files = crate::app::file_ops::check_files(&paths).map_err(|e| e.to_string())?;
    crate::app::file_ops::copy_paths_to_clipboard(&files).map_err(|e| e.to_string())?;
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
    let files = crate::app::file_ops::check_files(&paths).map_err(|e| e.to_string())?;
    let dest = PathBuf::from(&dest_dir);
    let written = crate::app::file_ops::copy_into(&files, &dest).map_err(|e| e.to_string())?;
    let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
    crate::app::run_log::info(&format!(
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
    crate::app::file_ops::reveal_in_file_manager(p).map_err(|e| e.to_string())
}

/// 重命名投影文件，并把它的备份一并重新链接到新名字。
///
/// 只接受文件名本身（不含目录），校验见 `file_ops::normalize_file_name`。
/// 改名本身不改动内容，因此不需要为此新建备份；但要动的是备份的**关联**，
/// 否则改完名字后这个文件的备份会在「备份管理」里消失、也无法还原。
#[tauri::command]
pub fn rename_schematic(path: String, new_name: String) -> Result<RenameOutcomeDto, String> {
    let old = Path::new(&path);
    let name = crate::app::file_ops::normalize_file_name(&new_name).map_err(|e| e.to_string())?;
    let dest = crate::app::file_ops::rename_file(old, &name).map_err(|e| e.to_string())?;

    let root = crate::app::backup::backup_root(&exe_dir());
    // 重新链接失败时如实记录原因：只回一个失败提示而不落日志，
    // 事后完全无法判断备份到底动没动、动到哪一步
    let (relinked, relink_error) = match crate::app::backup::relink(&root, old, &dest) {
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
    crate::app::run_log::info(&summary);
    Ok(RenameOutcomeDto {
        path: dest.to_string_lossy().into_owned(),
        file_name,
        relinked,
        relink_error,
    })
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
    crate::app::run_log::info(&format!("还原 {name} <- {backup_path}"));
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
    crate::app::run_log::info(&format!("删除备份 {name}"));
    Ok(())
}
