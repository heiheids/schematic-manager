//! 改动投影文件前的自动备份。
//!
//! 备份目录放在程序所在目录的 backups/ 下，与 legacy-cpp 的行为保持一致：
//! 用户在有问题的改动后能直接找到原件，而不是去猜测备份去了哪里。

use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 备份根目录：<程序所在目录>/backups
///
/// exe_dir 由调用方传入，便于测试时注入临时目录。
pub fn backup_root(exe_dir: &Path) -> PathBuf {
    exe_dir.join("backups")
}

/// 备份文件名的唯一后缀。
///
/// 用纳秒而非毫秒：同一毫秒内的两次备份若得到相同文件名，
/// std::fs::copy 会直接覆盖，静默丢掉一份备份。
///
/// 必须是**纯数字**：文件名以 "__" 分隔字段，后缀里若含下划线，
/// 解析时会把它当成字段分隔符，导致所有真实备份都列不出来。
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    now.as_nanos().to_string()
}

/// 备份结果
#[derive(Debug, Clone)]
pub struct BackupOutcome {
    pub backup_path: PathBuf,
}

/// 把 src 复制到 backups/<文件名主干>/<主干>__<时间戳>__<操作>.litematic
///
/// 每个源文件一个子目录，避免同名文件互相覆盖；
/// 文件名里带上操作名与时间戳，便于在资源管理器里直接辨认。
pub fn create(exe_dir: &Path, src: &Path, op: &str) -> AppResult<BackupOutcome> {
    let stem = src
        .file_stem()
        .map(|v| v.to_string_lossy().into_owned())
        .ok_or_else(|| crate::error::AppError::Parse("源文件没有文件名主干".into()))?;

    // 分组目录必须把「原始所在目录」也区分开：不同目录下同名文件
    // （A/demo.litematic 与 B/demo.litematic）若共用一个分组，
    // 还原时只比对文件名主干会把两个不同文件判为同源，进而覆盖错对象。
    let dir = group_dir(exe_dir, src)?;
    std::fs::create_dir_all(&dir)?;

    // 记录原始完整路径。legacy-cpp 用一个 index.tsv 记 origPath，
    // 这里改为每组一个旁文件——省掉索引与实际文件不一致的失配面。
    std::fs::write(dir.join(ORIG_PATH_FILE), orig_path_of(src))?;

    let file_name = format!("{stem}__{}__{op}.litematic", timestamp());
    let dst = dir.join(file_name);
    std::fs::copy(src, &dst)?;

    Ok(BackupOutcome { backup_path: dst })
}

/// 两个路径是否指向同一个实际位置。
///
/// 用规范化结果比较，而不是自己比小写字符串：在大小写敏感的系统上
/// `A/b` 与 `A/B` 是两个不同的位置，按字符串小写比较会误判成同一个。
pub fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        // 规范化不了（路径不存在）时退回原样比较：此时「相等」最多意味着
        // 写法一致，不会把两个不同目录误判成一个
        _ => a == b,
    }
}

/// 记录原始路径的旁文件名
const ORIG_PATH_FILE: &str = ".orig_path";

