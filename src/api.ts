// Rust 命令的类型化封装，避免各组件直接拼命令名与参数。
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AnalysisResult,
  BackupEntry,
  BaseDbStatus,
  BaseSetupProgress,
  BlockHit,
  BlockIcon,
  McScan,
  SaveOutcome,
  ScanResult,
  ConvertItem,
  ConvertOutcome,
  ConvertTarget,
  DuplicateScan,
  EntityTexture,
  EntityTextureStatus,
  FileOpOutcome,
  ReplaceOutcome,
  ReplaceRule,
  RenameOutcome,
  SceneData,
  SchematicSummary,
  SetupOutcome,
  SetupSource,
  SetupVersion,
  UpdateInfo,
  VersionOption,
} from "./types";

/**
 * 调命令的统一出口：失败时既把原始错误抛给调用方，也先记一条带命令名的 error 运行日志。
 *
 * 为什么收口在这里：命令失败（「还原备份失败：权限不足」这类）只在界面上弹一次提示，
 * 关掉就没了；用户报「点了报错」时日志里连是哪条命令失败都查不到。在后端逐个上抛点
 * 补日志既漏又散，而命令名只有这里知道，因此统一在这一层记。
 */
function logged<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  // 无参命令必须只发命令名：后端按无参签名收，多传一个 undefined 不算错，
  // 但那是与契约不符的调用形状，也让「无参」这件事在调用点看不出来
  const call = args === undefined ? invoke<T>(cmd) : invoke<T>(cmd, args);
  return call.catch((e) => {
    // 上报失败只补一条控制台线索：诊断通道不能盖掉原始错误，也不能把调用方带崩
    void logUiEvent("error", `命令 ${cmd} 失败：${String(e)}`).catch((reportErr) => {
      console.warn("写入运行日志失败：", reportErr);
    });
    throw e;
  });
}

/** 只读扫描单个投影文件（不解码方块） */
export function peekSchematic(path: string): Promise<SchematicSummary> {
  return logged<SchematicSummary>("peek_schematic", { path });
}

/** 递归扫描目录下全部 .litematic。返回值含被跳过的目录数，便于界面提示。 */
export function scanLitematics(dir: string, maxDepth = 6): Promise<ScanResult> {
  return logged<ScanResult>("scan_litematics", { dir, maxDepth });
}

/** 完整解码并统计方块与实体 */
export function analyzeSchematic(path: string): Promise<AnalysisResult> {
  return logged<AnalysisResult>("analyze_schematic", { path });
}

/**
 * 保存元数据。未提供的字段保持不变。
 * 保存前会由后端自动备份原件，备份失败则不写入。
 */
export function saveMetadata(
  path: string,
  fields: { name?: string; author?: string; description?: string },
): Promise<SaveOutcome> {
  return logged<SaveOutcome>("save_metadata", { path, ...fields });
}

/**
 * 把当前投影另存为新文件，并把未保存的元数据改动一并写进去。
 *
 * 原文件不动，也不产生备份：产物是一个全新文件。
 * 目标已存在时后端直接报错，不会覆盖。
 */
export function saveSchematicAs(
  sourcePath: string,
  targetPath: string,
  fields: { name?: string; author?: string; description?: string },
): Promise<string> {
  return logged<string>("save_schematic_as", { sourcePath, targetPath, ...fields });
}

/**
 * 组装前端渲染所需的场景。
 * 基础库由后端按「版本库」准备的内容解析，前端只提供投影文件路径。
 */
export function buildScene(schematicPath: string): Promise<SceneData> {
  return logged<SceneData>("build_scene", { schematicPath });
}

/** 跨版本检索方块 */
export function searchBlocks(query: string, limit = 200): Promise<BlockHit[]> {
  return logged<BlockHit[]>("search_blocks", { query, limit });
}

/** 列出方块库中的全部版本 */
export function listBlockVersions(): Promise<VersionOption[]> {
  return logged<VersionOption[]>("list_block_versions");
}

/** 预计会被替换的方块数量，不写入文件 */
export function previewReplace(schematicPath: string, rule: ReplaceRule): Promise<ReplaceOutcome> {
  return logged<ReplaceOutcome>("preview_replace", { schematicPath, rule });
}

/** 执行替换并写回文件；后端会先备份 */
export function applyReplace(schematicPath: string, rule: ReplaceRule): Promise<ReplaceOutcome> {
  return logged<ReplaceOutcome>("apply_replace", { schematicPath, rule });
}

