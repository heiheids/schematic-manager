//! 基础版本库：方块与实体贴图的底座数据，以及本机 Minecraft 版本的发现。
//!
//! 方块库与实体贴图库不随安装包分发，而是由用户选择本机已装的客户端
//! jar 或在线下载，逐版本构建成单版本包，存放在应用数据目录的 `base/`
//! 下。本模块负责三件事：给出「支持补齐哪些正式版」的清单、把磁盘上的
//! 单版本包合成一个基础方块库、扫描 .minecraft 找出已装版本。

use crate::block::db::BlockDb;
use crate::error::{AppError, AppResult};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 支持补齐的正式版：(数据版本, 版本号)，按数据版本严格升序。
///
/// 清单取自 legacy-cpp/data/pack_report.json 的构建结果，即历史上实际
/// 能完整构建方块库的正式版集合。不能从 mc::ver::DATA_VERSIONS 反推：
/// 快照也占数据版本号，一个数据版本可能对应多个版本号，且 1.20.3/
/// 1.20.5 与 26.x 段的正式版在那里没有可区分的条目。
pub const SUPPORTED_RELEASES: &[(i32, &str)] = &[
    (1631, "1.13.2"),
    (1952, "1.14"),
    (1957, "1.14.1"),
    (1963, "1.14.2"),
    (1968, "1.14.3"),
    (1976, "1.14.4"),
    (2225, "1.15"),
    (2227, "1.15.1"),
    (2230, "1.15.2"),
    (2566, "1.16"),
    (2567, "1.16.1"),
    (2578, "1.16.2"),
    (2580, "1.16.3"),
    (2584, "1.16.4"),
    (2586, "1.16.5"),
    (2724, "1.17"),
    (2730, "1.17.1"),
    (2860, "1.18"),
    (2865, "1.18.1"),
    (2975, "1.18.2"),
    (3105, "1.19"),
    (3117, "1.19.1"),
    (3120, "1.19.2"),
    (3218, "1.19.3"),
    (3337, "1.19.4"),
    (3463, "1.20"),
    (3465, "1.20.1"),
    (3578, "1.20.2"),
    (3698, "1.20.3"),
    (3700, "1.20.4"),
    (3837, "1.20.5"),
    (3839, "1.20.6"),
    (3953, "1.21"),
    (3955, "1.21.1"),
    (4080, "1.21.2"),
    (4082, "1.21.3"),
    (4189, "1.21.4"),
    (4325, "1.21.5"),
    (4435, "1.21.6"),
    (4438, "1.21.7"),
    (4440, "1.21.8"),
    (4554, "1.21.9"),
    (4556, "1.21.10"),
    (4671, "1.21.11"),
    (4786, "26.1"),
    (4788, "26.1.1"),
    (4790, "26.1.2"),
    (4903, "26.2"),
    (5023, "26.3"),
];

/// 基础库的存放根目录（应用数据目录下的 `base/`）。
///
/// 与界面状态同样放系统应用数据目录而不是程序目录：程序可能装在
/// Program Files 这类只读位置，而基础库是运行时才下载/构建出来的，
/// 体积可达数百 MB，必须落在可写且有足够空间的位置。
pub fn base_dir(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Parse(format!("无法定位应用数据目录：{e}")))?;
    Ok(dir.join("base"))
}

/// 单版本方块包的存放目录。
pub fn base_blocks_dir(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    Ok(base_dir(app)?.join("blocks"))
}

/// 实体贴图库文件的位置。
pub fn base_entities_path(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    Ok(base_dir(app)?.join("entities.dat.z"))
}

/// 已下载客户端 jar 的缓存目录。
///
/// 下载落盘走「写临时文件再改名」，所以目录里出现 `<版本>.jar` 就一定是
/// 完整文件，下次补同版本时直接复用，不必重新下载几十 MB。
pub fn base_jars_dir(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    Ok(base_dir(app)?.join("jars"))
}

