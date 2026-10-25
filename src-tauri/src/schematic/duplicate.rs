//! 重复投影文件筛查。
//!
//! 判定「重复」只看文件内容，不看文件名：同一份投影常被改名保存多次，
//! 名称相同也可能是不同内容。
//!
//! 三轮筛选，逐步收窄，避免对每个文件都做全量比较：
//! 先按大小分组（读取目录时已经拿到大小），组内再按内容哈希分组，
//! 最后对同哈希的文件逐字节比对确认——哈希只是过滤手段，
//! 结论必须由逐字节比对得出，否则哈希碰撞会把不同文件误判为重复。

use std::io::Read;
use std::path::{Path, PathBuf};

/// 筛查结果
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DuplicateScan {
    /// 内容相同的文件组
    pub groups: Vec<DuplicateGroup>,
    /// 读取失败、因而未能参与判定的文件及原因。
    ///
    /// 必须上报而不是丢弃：这些文件既不能判为「有重复」，
    /// 也不能被当成「与谁都不重复」——对用户来说两者含义完全不同。
    pub unreadable: Vec<(PathBuf, String)>,
}

/// 一组内容完全相同的文件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGroup {
    pub size: u64,
    /// 组内文件路径，按路径排序，结果稳定
    pub paths: Vec<PathBuf>,
}

/// 参与筛查的文件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub path: PathBuf,
    pub size: u64,
}

/// FNV-1a 64 位哈希。
///
/// 只用于把候选集合收窄，不承担正确性——最终由逐字节比对定论。
fn hash_reader<R: Read>(mut r: R) -> std::io::Result<u64> {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for b in &buf[..n] {
            h ^= *b as u64;
            h = h.wrapping_mul(PRIME);
        }
    }
    Ok(h)
}

fn content_hash(path: &Path) -> std::io::Result<u64> {
    let f = std::fs::File::open(path)?;
    hash_reader(std::io::BufReader::new(f))
}

/// 逐字节比较两个文件是否完全相同。
///
/// 大小已在调用前确认一致，这里只比内容。
fn same_content(a: &Path, b: &Path) -> std::io::Result<bool> {
    let fa = std::fs::File::open(a)?;
    let fb = std::fs::File::open(b)?;
    let mut ra = std::io::BufReader::new(fa);
    let mut rb = std::io::BufReader::new(fb);
    let mut ba = [0u8; 64 * 1024];
    let mut bb = [0u8; 64 * 1024];
    loop {
        let na = ra.read(&mut ba)?;
        let nb = rb.read(&mut bb)?;
        if na != nb {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
        if ba[..na] != bb[..nb] {
            return Ok(false);
        }
    }
}

/// 找出内容相同的文件组。
///
/// 只返回至少两个成员的组。某个文件读不了不应让整次筛查失败，
/// 但也不能把它当成「与谁都不重复」——对用户来说两者含义完全不同，
/// 因此读不了的文件单独列入 unreadable 上报。
pub fn find(candidates: &[Candidate]) -> DuplicateScan {
    use std::collections::HashMap;

    // 第一轮：按大小分组，大小不同的必然不重复
    let mut by_size: HashMap<u64, Vec<&Candidate>> = HashMap::new();
    for c in candidates {
        by_size.entry(c.size).or_default().push(c);
    }

    let mut out: Vec<DuplicateGroup> = Vec::new();
    let mut unreadable: Vec<(PathBuf, String)> = Vec::new();

    for (size, group) in by_size {
        if group.len() < 2 {
            continue;
        }
        // 第二轮：按内容哈希分组
        let mut by_hash: HashMap<u64, Vec<&Candidate>> = HashMap::new();
        for c in group {
            match content_hash(&c.path) {
                Ok(h) => by_hash.entry(h).or_default().push(c),
                Err(e) => unreadable.push((c.path.clone(), e.to_string())),
            }
        }

        // 第三轮：同哈希的逐字节比对，确认后才算重复
        for (_, bucket) in by_hash {
            if bucket.len() < 2 {
                continue;
            }
            let mut remaining: Vec<&Candidate> = bucket;
            while !remaining.is_empty() {
                let head = remaining.remove(0);
                let mut same: Vec<PathBuf> = vec![head.path.clone()];
                let mut failed: Vec<(PathBuf, String)> = Vec::new();
                remaining.retain(|other| match same_content(&head.path, &other.path) {
                    Ok(true) => {
                        same.push(other.path.clone());
                        false // 已归入本组，从待处理中移除
                    }
                    Ok(false) => true,
                    Err(e) => {
                        // 比对失败（打开/读取出错）：该文件既不能确认重复，
                        // 也不能算作「与谁都不重复」，因此移出待处理集合并
                        // 单独上报，不做重试——同一次筛查里重试也只会同样失败。
                        failed.push((other.path.clone(), e.to_string()));
                        false
                    }
                });
                unreadable.extend(failed);
                if same.len() > 1 {
                    same.sort();
                    out.push(DuplicateGroup { size, paths: same });
                }
            }
        }
    }

    // 组间按路径排序，结果稳定可复现
    out.sort_by(|a, b| a.paths.cmp(&b.paths));
    unreadable.sort();
    DuplicateScan { groups: out, unreadable }
}

#[cfg(test)]
#[path = "../tests/schematic/duplicate.rs"]
mod tests;