/** 列出可作为转换目标的版本 */
export function convertTargets(schematicPath: string): Promise<ConvertTarget[]> {
  return logged<ConvertTarget[]>("convert_targets", { schematicPath });
}

/** 分析投影中用到但目标版本没有的方块 */
export function analyzeConversion(
  schematicPath: string,
  targetIndex: number,
): Promise<ConvertItem[]> {
  return logged<ConvertItem[]>("analyze_conversion", { schematicPath, targetIndex });
}

/** 执行转换，结果另存为新文件，原文件不动 */
export function applyConversion(
  schematicPath: string,
  targetIndex: number,
  items: ConvertItem[],
): Promise<ConvertOutcome> {
  return logged<ConvertOutcome>("apply_conversion", { schematicPath, targetIndex, items });
}

/** 在给定的若干目录下递归筛查内容重复的投影文件。
 *
 * 多个目录一次交过去，跨目录的副本才能归到同一组；递归层数与
 * 文件列表一致，因此列表里看到的文件都会参与筛查。 */
export function scanDuplicates(dirs: string[]): Promise<DuplicateScan> {
  return logged<DuplicateScan>("scan_duplicates", { dirs });
}

/** 列出备份；传 stem 时只返回该文件的备份 */
export function listBackups(stem?: string): Promise<BackupEntry[]> {
  return logged<BackupEntry[]>("list_backups", { stem: stem ?? null });
}

/** 把备份还原到目标文件。后端会校验二者同源。 */
export function restoreBackup(backupPath: string, targetPath: string): Promise<void> {
  return logged<void>("restore_backup", { backupPath, targetPath });
}

/** 删除一条备份 */
export function deleteBackup(backupPath: string): Promise<void> {
  return logged<void>("delete_backup", { backupPath });
}

/** 读取运行日志，由新到旧 */
export function readRunLog(maxLines = 200): Promise<string[]> {
  return logged<string[]>("read_run_log", { maxLines });
}

/** 清空运行日志（只清内容，日志文件保留） */
export function clearRunLog(): Promise<void> {
  return logged<void>("clear_run_log");
}

/** 在系统文件管理器里打开运行日志所在的目录 */
export function openRunLogDir(): Promise<void> {
  return logged<void>("open_run_log_dir");
}

/**
 * 设置运行日志的等级与当前本地日期。
 *
 * 等级决定后端收下哪些行；日期决定日志按哪一天写入、以及滚动时归档成什么名字。
 * 两者每次一起给出，是因为等级变更时也要顺带刷新日期——用户可能开着界面跨了天。
 */
export function configureRunLog(level: string, localDate: string): Promise<void> {
  return logged<void>("configure_run_log", { level, localDate });
}

/**
 * 把界面侧遇到的警告/错误交给后端写进运行日志。
 *
 * 等级取值同 configureRunLog。后端会在说明前加「界面 」标明来源，调用方不必自己加。
 *
 * 这一条刻意不走 logged：它就是 logged 上报失败的通道，若也走一遍，
 * 写日志失败会再触发一次写日志，递归下去。
 */
export function logUiEvent(level: string, message: string): Promise<void> {
  return invoke<void>("log_ui_event", { level, message });
}

/** 运行日志所在目录；日志尚未启用时为 null */
export function runLogDir(): Promise<string | null> {
  return logged<string | null>("run_log_dir");
}

/** dataVersion 对应的游戏版本名 */
export function mcVersionName(dataVersion: number): Promise<string> {
  return logged<string>("mc_version_name", { dataVersion });
}

// ---------------------------------------------------------------- 方块渲染图

/**
 * 批量渲染方块的等轴测小图。
 *
 * `states` 是方块状态键（`BlockCount.state`），一次一批而不是一行一次：
 * 方块栏一页 50 行，逐行调用就是 50 次跨进程往返。
 * 单项失败会在返回值里带上原因，不会让整批失败。
 */
export function renderBlockIcons(
  dataVersion: number,
  states: string[],
  size?: number,
): Promise<BlockIcon[]> {
  return logged<BlockIcon[]>("render_block_icons", {
    dataVersion,
    states,
    size: size ?? null,
  });
}

// ---------------------------------------------------------------- 目录栏文件操作

/** 删除选中的投影文件（移入系统回收站，可还原） */
export function deleteFiles(paths: string[]): Promise<FileOpOutcome> {
  return logged<FileOpOutcome>("delete_files", { paths });
}

