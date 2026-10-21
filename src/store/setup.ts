/**
 * 版本库域：「版本库」弹窗的全部状态与动作——选择目录、递归扫描其中的客户端
 * jar 并自动导入，以及勾选清单里没有、需要联网下载的版本。
 *
 * 基础库是方块检索与 3D 预览的数据底座：库里一个版本都没有时，
 * 这些功能都无数据可显示，所以启动时发现库为空会自动弹出本界面
 * 引导补齐；之后也可从工具栏再次打开补充缺失版本。
 *
 * 与其他域的协作：不直接读写其他域的状态；补库完成后后端的库缓存
 * 已失效，各域下次取数据（listBlockVersions、buildScene 等）自然
 * 用上新库，无需在这里逐个通知。
 */
import { computed, onMounted, ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { open } from "@tauri-apps/plugin-dialog";
import { t } from "../i18n";
import {
  baseDbStatus,
  defaultMinecraftDir,
  loadSetupState,
  onBaseSetupProgress,
  saveSetupState,
  scanMinecraft,
  setupAddVersions,
} from "../api";
import type {
  BaseSetupProgress,
  BaseVersion,
  McVersion,
  SetupSource,
  SetupVersionOutcome,
} from "../types";
import { reportUiWarning } from "../utils/run_log_report";

/** 「版本库」域用到的外部成员。
 *
 * 只声明本域真的用到的那些，避免和别的域互相引用成环。
 */
export interface SetupDeps {
  /** 重新解析当前选中项：基础库变了之后方块名与渲染图都会过期 */
  refreshSelectedAnalysis: () => Promise<void>;
}

/** 会被记住的「版本库」界面状态。版本号用于日后改结构时丢弃旧数据。 */
interface SetupState {
  version: 1;
  mcDir: string;
  source: SetupSource;
}

const SETUP_STATE_VERSION = 1;

/** 一个支持版本在界面上的状态 */
export interface SetupRow {
  name: string;
  dataVersion: number;
  /** ready=基础库已有；local=本机已装可直接用；missing=需要下载 */
  state: "ready" | "local" | "missing";
  jarPath: string | null;
}

export function createSetupSlice(s: SetupDeps) {
  const setupOpen = ref(false);
  /** 打开弹窗过程中的加载态，供工具栏按钮显示 */
  const setupOnOpen = ref(false);
  const scanning = ref(false);
  const running = ref(false);

  const mcDir = ref("");
  const source = ref<SetupSource>("official");

  /** 后端支持表与就绪清单；行状态由它们和扫描结果合成 */
  const supported = ref<BaseVersion[]>([]);
  const ready = ref<BaseVersion[]>([]);
  const entityAvailable = ref(false);

  /** 本机已装版本的扫描结果（id -> 命中）与读不出信息的目录说明 */
  const scanHits = ref<Map<string, McVersion>>(new Map());
  const scanWarnings = ref<string[]>([]);

  /** 勾选的待补版本；已就绪的行不可勾选 */
  const checked = ref<Set<string>>(new Set());

  /** 运行中的逐版本进度与上一批的结果，供弹窗展示 */
  const progress = ref<Map<string, BaseSetupProgress>>(new Map());
  const outcomes = ref<SetupVersionOutcome[]>([]);

  /** 就绪与已装本机的版本排在前面，其余按数据版本升序。
   *
   * 支持的正式版有 49 个，列表很长；用户最常看的是「我已经有哪些」——
   * 已建好基础库的、以及本机装了可以直接建的。排在前面就不必每次都滚动。
   * 组内保持数据版本升序：同一组里版本高低的相对顺序仍然可读。
   */
  const stateRank: Record<SetupRow["state"], number> = {
    ready: 0,
    local: 1,
    missing: 2,
  };

  const rows = computed<SetupRow[]>(() => {
    const list: SetupRow[] = supported.value.map((v): SetupRow => {
      const hit = scanHits.value.get(v.name);
      if (ready.value.some((r) => r.name === v.name)) {
        // 就绪的版本也带上本机 jar：重做某一版本（贴图库缺了、包损坏了）
        // 时能用本地文件直接重建，不必再下一次 20 多 MB 的客户端
        return {
          name: v.name,
          dataVersion: v.dataVersion,
          state: "ready",
          jarPath: hit?.jarPath ?? null,
        };
      }
      return hit
        ? { name: v.name, dataVersion: v.dataVersion, state: "local", jarPath: hit.jarPath }
        : { name: v.name, dataVersion: v.dataVersion, state: "missing", jarPath: null };
    });
    // sort 是稳定的：同组内保持原来的数据版本升序
    return list.sort((a, b) => stateRank[a.state] - stateRank[b.state]);
  });

  /** 列定义随界面语言变化，用 computed 包住，切语言后表头跟着变。
   *
   * 勾选列不在这里：它由 Table 的 row-selection 生成（表头带全选框）。 */
  const setupColumns = computed(() => [
    { title: t("setup.version"), dataIndex: "name", width: 110 },
    { title: t("setup.dataVersion"), dataIndex: "dataVersion", width: 90 },
    { title: t("setup.state"), slotName: "state", width: 110 },
    { title: t("setup.result"), slotName: "result" },
  ]);

  /** 状态刷新后把勾选重置为「本机已装的版本」。
   *
   * 只自动勾这类：用本地 jar 构建，不花流量也不耗时，用户来这个界面
   * 通常就是想把已装的先补齐。清单里没有的版本要联网下载几十 MB，
   * 勾不勾必须由用户决定——默认全勾会让人一不小心点出一个 GB 级任务。
   */
  function resetChecked() {
    checked.value = new Set(
      rows.value.filter((r) => r.state === "local").map((r) => r.name),
    );
  }

  /** 勾选变化：Arco 传回完整 keys 数组（含表头全选）。
   *
   * 全选会把需要联网下载的版本也勾上——那是几十 MB 到 GB 级的任务，因此只在
   * 用户明确点全选框时这么做（默认勾选仍只含本机已装的版本，见 resetChecked）。
   */
  function onVersionSelectionChange(keys: (string | number)[]) {
    checked.value = new Set(keys.map(String));
  }

  async function refreshStatus() {
    const st = await baseDbStatus();
    supported.value = st.supported;
    ready.value = st.versions;
    entityAvailable.value = st.entityAvailable;
    resetChecked();
  }

  /** 把本机已装的版本并进勾选，保留用户已经手动勾上的。
   *
   * 扫描结果比状态查询晚到（列表要先出来），不补这一步的话，
   * 扫出的本地版本不会出现在默认勾选里。
   */
  function addLocalToChecked() {
    const next = new Set(checked.value);
    for (const r of rows.value) {
      if (r.state === "local") next.add(r.name);
    }
    checked.value = next;
  }

  /** 记住目录与来源选择；失败只留日志，不该打断界面操作 */
  async function persistSetupState() {
    const st: SetupState = {
      version: SETUP_STATE_VERSION,
      mcDir: mcDir.value,
      source: source.value,
    };
    try {
      await saveSetupState(JSON.stringify(st));
    } catch (e) {
      // save_setup_state 的失败已经由 api.ts 的 logged 出口记进运行日志（带命令名），
      // 这里只留控制台一份，免得同一次失败出现两条
      console.warn("保存版本库状态失败：", e);
    }
  }

  /** 恢复上次的目录与来源；没有记录时退回官方启动器的默认路径 */
  async function restoreSetupState() {
    let raw: string | null = null;
    try {
      raw = await loadSetupState();
    } catch (e) {
      // load_setup_state 的失败已由 logged 记录，这里只留控制台一份
      console.warn("读取版本库状态失败：", e);
    }
    if (raw) {
      try {
        const st = JSON.parse(raw) as Partial<SetupState>;
        if (st.version === SETUP_STATE_VERSION) {
          if (typeof st.mcDir === "string") mcDir.value = st.mcDir;
          if (st.source === "official" || st.source === "bmcl") source.value = st.source;
        }
      } catch (e) {
        reportUiWarning("版本库状态内容无法解析，已忽略：", e);
      }
    }
    if (mcDir.value) return;
    try {
      const def = await defaultMinecraftDir();
      if (def) mcDir.value = def;
    } catch (e) {
      // default_minecraft_dir 的失败已由 logged 记录，这里只留控制台一份
      console.warn("定位默认 .minecraft 路径失败：", e);
    }
  }

  /** 扫描目录里的已装版本。
   *
   * 扫描是「找到即导入」：扫出的客户端 jar 直接构建进基础库，随后标记为已就绪，
   * 用户不必再去勾选。用本地 jar 构建不消耗流量，本来也没有需要用户决定的地方；
   * 真正需要用户点头的是「联网下载」那一类（清单里没有、本机也没有的版本）。
   *
   * 已经在库里的版本不重复构建。
   */
  async function rescan(autoImport = true) {
    const dir = mcDir.value.trim();
    if (!dir) {
      Message.warning(t("library.needDir"));
      return;
    }
    scanning.value = true;
    try {
      const r = await scanMinecraft(dir);
      scanHits.value = new Map(r.versions.map((v) => [v.id, v]));
      scanWarnings.value = r.warnings;
      addLocalToChecked();
      await persistSetupState();
      const fresh = rows.value.filter(
        (row) => row.state === "local" && !ready.value.some((x) => x.name === row.name),
      );
      if (fresh.length > 0) {
        Message.info(t("library.foundLocal", { count: fresh.length }));
      }
      if (autoImport && fresh.length > 0) {
        await runFor(fresh);
      }
    } catch (e) {
      Message.error(t("library.scanFailed", { error: String(e) }));
    } finally {
      scanning.value = false;
    }
  }

  /** 手动选择目录 */
  async function pickMcDir() {
    const dir = await open({
      directory: true,
      multiple: false,
      title: t("library.chooseDir"),
    });
    if (typeof dir !== "string" || !dir) return;
    mcDir.value = dir;
    await rescan();
  }

  async function selectSource(v: SetupSource) {
    source.value = v;
    await persistSetupState();
  }

  /** 打开弹窗：恢复状态、刷新基础库状态，目录已知时顺手扫描 */
  async function openSetup() {
    if (setupOpen.value) return;
    setupOpen.value = true;
    setupOnOpen.value = true;
    try {
      await restoreSetupState();
      await refreshStatus();
      if (mcDir.value) await rescan();
    } catch (e) {
      Message.error(t("library.statusFailed", { error: String(e) }));
    } finally {
      setupOnOpen.value = false;
    }
  }

  /** 执行一批版本的补库；返回是否全部就绪。
   *
   * 整批补库与「点一个未加载的版本」走的是同一条链路：都从 rows 取状态
   * （本机已装就用本地 jar，否则下载）、逐版本汇报进度、结束后刷新状态。
   */
  async function runFor(targets: SetupRow[]): Promise<boolean> {
    if (running.value) return false;
    running.value = true;
    outcomes.value = [];
    progress.value = new Map();
    // 订阅进度事件；结束后退订。listen 失败（窗口环境异常）只影响进度
    // 展示，不影响补库本身，因此退了订也要如实留日志
    let unlisten: (() => void) | null = null;
    try {
      unlisten = await onBaseSetupProgress((p) => {
        const next = new Map(progress.value);
        next.set(p.id, p);
        progress.value = next;
      });
    } catch (e) {
      reportUiWarning("订阅版本库进度事件失败：", e);
    }
    let allOk = false;
    try {
      const r = await setupAddVersions(
        source.value,
        targets.map((t) => ({
          id: t.name,
          dataVersion: t.dataVersion,
          jarPath: t.jarPath,
          mcDir: mcDir.value || null,
        })),
      );
      outcomes.value = r.outcomes;
      const failed = r.outcomes.filter((o) => !o.ok);
      allOk = r.outcomes.length > 0 && failed.length === 0;
      if (failed.length > 0) {
        Message.warning({
          content: t("library.batchPartial", {
            ok: r.outcomes.length - failed.length,
            failed: failed.length,
          }),
          duration: 8000,
        });
      } else {
        Message.success(t("library.batchAllReady", { count: r.outcomes.length }));
      }
      await refreshStatus();
      // 基础库可能刚被重建（语言表补齐、缺的版本补上），当前选中项的方块名与
      // 渲染图都来自旧库；这里立刻重解析一次，用户关掉弹窗就能看到新的名字，
      // 不必自己去再点一遍那一行
      await s.refreshSelectedAnalysis();
    } catch (e) {
      Message.error(t("library.batchFailed", { error: String(e) }));
    } finally {
      unlisten?.();
      running.value = false;
    }
    return allOk;
  }

  /** 开始准备勾选的版本：下载/使用本地 jar 并构建基础库 */
  async function startSetup() {
    const targets = rows.value.filter((r) => checked.value.has(r.name));
    if (targets.length === 0) {
      Message.warning(t("library.noChecked"));
      return;
    }
    await runFor(targets);
  }

  /** 直接准备某一个版本，供「版本转换」窗口点击未加载的版本时调用。
   *
   * 与版本库同一套规则：先定位 .minecraft、扫一次本机已装版本，
   * 本机装了的直接用本地 jar 构建（不花流量），没有的才下载。
   * 返回该版本最终是否就绪。
   */
  async function prepareVersion(name: string): Promise<boolean> {
    await restoreSetupState();
    await refreshStatus();
    if (ready.value.some((r) => r.name === name)) return true;
    // 本机版本还没扫过就扫一次：本地有就别下载几十 MB 的客户端。
    // 这里不自动导入其余版本——用户点的是某一个版本，替他顺带导入一批
    // 不是他要的结果（要整批导入请去「版本库」里扫描）
    if (mcDir.value && scanHits.value.size === 0) await rescan(false);
    const row = rows.value.find((r) => r.name === name);
    if (!row) {
      Message.error(t("library.versionUnknown", { name }));
      return false;
    }
    if (row.state === "missing") {
      Message.info(t("library.downloading", { name }));
    }
    return await runFor([row]);
  }

  onMounted(() => {
    // 启动时检查基础库：一个版本都没有就意味着方块检索和 3D 预览
    // 都没有数据可显示，直接弹出引导界面比让用户点了一圈才发现
    // 各处空白更友好
    void (async () => {
      try {
        const st = await baseDbStatus();
        supported.value = st.supported;
        ready.value = st.versions;
        entityAvailable.value = st.entityAvailable;
        if (st.versions.length === 0) await openSetup();
      } catch (e) {
        // base_db_status 的失败已由 logged 记录，这里只留控制台一份
        console.warn("启动检查版本库状态失败：", e);
      }
    })();
  });

  return {
    setupOpen,
    setupOnOpen,
    scanning,
    running,
    mcDir,
    source,
    supported,
    ready,
    entityAvailable,
    scanWarnings,
    checked,
    progress,
    outcomes,
    rows,
    setupColumns,
    onVersionSelectionChange,
    rescan,
    pickMcDir,
    selectSource,
    openSetup,
    startSetup,
    prepareVersion,
  };
}

export type SetupSlice = ReturnType<typeof createSetupSlice>;