/// 把目录里的单版本包合成一个基础方块库。
///
/// 目录里每个 `<数据版本>-<版本号>.dat.z` 是一个单版本包（补版本时由
/// 客户端 jar 构建）。合成顺序按文件名里的数据版本升序，保证结果与文件
/// 写入先后无关。目录不存在或没有包时返回空库——首次启动就是这种
/// 情况，由前端引导用户去补版本，而不是报错。
///
/// 单个文件无法解析（文件名不符约定或内容损坏）不中断整库：其余版本
/// 仍应可用，失败项以 (文件名, 原因) 返回供界面显示。
pub fn load_base_block_db(dir: &Path) -> AppResult<(BlockDb, Vec<(String, String)>)> {
    let mut db = BlockDb::empty();
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut packs: Vec<(i32, PathBuf, String)> = Vec::new();

    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        // 目录不存在就是「还没补过版本」的正常状态
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((db, failures)),
        Err(e) => {
            return Err(AppError::Parse(format!(
                "读取基础库目录 {} 失败：{e}",
                dir.display()
            )))
        }
    };
    for entry in rd {
        let entry = entry
            .map_err(|e| AppError::Parse(format!("读取基础库目录 {} 失败：{e}", dir.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        // 构建中的临时文件等不带 .dat.z 后缀，不属于版本包
        if !path.is_file() || !name.ends_with(".dat.z") {
            continue;
        }
        let stem = &name[..name.len() - ".dat.z".len()];
        let Some(dash) = stem.find('-') else {
            failures.push((name, "文件名应为 <数据版本>-<版本号>.dat.z".into()));
            continue;
        };
        match stem[..dash].parse::<i32>() {
            Ok(dv) => packs.push((dv, path, name)),
            Err(_) => failures.push((name, "文件名应以数据版本开头".into())),
        }
    }
    packs.sort_by_key(|(dv, _, _)| *dv);
    for (_, path, name) in packs {
        match BlockDb::load(&path.to_string_lossy(), false) {
            Ok(pack) => {
                if let Err(e) = db.merge_from(&pack, false) {
                    failures.push((name, format!("合并失败：{e}")));
                }
            }
            Err(e) => failures.push((name, format!("载入失败：{e}"))),
        }
    }
    Ok((db, failures))
}

/// 扫描到的本机已装版本。
#[derive(Debug)]
pub struct McVersionHit {
    /// 版本 id，取自 jar 内 version.json 的 `id`（如 `1.20.1`）
    pub id: String,
    /// 从 jar 内 version.json 读到的数据版本
    pub data_version: i32,
    /// 客户端 jar 的完整路径
    pub jar_path: PathBuf,
}

#[derive(Debug)]
pub struct McScan {
    pub versions: Vec<McVersionHit>,
    /// 看着像客户端却读不出版本信息的文件；逐条说明原因，不静默跳过
    pub warnings: Vec<String>,
}

/// 递归走查的深度上限。
///
/// 用户可以选任意目录（甚至整块盘）：没有上限时一次扫描可能走上几十分钟，
/// 界面看起来就是卡死。真实客户端的存放层级很浅（`.minecraft/versions/<id>/`），
/// 六层足以覆盖各家启动器的目录结构。
const SCAN_MAX_DEPTH: usize = 6;

/// 一次扫描最多检查多少个 jar。
///
/// 与深度上限同理：在一个装满模组的目录里逐个解压几千个包会长时间卡住。
const SCAN_MAX_JARS: usize = 2000;

/// 扫描任意目录，递归找出其中所有能识别的 Minecraft 客户端 jar。
///
/// 不要求目录是 .minecraft：整棵子树都会走一遍，逐个 jar 读 `version.json`
/// 判断它是不是客户端，版本 id 与数据版本都取自该文件而不是目录名——jar 可能
/// 被改名、也可能被放在任意位置。
///
/// 没有 `version.json` 的 jar 是模组一类，静默跳过：扫一个 mods 目录时把它们
/// 逐条报成「失败」只会刷屏。有 `version.json` 却解析不出来的才作为失败项上报。
/// 同一个版本在多个位置出现时只保留先遇到的那个（目录项排序后走查，结果稳定）。
///
/// 结果按数据版本升序。
pub fn scan_minecraft(dir: &Path) -> AppResult<McScan> {
    let meta = std::fs::metadata(dir)
        .map_err(|e| AppError::Parse(format!("读取 {} 失败：{e}", dir.display())))?;
    if !meta.is_dir() {
        return Err(AppError::Parse(format!("{} 不是目录", dir.display())));
    }

    let mut scan = McScan { versions: Vec::new(), warnings: Vec::new() };
    let mut seen: HashSet<String> = HashSet::new();
    let mut checked = 0usize;
    let mut truncated = false;
    let mut pending: Vec<(PathBuf, usize)> = vec![(dir.to_path_buf(), 0)];

    while let Some((current, depth)) = pending.pop() {
        let Ok(rd) = std::fs::read_dir(&current) else { continue };
        // 目录项顺序由文件系统决定，先排序再走：同一台机器上结果稳定
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();

        for path in entries {
            let Ok(ft) = std::fs::symlink_metadata(&path) else { continue };
            // 符号链接一律不跟：链成环时走查不会终止
            if ft.file_type().is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if depth < SCAN_MAX_DEPTH {
                    pending.push((path, depth + 1));
                } else {
                    truncated = true;
                }
                continue;
            }
            let is_jar = path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("jar"))
                .unwrap_or(false);
            if !is_jar {
                continue;
            }
            if checked >= SCAN_MAX_JARS {
                truncated = true;
                break;
            }
            checked += 1;
            match read_jar_version(&path) {
                Ok(Some((id, data_version))) => {
                    if seen.insert(id.clone()) {
                        scan.versions.push(McVersionHit {
                            id,
                            data_version,
                            jar_path: path,
                        });
                    }
                }
                // 不是客户端（模组、资源包一类），静默跳过
                Ok(None) => {}
                Err(e) => scan.warnings.push(format!("{}：{e}", path.display())),
            }
        }
        if checked >= SCAN_MAX_JARS {
            break;
        }
    }

    if truncated {
        scan.warnings.push(format!(
            "为避免长时间扫描，最多走到第 {SCAN_MAX_DEPTH} 层、检查 {SCAN_MAX_JARS} 个 jar；\
             需要更深处的版本时，直接选那一层目录再扫一次"
        ));
    }
    scan.versions.sort_by_key(|v| v.data_version);
    Ok(scan)
}