/** 把选中的文件复制到剪贴板，供在资源管理器里粘贴 */
export function copyFilesToClipboard(paths: string[]): Promise<FileOpOutcome> {
  return logged<FileOpOutcome>("copy_files_to_clipboard", { paths });
}

/** 把选中的文件复制到指定目录（同名不覆盖，自动加序号） */
export function copyFilesToDir(paths: string[], destDir: string): Promise<FileOpOutcome> {
  return logged<FileOpOutcome>("copy_files_to_dir", { paths, destDir });
}

/** 在文件管理器中定位到一个文件 */
export function revealInFileManager(path: string): Promise<void> {
  return logged<void>("reveal_in_file_manager", { path });
}

/** 重命名投影文件（只传文件名，不含目录），备份会一并重新链接 */
export function renameSchematic(path: string, newName: string): Promise<RenameOutcome> {
  return logged<RenameOutcome>("rename_schematic", { path, newName });
}

// ---------------------------------------------------------------- 实体贴图

/** 实体贴图库的可用情况（版本数、贴图数、路径数） */
export function entityTextureStatus(): Promise<EntityTextureStatus> {
  return logged<EntityTextureStatus>("entity_texture_status");
}

/**
 * 取一张实体贴图的 RGBA 像素。
 *
 * 路径形如 `entity/chest/normal`。查不到时后端返回错误，
 * 而不是一张空图——否则界面上只会看到一张纯黑贴图，无从判断原因。
 */
export function entityTexturePixels(
  dataVersion: number,
  path: string,
): Promise<EntityTexture> {
  return logged<EntityTexture>("entity_texture_pixels", { dataVersion, path });
}

// ---------------------------------------------------------------- 界面状态记忆

/** 读取上次保存的界面状态；从未保存过时为 null */
export function loadUiState(): Promise<string | null> {
  return logged<string | null>("load_ui_state");
}

/** 保存界面状态（一段前端解释的 JSON） */
export function saveUiState(json: string): Promise<void> {
  return logged<void>("save_ui_state", { json });
}

// ---------------------------------------------------------------- 版本库

/** 基础库的当前状态（已就绪版本、实体贴图库是否可用） */
export function baseDbStatus(): Promise<BaseDbStatus> {
  return logged<BaseDbStatus>("base_db_status");
}

/** 官方启动器的默认 .minecraft 路径；没有安装过官方启动器时为 null */
export function defaultMinecraftDir(): Promise<string | null> {
  return logged<string | null>("default_minecraft_dir");
}

/** 扫描 .minecraft 目录里已安装的版本（只读） */
export function scanMinecraft(dir: string): Promise<McScan> {
  return logged<McScan>("scan_minecraft", { dir });
}

/**
 * 准备基础库版本：有本地 jar 的直接用，没有的按来源下载后构建。
 * 单个版本失败不影响其它版本，逐版本结果在返回值里。
 */
export function setupAddVersions(
  source: SetupSource,
  versions: SetupVersion[],
): Promise<SetupOutcome> {
  return logged<SetupOutcome>("setup_add_versions", { source, versions });
}

/** 读取「版本库」界面状态；从未保存过时为 null */
export function loadSetupState(): Promise<string | null> {
  return logged<string | null>("load_setup_state");
}

/** 保存「版本库」界面状态（目录与下载来源，一段前端解释的 JSON） */
export function saveSetupState(json: string): Promise<void> {
  return logged<void>("save_setup_state", { json });
}

/** 订阅「版本库」进度事件；返回取消订阅函数 */
export function onBaseSetupProgress(
  handler: (p: BaseSetupProgress) => void,
): Promise<() => void> {
  return listen<BaseSetupProgress>("base-setup-progress", (e) => handler(e.payload));
}

/** 当前可执行文件的版本号 */
export function appVersion(): Promise<string> {
  return logged<string>("app_version");
}

/** 检查是否有新版本（后端读 GitHub 发布列表）。网络失败会 reject。 */
export function checkUpdate(): Promise<UpdateInfo> {
  return logged<UpdateInfo>("check_update");
}

/** 用系统默认浏览器打开外部链接。
 *
 * 后端只放行 http/https 并挡掉可能干扰命令行解析的字符，非法链接会 reject。
 */
export function openUrl(url: string): Promise<void> {
  return logged<void>("open_url", { url });
}
