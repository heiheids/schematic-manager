//! 版本库：扫描本机客户端、下载 jar、构建基础库并报告进度。

use crate::error::{AppError, AppResult};
use super::dto::{BaseVersionDto, BaseDbStatusDto, McVersionDto, McScanDto, SetupSource, SetupVersionDto, SetupVersionOutcomeDto, SetupOutcomeDto};
use super::shared::{open_entity_db, invalidate_db, invalidate_entity_db};
use super::state::{read_ui_state_file, write_ui_state_file, config_dir};
use std::path::{Path, PathBuf};

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
        crate::app::run_log::warn(&format!("发送版本库进度事件失败：{e}"));
    }
}

/// 基础库的当前状态：已就绪的版本清单与实体贴图库是否可用。
///
/// 前端据此决定要不要自动弹出「版本库」界面——库里一个版本都没有时
/// 方块检索和 3D 预览都没有数据可显示，必须先引导用户补版本。
#[tauri::command]
pub fn base_db_status(app: tauri::AppHandle) -> Result<BaseDbStatusDto, String> {
    let dir = crate::pack::base::base_blocks_dir(&app).map_err(|e| e.to_string())?;
    let (base, _failures) = crate::pack::base::load_base_block_db(&dir).map_err(|e| e.to_string())?;
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
        supported: crate::pack::base::SUPPORTED_RELEASES
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
    crate::pack::base::default_minecraft_dir()
        .filter(|p| p.is_dir())
        .map(|p| p.to_string_lossy().into_owned())
}

/// 扫描任意目录（含子目录）里所有能识别的客户端 jar（只读）。
///
/// 结果按数据版本升序；看着像客户端却读不出版本信息的文件逐条带原因返回，
/// 由前端决定怎么展示。模组一类没有 `version.json` 的 jar 会被静默跳过。
#[tauri::command]
pub fn scan_minecraft(dir: String) -> Result<McScanDto, String> {
    let scan = crate::pack::base::scan_minecraft(Path::new(&dir)).map_err(|e| e.to_string())?;
    // 读不出版本信息的目录会被跳过，用户在清单里少看到一个版本时要能查到原因
    for w in &scan.warnings {
        crate::app::run_log::warn(&format!("扫描 Minecraft 目录时跳过：{w}"));
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
            crate::pack::base::base_blocks_dir(app)?,
            crate::pack::base::base_jars_dir(app)?,
            crate::pack::base::base_entities_path(app)?,
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
            crate::mc::download::fetch_manifest()
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
                    crate::app::run_log::warn(&format!(
                        "准备 {} 完成，但有提示：{code}",
                        v.id
                    ));
                }
                crate::app::run_log::info(&format!("准备 {} 完成", v.id));
                SetupVersionOutcomeDto {
                    id: v.id.clone(),
                    ok: true,
                    error: String::new(),
                    warning,
                }
            }
            Err(e) => {
                // 用户报「某个版本准备不了」时，日志里得有版本号与原因
                crate::app::run_log::error(&format!("准备 {} 失败：{e}", v.id));
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
                    Some(m) => crate::mc::download::official_client_url(m, &v.id)?,
                    None => crate::mc::download::bmcl_client_url(&v.id),
                };
                let id = v.id.clone();
                crate::mc::download::download_to(&url, &dest, &mut |received, total| {
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
        .or_else(crate::pack::base::default_minecraft_dir);
    let mut langs = match &mc_dir {
        Some(dir) => crate::pack::base::find_lang_files(dir, &v.id),
        None => Vec::new(),
    };
    let warning = fill_missing_langs(v, manifest, mc_dir.as_deref(), source, &mut langs);
    crate::pack::jar::build_pack_to_with_langs(
        &jar_path.to_string_lossy(),
        &out.to_string_lossy(),
        &v.id,
        v.data_version,
        false,
        langs,
    )?;

    let jar = crate::pack::jar::JarFile::open(&jar_path.to_string_lossy())?;
    crate::pack::entity_write::add_version(entities_path, &jar, v.data_version, &v.id)?;

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
    crate::pack::base::LANG_KEYS
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
    let req = crate::mc::download::LangDownload {
        version_id: &v.id,
        source: match source {
            SetupSource::Official => crate::mc::download::AssetSource::Official,
            SetupSource::Bmcl => crate::mc::download::AssetSource::Bmcl,
        },
        mc_dir,
        manifest,
        langs: &missing,
    };
    match crate::mc::download::download_lang_files(&req) {
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
            crate::app::run_log::warn(&format!(
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

#[cfg(test)]
#[path = "../tests/commands/setup.rs"]
mod tests;