/// 从客户端 jar 读出版本 id 与数据版本。
///
/// 返回 `Ok(None)` 表示这个 jar 不是客户端（里面没有 `version.json`）——
/// 那是模组的正常形态，不该当成错误。
fn read_jar_version(jar: &Path) -> AppResult<Option<(String, i32)>> {
    let jf = crate::pack::jar::JarFile::open(&jar.to_string_lossy())?;
    let Ok(raw) = jf.raw("version.json") else {
        return Ok(None);
    };
    let v: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|e| AppError::Parse(format!("version.json 解析失败：{e}")))?;
    let id = v
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Parse("version.json 里没有 id".into()))?
        .to_string();
    let n = v
        .get("world_version")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| AppError::Parse("version.json 里没有 world_version".into()))?;
    let data_version = i32::try_from(n)
        .map_err(|_| AppError::Parse(format!("world_version {n} 超出范围")))?;
    Ok(Some((id, data_version)))
}

/// 基础库里要一并收集的语言表键（对应 `assets/minecraft/lang/<键>.json`）。
///
/// 界面语言只有中英两种，这两份就够；两种都要：缺中文时界面会显示英文，
/// 缺英文时反过来。「查本机资源库」与「上网补齐」必须用同一份清单，
/// 否则会出现「构建时想要的语言没人去找」。
pub const LANG_KEYS: [&str; 2] = ["zh_cn", "en_us"];

/// 从本机资源库里找出指定版本的语言文件（路径 -> 内容）。
///
/// 客户端 jar 只带 `en_us.json`，其余语言随资源索引下载后存在
/// `.minecraft/assets/objects/<hash 前两位>/<hash>` 里，因此中文要从这里补。
/// 索引文件本身给出「资源路径 -> hash」的映射。
///
/// 目标版本的索引缺失时（例如只装过别的版本、或索引被清理过）退而用任意
/// 一个能查到该语言的索引：语言表只是取名字用的辅助数据，方块名跨版本相当
/// 稳定，比整列没有中文更有用。对应地，查不到就不提供，构建侧会回落到英文。
pub fn find_lang_files(
    mc_dir: &Path,
    version_id: &str,
) -> Vec<(String, Vec<u8>)> {
    let langs = LANG_KEYS;

    let indexes = mc_dir.join("assets").join("indexes");
    let objects = mc_dir.join("assets").join("objects");
    if !indexes.is_dir() || !objects.is_dir() {
        return Vec::new();
    }

    // 先试该版本自己声明的索引，再退到本机任意索引
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(id) = asset_index_id(mc_dir, version_id) {
        candidates.push(indexes.join(format!("{id}.json")));
    }
    if let Ok(rd) = std::fs::read_dir(&indexes) {
        let mut rest: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
            .collect();
        rest.sort();
        candidates.extend(rest);
    }

    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for idx in candidates {
        let Ok(raw) = std::fs::read(&idx) else { continue };
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(&raw) else {
            continue;
        };
        let Some(objs) = json.get("objects").and_then(|v| v.as_object()) else {
            continue;
        };
        for lang in langs {
            let path = format!("assets/minecraft/lang/{lang}.json");
            if out.iter().any(|(p, _)| p == &path) {
                continue;
            }
            let Some(hash) = objs
                .get(&format!("minecraft/lang/{lang}.json"))
                .and_then(|v| v.get("hash"))
                .and_then(|v| v.as_str())
            else {
                continue;
            };
            if hash.len() < 2 {
                continue;
            }
            let obj = objects.join(&hash[..2]).join(hash);
            let Ok(bytes) = std::fs::read(&obj) else { continue };
            out.push((path, bytes));
        }
        if out
            .iter()
            .any(|(p, _)| p.ends_with("zh_cn.json"))
        {
            break;
        }
    }
    out
}

/// 读版本 json 里声明的资源索引 id（`assetIndex.id`）。
fn asset_index_id(mc_dir: &Path, version_id: &str) -> Option<String> {
    let path = mc_dir
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.json"));
    let raw = std::fs::read(path).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    json.get("assetIndex")?
        .get("id")?
        .as_str()
        .map(|s| s.to_string())
}

/// 官方启动器的默认 .minecraft 位置；环境变量缺失时返回 None。
///
/// 只按平台惯例拼路径，不判断目录是否存在：是否可用由调用方决定——
/// 对话框只把真实存在的目录当默认值预填。
pub fn default_minecraft_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|v| PathBuf::from(v).join(".minecraft"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|v| {
            PathBuf::from(v)
                .join("Library")
                .join("Application Support")
                .join("minecraft")
        })
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".minecraft"))
    }
}

#[cfg(test)]
#[path = "../tests/pack/base.rs"]
mod tests;