/// FNV-1a 64 位哈希，用于把父目录折叠成分组名后缀
fn hash_path(s: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// 原始路径的规范化文本，用于跨平台比较。
///
/// 统一小写并去掉尾部分隔符：Windows 路径大小写不敏感，
/// `C:\A` 与 `c:\a` 必须视为同一处。
fn orig_path_of(src: &Path) -> String {
    let abs = std::fs::canonicalize(src).unwrap_or_else(|_| {
        // 文件本身已不存在时（重命名之后要读旧路径的记录）无法规范化它，
        // 退而规范化它的父目录再拼上文件名。
        //
        // 父目录也必须规范化：canonicalize 在 Windows 上返回的是带
        // `\\?\` 前缀的最终路径，而这里若直接拼原样的父目录字符串，
        // 得到的形式与 create() 当时写下的记录不同，比对必然不相等——
        // 表现就是重命名后怎么都找不到旧备份的分组。
        let dir = src.parent().unwrap_or_else(|| Path::new(""));
        let canon_dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        canon_dir.join(src.file_name().unwrap_or_default())
    });
    let mut t = abs.to_string_lossy().replace('/', "\\").to_lowercase();
    while t.len() > 3 && t.ends_with('\\') {
        t.pop();
    }
    t
}

fn group_dir(exe_dir: &Path, src: &Path) -> AppResult<PathBuf> {
    group_dir_under(&backup_root(exe_dir), src)
}

/// 与 `group_dir` 相同，但直接给出备份根目录。
///
/// 拆分出来是因为重新链接（见 `relink`）拿到的是备份根，不是 exe 所在目录。
fn group_dir_under(root: &Path, src: &Path) -> AppResult<PathBuf> {
    let stem = src
        .file_stem()
        .map(|v| v.to_string_lossy().into_owned())
        .ok_or_else(|| crate::error::AppError::Parse("源文件没有文件名主干".into()))?;
    let parent = src
        .parent()
        .map(|p| p.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let key = hash_path(&parent);
    Ok(root.join(format!("{stem}_{key:016x}")))
}

/// 文件重命名后，把它的备份重新链接到新名字上。
///
/// 备份与文件的关联有三处依赖旧名字，缺一处都会让备份「消失」：
///   1. 分组目录名 `<主干>_<父目录哈希>`——列表按主干筛选，
///      不改名则「备份管理」里看不到这个文件的任何备份；
///   2. 分组内的 `.orig_path`——还原前用它校验同源，
///      不改则还原会被拒绝（提示「不同源」）；
///   3. 备份文件名 `<主干>__<时间戳>__<操作>.litematic`——
///      列表从文件名解析主干，不改则筛选同样匹配不上。
///
/// 返回重新链接的分组数。目标分组已存在时（例如同名文件曾存在、
/// 其备份仍在）把备份并入该分组：它们现在指向同一个路径，
/// 合成一组才能让「按路径找备份」保持一一对应。
pub fn relink(root: &Path, old: &Path, new: &Path) -> AppResult<usize> {
    let old_stem = old
        .file_stem()
        .map(|v| v.to_string_lossy().into_owned())
        .ok_or_else(|| AppError::Parse("源文件没有文件名主干".into()))?;
    let new_stem = new
        .file_stem()
        .map(|v| v.to_string_lossy().into_owned())
        .ok_or_else(|| AppError::Parse("目标文件没有文件名主干".into()))?;
    let old_key = orig_path_of(old);
    let new_key = orig_path_of(new);
    let dest_dir = group_dir_under(root, new)?;

    // 备份根不存在是正常的（还没备份过任何文件），但「存在却读不到」
    // （权限、被占用）不能当成「没有备份」：那会让重命名悄悄跳过重新链接，
    // 用户直到发现还原失败才知道备份没跟上
    let groups = match std::fs::read_dir(root) {
        Ok(g) => g,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => {
            return Err(AppError::Parse(format!(
                "无法读取备份目录 {}：{e}",
                root.display()
            )))
        }
    };
    let mut relinked = 0;
    for g in groups.flatten() {
        if !g.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let src_dir = g.path();
        // 只处理确实属于旧文件的分组：同名不同目录的文件各有一组，
        // 按路径比对才不会把别人的备份改到新名字上
        let recorded = std::fs::read_to_string(src_dir.join(ORIG_PATH_FILE))
            .map(|v| v.trim().to_string())
            .unwrap_or_default();
        if recorded != old_key {
            continue;
        }
        // 主干没变（例如只调整了大小写）时改名毫无必要，
        // 强行把目录搬来搬去反而会与自身冲突
        if old_stem == new_stem {
            continue;
        }
        // 只差大小写时两个分组目录**可能**是同一个目录。此时不能建目录、
        // 更不能删旧目录：那会把刚写好的 .orig_path 一并删掉，备份就此无法还原。
        //
        // 判断交给文件系统，而不是自己比小写字符串：小写相等只说明「可能是同一
        // 个」，在大小写敏感的系统上 `demo_h` 与 `DEMO_h` 确实各是一个目录，
        // 按字符串判成同一个就会跳过建目录，随后的 rename 必然失败。
        let same_dir = dest_dir.exists() && same_path(&src_dir, &dest_dir);

        // 先把要搬的文件全部列出来（含撞名检查），再动任何东西。
        // 一边枚举一边搬会在中途失败时留下「已搬走、但新分组还没有
        // .orig_path」的备份——那种备份既列得出、又还原不了。
        let Ok(files) = std::fs::read_dir(&src_dir) else {
            // 读不到这一组就整组不动：它仍带着旧的 .orig_path，
            // 至少还能还原回旧路径，比搬一半强
            continue;
        };
        let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
        for f in files.flatten() {
            let p = f.path();
            let Some(name) = p.file_name().map(|v| v.to_string_lossy().into_owned()) else {
                continue;
            };
            let Some((stem, created_ns, op)) = parse_backup_name(&name) else {
                continue; // .orig_path 之类的旁文件由下面统一处理
            };
            if stem != old_stem {
                continue;
            }
            let target = dest_dir.join(format!("{new_stem}__{created_ns}__{op}.litematic"));
            // 纳秒级时间戳本不会撞名；真撞上说明这个备份已经在目标分组里
            // （例如重命名做了两次）。此时**不猜**：既不能覆盖目标那份，
            // 也不能自作主张删掉源这份。中止这次重新链接并说明原因。
            //
            // 中止发生在「只枚举、未改动」的阶段，因此这一组原封不动、
            // 之前已完成的组也都各自完整，不会留下半搬状态。
            let only_case = p.to_string_lossy().to_lowercase()
                == target.to_string_lossy().to_lowercase();
            if target.exists() && !only_case {
                return Err(AppError::Parse(format!(
                    "备份 {} 在目标分组中已存在同名文件，已中止重新链接以免覆盖",
                    name
                )));
            }
            moves.push((p, target));
        }
        // 这一组里没有属于旧名字的备份：不动它，也不留下空目录
        if moves.is_empty() {
            continue;
        }

        if !same_dir {
            std::fs::create_dir_all(&dest_dir)?;
        }
        // 先建立新分组的关联，再搬文件：这样即便后面某一步失败，
        // 已经搬过去的备份也仍然能还原（它们的 .orig_path 已指向新路径），
        // 留在原地的备份则继续指向旧路径。两边都不会变成无法还原的孤儿。
        std::fs::write(dest_dir.join(ORIG_PATH_FILE), &new_key)?;
        for (from, to) in moves {
            std::fs::rename(&from, &to)?;
        }
        if !same_dir {
            std::fs::remove_file(src_dir.join(ORIG_PATH_FILE))?;
            // 旧分组里若还有没搬走的文件，目录删不掉。这不该被忽略：
            // 那些文件既不在新分组、又带着旧的 .orig_path，用户会看到
            // 一个永远还原不了的备份条目，却完全不知道原因。
            if let Err(e) = std::fs::remove_dir(&src_dir) {
                return Err(AppError::Parse(format!(
                    "备份分组未能清理干净 {}：{e}",
                    src_dir.display()
                )));
            }
        }
        relinked += 1;
    }
    Ok(relinked)
}

/// 一条备份记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupEntry {
    pub path: PathBuf,
    /// 原始文件的完整路径。还原时必须比对它，只看文件名主干会把
    /// 不同目录下的同名文件误判为同一文件。
    pub orig_path: String,
    /// 原始文件的文件名主干
    pub stem: String,
    /// 触发备份的操作名
    pub op: String,
    /// 备份时间（纳秒时间戳，由 timestamp() 产生）
    pub created_ns: u64,
    pub size: u64,
}

/// 解析备份文件名：<主干>__<时间戳>__<操作>.litematic
///
/// 从右侧切分：主干本身可能含有 "__"，从左侧切会把名字切坏。
fn parse_backup_name(name: &str) -> Option<(String, u64, String)> {
    let base = name.strip_suffix(".litematic")?;
    let parts: Vec<&str> = base.rsplitn(3, "__").collect();
    if parts.len() != 3 {
        return None;
    }
    let op = parts[0].to_string();
    let created_ns = parts[1].parse::<u64>().ok()?;
    let stem = parts[2].to_string();
    if stem.is_empty() || op.is_empty() {
        return None;
    }
    Some((stem, created_ns, op))
}

/// 列出全部备份，按时间倒序（最新的在前）。
///
/// 采用直接扫描目录而非维护索引文件：索引与实际文件容易不一致，
/// 一旦不一致就会出现「备份存在但列表为空」这类问题。
pub fn list(root: &Path) -> Vec<BackupEntry> {
    let mut out = Vec::new();
    let Ok(groups) = std::fs::read_dir(root) else { return out };
    for g in groups.flatten() {
        let Ok(ft) = g.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(g.path()) else { continue };
        for f in files.flatten() {
            let path = f.path();
            let Some(name) = path.file_name().map(|v| v.to_string_lossy().into_owned()) else {
                continue;
            };
            let Some((stem, created_ns, op)) = parse_backup_name(&name) else { continue };
            let size = f.metadata().map(|m| m.len()).unwrap_or(0);
            // 原始路径来自分组目录下的旁文件；缺失时留空，
            // 由还原前的校验拒绝，而不是默认放行
            let orig_path = path
                .parent()
                .and_then(|d| std::fs::read_to_string(d.join(ORIG_PATH_FILE)).ok())
                .map(|v| v.trim().to_string())
                .unwrap_or_default();
            out.push(BackupEntry { path, orig_path, stem, op, created_ns, size });
        }
    }
    // 时间戳相同（同纳秒级）时按路径兜底排序，保证结果稳定
    out.sort_by(|a, b| b.created_ns.cmp(&a.created_ns).then_with(|| a.path.cmp(&b.path)));
    out
}

/// 把备份还原到指定位置。
///
/// 还原前会先检查备份文件确实存在：否则会静默产生一个空目标文件。
pub fn restore(backup: &Path, target: &Path) -> AppResult<()> {
    if !backup.is_file() {
        return Err(AppError::Parse(format!("备份文件不存在：{}", backup.display())));
    }
    std::fs::copy(backup, target)?;
    Ok(())
}

/// 删除一条备份，并在分组内已无备份时清理该分组目录。
///
/// 顺序很重要：先删备份文件，**再**判断分组是否为空，最后才删记录原始路径的
/// 旁文件与目录。同一次备份的多个副本共用一个分组和一份 .orig_path，若在删除
/// 备份前就无条件删掉 .orig_path，同组其余备份会因 matches_target 读不到记录
/// 而永远无法还原。
pub fn delete(backup: &Path) -> AppResult<()> {
    if !backup.exists() {
        return Err(AppError::Parse(format!("备份文件不存在：{}", backup.display())));
    }
    std::fs::remove_file(backup)?;
    let Some(dir) = backup.parent() else { return Ok(()) };

    // 分组里还有别的备份时到此为止：.orig_path 是整组共用的，不能删。
    // 判空时必须把 .orig_path 本身排除掉，否则它会让目录看起来永远非空。
    let remaining = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .any(|e| e.file_name() != std::ffi::OsStr::new(ORIG_PATH_FILE)),
        // 读不到目录时不敢当作「空」：宁可留下空分组，也不要误删同组记录
        Err(_) => true,
    };
    if remaining {
        return Ok(());
    }

    // 只剩记录原始路径的旁文件：连同目录一起清理，避免 backups 下堆积空目录。
    let _ = std::fs::remove_file(dir.join(ORIG_PATH_FILE));
    let _ = std::fs::remove_dir(dir);
    Ok(())
}

