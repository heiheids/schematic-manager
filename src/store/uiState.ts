/**
 * 界面状态记忆域：把用户调过的界面形态（侧栏宽度、目录、列宽、
 * 视角、亮度、导出尺寸、搜索词、日志等级）持久化，下次启动时恢复。
 *
 * 这个域横跨几乎所有其他域（读它们的状态来收集、写它们的状态来恢复），
 * 因此它最后被创建：注册顺序决定 onMounted 执行顺序，恢复必须发生在
 * 菜单监听等挂载逻辑之后（见 store/index.ts 的装配顺序）。
 * 依赖只声明本域用到的成员，避免类型上的循环引用（见 store/fileList.ts）。
 */
import { onBeforeUnmount, onMounted, watch } from "vue";
import type { ComputedRef, Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { loadUiState, saveUiState } from "../api";
import { currentLocale, isLocale, locale, setLocale, t } from "../i18n";
import type { ExportAspect, SavedViewPreset } from "../render/scene3d";
import type { SortKey, SortOrder, StatusText } from "./fileList";
import type { WorkspaceTab, WorkspaceTabKind } from "./workspace";
import {
  EXPORT_ASPECTS,
  EXPORT_SHORT_EDGES,
  isBuiltinView,
  sanitizeViewPresets,
} from "../render/scene3d";
import { isLogLevel } from "../utils/run_log";
import type { LogLevel } from "../utils/run_log";
import { reportUiWarning } from "../utils/run_log_report";
import type { SchematicSummary } from "../types";

/** 会被记住的界面状态。
 *
 * 只记「用户调过的界面形态」，不记业务数据（备份、基础库等有自己的文件）。
 * 版本号用于日后改结构时丢弃旧数据，而不是把不认识的字段硬套上去。
 */
interface UiState {
  version: 1;
  sidebarWidth: number;
  /** 预览面板高度；缺失时用默认值 */
  previewHeight?: number;
  /** 预览舞台宽度；缺失时用默认值 */
  previewWidth?: number;
  /** 已添加的目录（顺序保持），下次启动自动重新扫描 */
  dirs: string[];
  /** 已勾选显示的目录；缺失时视为全部显示 */
  enabledDirs?: string[];
  /** 用户拖动表头调整过的列宽（dataIndex -> 像素宽度） */
  columnWidths?: Record<string, number>;
  activeView: string;
  brightness: number;
  /** 导出尺寸的短边像素数（旧版含义是正方形边长，取值同样落在合法档位内） */
  exportSize: number;
  /** 导出尺寸的宽高比标识；缺失时按 1:1 处理 */
  exportAspect?: string;
  /** 用户保存的自定义视角预设；读回时逐条校验 */
  savedViews?: unknown;
  searchText: string;
  /** 界面语言；认不出来的值按默认语言处理 */
  locale?: string;
  /** 运行日志等级；缺失或认不出来的值按 "info" 处理 */
  logLevel?: string;
  /** 列表排序字段与方向；都没选时按路径排序（原行为） */
  sortKey?: string;
  sortOrder?: string;
  /** 文件管理目录树的界面状态 */
  selectedDirKey?: string;
  expandedDirKeys?: string[];
  showDescendants?: boolean;
  /** 固定标签的开启状态；编辑会话不写入磁盘 */
  fixedTabs?: string[];
}

const UI_STATE_VERSION = 1;

/** 排序字段的合法取值，与 fileList 的 SortKey 保持一致 */
const SORT_KEYS: readonly SortKey[] = [
  "fileName",
  "name",
  "mcVersionName",
  "timeModified",
  "author",
];

function isSortKey(v: unknown): v is SortKey {
  return typeof v === "string" && (SORT_KEYS as readonly string[]).includes(v);
}

function isSortOrder(v: unknown): v is SortOrder {
  return v === "ascend" || v === "descend";
}

/** 界面状态域用到的其他域成员（循环引用回避见 fileList.ts）。
 *
 * 这个域的依赖面最宽——它要收集和恢复各域的界面形态。
 */
export interface UiStateDeps {
  /** 侧栏宽度（composables/useSidebarResize） */
  sidebarWidth: Ref<number>;
  /** 侧栏宽度钳制（composables/useSidebarResize） */
  clampSidebar: (w: number) => number;
  /** 预览面板高度（composables/usePreviewResize） */
  previewHeight: Ref<number>;
  /** 预览面板高度钳制（composables/usePreviewResize） */
  clampPreviewHeight: (h: number) => number;
  /** 预览舞台宽度（composables/usePreviewResize） */
  previewWidth: Ref<number>;
  /** 预览舞台宽度钳制（composables/usePreviewResize） */
  clampPreviewWidth: (w: number) => number;
  /** 各目录的文件清单（fileList） */
  dirItems: Ref<Record<string, SchematicSummary[]>>;
  /** 已加载目录列表（fileList） */
  loadedDirs: ComputedRef<string[]>;
  /** 勾选显示的目录集合（fileList） */
  enabledDirs: Ref<Set<string>>;
  /** 按根目录扫描，提交时忽略已失效的响应 */
  scanDirectories: (
    dirs: string[],
  ) => Promise<{ scanned: string[]; missing: string[] }>;
  /** 文件表列宽（fileList） */
  colWidths: Ref<Record<string, number>>;
  /** 预览视角（scenePreview） */
  activeView: Ref<string>;
  /** 预览亮度（scenePreview） */
  brightness: Ref<number>;
  /** 预览导出尺寸的短边像素数（scenePreview） */
  exportSize: Ref<number>;
  /** 预览导出尺寸的宽高比（scenePreview） */
  exportAspect: Ref<ExportAspect>;
  /** 用户保存的自定义视角预设（scenePreview） */
  savedViews: Ref<SavedViewPreset[]>;
  /** 文件搜索词（fileList） */
  searchText: Ref<string>;
  /** 列表排序字段（fileList）；null 表示按路径 */
  sortKey: Ref<SortKey | null>;
  /** 列表排序方向（fileList） */
  sortOrder: Ref<SortOrder | null>;
  /** 状态栏内容（fileList）；恢复完成后汇报目录数 */
  status: Ref<StatusText>;
  /** 文件管理当前目录与展开状态 */
  selectedDirKey: Ref<string>;
  selectDirectory: (key: string) => void;
  expandedDirKeys: Ref<Set<string>>;
  showDescendants: Ref<boolean>;
  /** 固定标签的开启状态 */
  tabs: Ref<WorkspaceTab[]>;
  restoreFixedTabs: (kinds: WorkspaceTabKind[]) => void;
  /** 运行日志等级（backups）；本身也是要记住的界面形态 */
  logLevel: Ref<LogLevel>;
  /** 把当前等级与本地日期推给后端（backups）；恢复完成后必须调用 */
  syncRunLogLevel: () => Promise<void>;
}

export function createUiStateSlice(s: UiStateDeps) {
  /** 把当前界面形态收集成可保存的对象 */
  function collectUiState(): UiState {
    return {
      version: UI_STATE_VERSION,
      sidebarWidth: s.sidebarWidth.value,
      previewHeight: s.previewHeight.value,
      previewWidth: s.previewWidth.value,
      dirs: s.loadedDirs.value,
      enabledDirs: Array.from(s.enabledDirs.value),
      columnWidths: { ...s.colWidths.value },
      activeView: s.activeView.value,
      brightness: s.brightness.value,
      exportSize: s.exportSize.value,
      exportAspect: s.exportAspect.value,
      savedViews: s.savedViews.value,
      searchText: s.searchText.value,
      locale: currentLocale(),
      logLevel: s.logLevel.value,
      sortKey: s.sortKey.value ?? undefined,
      sortOrder: s.sortOrder.value ?? undefined,
      selectedDirKey: s.selectedDirKey.value || undefined,
      expandedDirKeys: Array.from(s.expandedDirKeys.value),
      showDescendants: s.showDescendants.value,
      fixedTabs: s.tabs.value.filter((tab) => tab.kind !== "editor").map((tab) => tab.kind),
    };
  }

  let saveStateTimer: ReturnType<typeof setTimeout> | null = null;

  /** 立即写出当前界面状态。
   *
   * 失败不是致命问题，但要留下线索：静默失败会让用户以为「记住了」，
   * 下次启动发现设置全丢却无从判断。
   */
  async function writeUiState() {
    try {
      await saveUiState(JSON.stringify(collectUiState()));
    } catch (e) {
      // save_ui_state 的失败已由 api.ts 的 logged 出口记进运行日志（带命令名），
      // 这里只留控制台一份，免得同一次失败出现两条
      console.warn("保存界面状态失败：", e);
    }
  }

  /** 保存界面状态（合并短时间内的多次改动）。
   *
   * 用防抖：拖动分隔条会连续改动侧栏宽度，逐次写盘既无必要也拖慢拖动。
   */
  function persistUiState() {
    if (saveStateTimer !== null) clearTimeout(saveStateTimer);
    saveStateTimer = setTimeout(() => {
      saveStateTimer = null;
      void writeUiState();
    }, 400);
  }

  /** 卸载时把防抖中的改动立刻写出。
   *
   * 只清定时器不写，用户在关窗前 <400ms 的调整会白做；
   * 只留定时器不清，这个已销毁实例的写盘会发生在之后任意时刻——
   * 读到的是它自己那份早已过期的状态（实测会污染后续的界面状态记录）。
   * 两者都不可接受，因此在这里收口成「马上写一次」。
   */
  function flushUiState() {
    if (saveStateTimer === null) return;
    clearTimeout(saveStateTimer);
    saveStateTimer = null;
    void writeUiState();
  }

  /** 恢复界面状态并重新扫描上次的目录 */
  async function restoreUiState() {
    let raw: string | null = null;
    try {
      raw = await loadUiState();
    } catch (e) {
      // load_ui_state 的失败已由 logged 记录，这里只留控制台一份
      console.warn("读取界面状态失败，使用默认界面：", e);
      return;
    }
    if (!raw) return;

    let st: Partial<UiState>;
    try {
      st = JSON.parse(raw) as Partial<UiState>;
    } catch (e) {
      // 内容损坏时不应让程序起不来，也不该静默：明确告知并按默认界面继续
      reportUiWarning("界面状态内容无法解析，已忽略：", e);
      Message.warning(t("ui.stateUnreadable"));
      return;
    }
    // 版本不匹配时丢弃：旧结构硬套会得到难以解释的界面
    if (st.version !== UI_STATE_VERSION) {
      reportUiWarning(`界面状态版本为 ${String(st.version)}，当前为 ${UI_STATE_VERSION}，已忽略`);
      return;
    }

    if (typeof st.sidebarWidth === "number") {
      s.sidebarWidth.value = s.clampSidebar(st.sidebarWidth);
    }
    if (typeof st.previewHeight === "number") {
      s.previewHeight.value = s.clampPreviewHeight(st.previewHeight);
    }
    if (typeof st.previewWidth === "number") {
      s.previewWidth.value = s.clampPreviewWidth(st.previewWidth);
    }
    // 只认内置预设的键。不能写成 `st.activeView in VIEW_PRESETS`：`in` 会
    // 沿原型链查到 constructor、toString 这类键，放进来会让取景算出 NaN。
    // 自定义预设的 `custom:` 键在这里被排除——它要连 savedViews 一起校验，
    // 而恢复顺序上没法保证两者对得上，不如回到默认视角更可预期。
    if (typeof st.activeView === "string" && isBuiltinView(st.activeView)) {
      s.activeView.value = st.activeView;
    }
    if (typeof st.brightness === "number") {
      s.brightness.value = Math.min(2, Math.max(0.5, st.brightness));
    }
    if (
      typeof st.exportSize === "number" &&
      EXPORT_SHORT_EDGES.includes(st.exportSize as never)
    ) {
      s.exportSize.value = st.exportSize;
    }
    // 宽高比只认预设里的值：脏值会让导出尺寸算得毫无道理，
    // 而用户在界面上看不出是「记住了坏值」还是「导出坏了」
    if (typeof st.exportAspect === "string" && EXPORT_ASPECTS.some((a) => a.key === st.exportAspect)) {
      s.exportAspect.value = st.exportAspect as ExportAspect;
    }
    // 自定义视角预设逐条过校验：磁盘上的数据可能是旧版本写的或被手工改坏，
    // 把脏值套进相机会得到一个指向 NaN 的画面，且用户看不出是数据坏了
    if (st.savedViews !== undefined) {
      s.savedViews.value = sanitizeViewPresets(st.savedViews);
    }
    if (typeof st.searchText === "string") {
      s.searchText.value = st.searchText;
    }
    // 排序只认约定里的字段与方向：脏值留下去会让列表顺序变得无法解释，
    // 而用户看不出是「记住了坏值」还是「排序坏了」
    if (isSortKey(st.sortKey) && isSortOrder(st.sortOrder)) {
      s.sortKey.value = st.sortKey;
      s.sortOrder.value = st.sortOrder;
    }
    const savedDirKey = typeof st.selectedDirKey === "string" ? st.selectedDirKey : "";
    const savedExpandedDirKeys = Array.isArray(st.expandedDirKeys)
      ? st.expandedDirKeys.filter((key): key is string => typeof key === "string")
      : null;
    if (typeof st.showDescendants === "boolean") {
      s.showDescendants.value = st.showDescendants;
    }
    if (Array.isArray(st.fixedTabs)) {
      s.restoreFixedTabs(st.fixedTabs as WorkspaceTabKind[]);
    }
    // 语言先恢复：后面那些提示文案要用它来出
    if (isLocale(st.locale)) {
      setLocale(st.locale);
    }
    // 日志等级只认约定里的四档：认不出的取值照原样记下会让后端收到它不认识的等级
    if (isLogLevel(st.logLevel)) {
      s.logLevel.value = st.logLevel;
    }
    // 恢复列宽。只接受正数：0 或负数会让列不可见，损坏的值不该让界面变形
    if (st.columnWidths && typeof st.columnWidths === "object") {
      const clean: Record<string, number> = {};
      for (const [k, v] of Object.entries(st.columnWidths)) {
        if (typeof v === "number" && Number.isFinite(v) && v > 0) clean[k] = v;
      }
      s.colWidths.value = clean;
    }

    // 重新扫描上次的目录。逐个独立处理：某个目录已被删除或暂时不可读时，
    // 其余目录仍应恢复，并如实说明少了哪个。
    const dirs = Array.isArray(st.dirs) ? st.dirs.filter((d) => typeof d === "string") : [];
    if (dirs.length === 0) return;

    const { scanned, missing } = await s.scanDirectories(dirs);
    const keep = new Set(
      Array.isArray(st.enabledDirs) ? st.enabledDirs.filter((d) => typeof d === "string") : [],
    );
    const enabled = new Set(s.enabledDirs.value);
    for (const dir of scanned) {
      if (!Array.isArray(st.enabledDirs) || keep.has(dir)) enabled.add(dir);
      else enabled.delete(dir);
    }
    s.enabledDirs.value = enabled;
    if (savedDirKey) s.selectDirectory(savedDirKey);
    if (savedExpandedDirKeys) s.expandedDirKeys.value = new Set(savedExpandedDirKeys);
    if (missing.length > 0) {
      Message.warning(t("ui.dirsMissing", { dirs: missing.join("\n") }));
      // 把失效目录从记忆里清掉
      persistUiState();
    }
    s.status.value = {
      key: "ui.dirsRestored",
      params: { count: scanned.length },
    };
  }

  // 记住界面改动。逐个监听而不是 watch 一个大对象：后者要么深比较整棵状态树，
  // 要么在 restoreUiState 写入时也触发一次无谓回写。
  watch(s.sidebarWidth, persistUiState);
  watch(s.previewHeight, persistUiState);
  watch(s.previewWidth, persistUiState);
  watch(s.activeView, persistUiState);
  watch(s.brightness, persistUiState);
  watch(s.exportSize, persistUiState);
  watch(s.exportAspect, persistUiState);
  // 预设是数组：deep 才能感知增删与改名
  watch(s.savedViews, persistUiState, { deep: true });
  watch(s.searchText, persistUiState);
  // 排序：点一次列头就换一种顺序，属于用户对界面的调整
  watch([s.sortKey, s.sortOrder], persistUiState);
  // 语言也是要记住的界面形态
  watch(locale, persistUiState);
  // 日志等级由「日志管理」窗口里的选择器改动，同样要记住
  watch(s.logLevel, persistUiState);
  // 目录集合变化（添加/移除/勾选）同样要记住
  watch(s.dirItems, persistUiState, { deep: false });
  watch(s.enabledDirs, persistUiState, { deep: false });
  watch(s.selectedDirKey, persistUiState);
  watch(s.expandedDirKeys, persistUiState, { deep: false });
  watch(s.showDescendants, persistUiState);
  watch(s.tabs, persistUiState, { deep: false });

  onMounted(async () => {
    // 恢复上次的界面状态与已添加目录。放在最后：先把界面建起来，
    // 恢复只改状态，不必等它完成就能显示
    await restoreUiState();
    // 恢复之后再推日志等级：等级要用恢复出来的值，日期要用用户当下的本地日期。
    // 没保存过状态时也要推，否则后端只能用它的默认值记日志
    await s.syncRunLogLevel();
  });

  onBeforeUnmount(() => {
    flushUiState();
  });

  return {
    persistUiState,
    flushUiState,
    restoreUiState,
  };
}

export type UiStateSlice = ReturnType<typeof createUiStateSlice>;
