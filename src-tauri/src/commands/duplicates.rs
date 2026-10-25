//! 重复投影筛查：按大小、内容摘要与逐字节比对找出副本。

use super::dto::{DuplicateGroupDto, DuplicateScanDto, UnreadableFileDto};
use super::shared::{SCAN_MAX_DEPTH, SkipReport, collect_litematics};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    let mut candidates: Vec<crate::schematic::duplicate::Candidate> = Vec::new();
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
                    candidates.push(crate::schematic::duplicate::Candidate { path, size: m.len() });
                }
                // 拿不到大小就无法参与「先按大小分组」。如实上报，而不是
                // 当成 0 字节：按 0 分组会让读不出来的文件与真正的空文件混作一堆。
                Err(e) => unreadable.push((path, e.to_string())),
            }
        }
    }

    let r = crate::schematic::duplicate::find(&candidates);
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

#[cfg(test)]
#[path = "../tests/commands/duplicates.rs"]
mod tests;