/// 备份与目标文件是否同源（文件名主干一致）。
///
/// 还原是覆盖写，若允许跨文件还原，用户很容易把 A 的备份盖到 B 上，
/// 而且覆盖后原内容只剩备份可寻。因此还原前必须校验。
pub fn matches_target(backup: &Path, target: &Path) -> bool {
    let Some(name) = backup.file_name().map(|v| v.to_string_lossy().into_owned()) else {
        return false;
    };
    if parse_backup_name(&name).is_none() {
        return false;
    }
    // 比对完整原始路径，而不是只看文件名主干：
    // A/demo.litematic 与 B/demo.litematic 主干相同却是两个文件，
    // 只比主干会把 A 的备份还原到 B 上。
    let Some(dir) = backup.parent() else { return false };
    let Ok(recorded) = std::fs::read_to_string(dir.join(ORIG_PATH_FILE)) else {
        return false; // 没有记录就不放行
    };
    let recorded = recorded.trim();
    if recorded.is_empty() {
        return false;
    }
    recorded == orig_path_of(target)
}

/// 路径是否位于指定根目录之内。
///
/// 删除与还原的参数来自前端，必须限定在备份目录内，否则命令层就成了
/// 一个可以删除任意文件的入口。
///
/// 比较前先规范化：否则 `backups/../other` 之类的路径会绕过检查。
pub fn is_inside(root: &Path, path: &Path) -> bool {
    let norm = |p: &Path| -> PathBuf {
        let mut out = PathBuf::new();
        for seg in p.components() {
            match seg {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::CurDir => {}
                other => out.push(other.as_os_str()),
            }
        }
        out
    };
    norm(path).starts_with(norm(root))
}

#[cfg(test)]
#[path = "../tests/app/backup.rs"]
mod tests;
