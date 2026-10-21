/**
 * 备份与运行日志域：备份抽屉的清单、还原与删除，以及全局运行日志。
 *
 * 备份只跟「当前选中的投影」有关，日志是全局的。两者放成一个域，
 * 一是共用同一组刷新动作，二是日志同时供备份抽屉的页签和独立的
 * 「日志管理」窗口使用（见 components/LogModal.vue），两边共用同一份
 * logs 状态就不会出现一个清空了、另一个还显示旧内容。日志等级同理放在这里：
 * 它既决定后端收下哪些行，也要由界面状态域记住，只留一份才不会两处不一致。
 *
 * 与其他域的协作：还原成功后经 BackupsDeps 让文件列表域重扫、让 3D
 * 预览域重建场景，保证界面显示的是还原后的真实内容。
 * 依赖只声明本域用到的成员，避免类型上的循环引用（见 store/fileList.ts）。
 */
import { ref } from "vue";
import type { ComputedRef, Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { t } from "../i18n";
import {
  clearRunLog,
  configureRunLog,
  deleteBackup,
  listBackups,
  openRunLogDir,
  readRunLog,
  restoreBackup,
} from "../api";
import type { LogLevel } from "../utils/run_log";
import type { BackupEntry, SchematicSummary } from "../types";

/** 备份与日志域用到的其他域成员（循环引用回避见 fileList.ts） */
export interface BackupsDeps {
  /** 文件列表域的当前选中；备份清单与还原的作用对象 */
  selected: Ref<SchematicSummary | null>;
  /** 文件列表域的文件清单；还原后按路径找回同名文件 */
  files: ComputedRef<SchematicSummary[]>;
  /** 文件列表域的重扫；还原覆盖文件后刷新清单 */
  rescanAll: () => Promise<void>;
  /** 文件列表域的草稿复位；还原后重新载入元数据 */
  resetDraft: (rec: SchematicSummary) => void;
  /** 3D 预览域的场景加载；还原后重建场景 */
  loadScene: (record: SchematicSummary) => void;
}

/** 一次读取的日志条数上限。
 *
 * 「日志管理」窗口里的说明文案引用它，避免同一个数字在界面与这里各写一遍。
 */
const LOG_MAX_LINES = 300;

/** 用户没调过等级时的取值：常规操作已足够说明「程序做过什么」 */
const DEFAULT_LOG_LEVEL: LogLevel = "info";

/** 两位补零：日期与时间的字符串拼装都要用 */
function two(n: number): string {
  return String(n).padStart(2, "0");
}

export function createBackupsSlice(s: BackupsDeps) {
  // 备份与日志
  const backupOpen = ref(false);
  const backups = ref<BackupEntry[]>([]);
  const backupLoading = ref(false);
  const logs = ref<string[]>([]);
  const logLoading = ref(false);
  const logLevel = ref<LogLevel>(DEFAULT_LOG_LEVEL);
  /** 独立的「日志管理」窗口。与备份抽屉里的日志页签看的是同一份内容，
   *  入口在备份与日志的「运行日志」页签里。 */
  const logOpen = ref(false);

  /** 本地日期 YYYY-MM-DD。后端按它决定日志写进哪一天的文件、滚动时归档成什么名字。 */
  function localDate(): string {
    const d = new Date();
    return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())}`;
  }

  function formatTime(ns: string): string {
    if (!ns) return "—";
    // 非纯数字直接视为无法解析，而不是靠 BigInt 抛错再走 catch：
    // 把异常当正常分支用会掩盖真正的解析错误
    if (!/^\d+$/.test(ns)) return "—";
    // 纳秒用 BigInt 解析再转毫秒，避免超出安全整数范围导致时间错乱
    const ms = Number(BigInt(ns) / 1000000n);
    const d = new Date(ms);
    if (Number.isNaN(d.getTime())) return "—";
    // 逐段拼出 YYYY/MM/DD HH:mm:ss，不用 toLocaleString：后者按运行环境的
    // 区域设置排版，英文环境会给出 10/16/2026, 8:17:31 AM，同一份日志在不同
    // 机器上格式不一致，也让断言随环境时好时坏。
    const date = `${d.getFullYear()}/${two(d.getMonth() + 1)}/${two(d.getDate())}`;
    return `${date} ${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
  }

  async function refreshBackups() {
    backupLoading.value = true;
    try {
      backups.value = await listBackups(s.selected.value?.fileName.replace(/\.litematic$/i, ""));
    } catch (e) {
      Message.error(t("backup.loadFailed", { error: String(e) }));
    } finally {
      backupLoading.value = false;
    }
  }

  async function refreshLogs() {
    logLoading.value = true;
    try {
      logs.value = await readRunLog(LOG_MAX_LINES);
    } catch (e) {
      Message.error(t("backup.logLoadFailed", { error: String(e) }));
    } finally {
      logLoading.value = false;
    }
  }

  /** 把当前等级与本地日期推给后端。
   *
   * 后端按这套设置决定收下哪些行、写进哪一天的文件，因此界面恢复状态之后
   * 与用户改等级之后都要推一次，而不是只在启动时推。
   */
  async function syncRunLogLevel() {
    try {
      await configureRunLog(logLevel.value, localDate());
    } catch (e) {
      Message.error(t("backup.logLevelFailed", { error: String(e) }));
    }
  }

  /** 切换日志等级：记下来（界面状态域会持久化它），再同步给后端 */
  function setLogLevel(level: LogLevel) {
    logLevel.value = level;
    void syncRunLogLevel();
  }

  async function onRestore(row: BackupEntry) {
    const cur = s.selected.value;
    if (!cur) return;
    try {
      await restoreBackup(row.path, cur.path);
      Message.success(t("backup.restored"));
      await Promise.all([refreshBackups(), refreshLogs()]);
      // 还原后重新读取文件内容与预览，避免界面仍显示还原前的数据
      await s.rescanAll();
      const hit = s.files.value.find((f) => f.path === cur.path);
      if (hit) {
        s.selected.value = hit;
        s.resetDraft(hit);
        const idx = s.files.value.findIndex((f) => f.path === hit.path);
        if (idx >= 0) s.files.value[idx] = hit;
        void s.loadScene(hit);
      }
    } catch (e) {
      Message.error(t("backup.restoreFailed", { error: String(e) }));
    }
  }

  async function onDelete(row: BackupEntry) {
    try {
      await deleteBackup(row.path);
      Message.success(t("backup.deleted"));
      await Promise.all([refreshBackups(), refreshLogs()]);
    } catch (e) {
      Message.error(t("backup.deleteFailed", { error: String(e) }));
    }
  }

  async function openBackups() {
    backupOpen.value = true;
    // 抽屉里也有日志页签，因此和「日志管理」窗口一样顺手推一次当前日期：
    // 只开抽屉、从不打开窗口的用户，跨天后归档日期同样要刷新。
    // 后端只在等级或日期真的变了时才记一条，重复同步不会产生噪音。
    await Promise.all([refreshBackups(), refreshLogs(), syncRunLogLevel()]);
  }

  /** 打开「日志管理」窗口。打开即刷新：日志随时可能被其它操作写入，
   *  显示上一次打开时的旧内容会误导用户。 */
  async function openLogs() {
    logOpen.value = true;
    // 顺带把等级与**当前**本地日期再推一次：跨天挂机时，按大小滚动的归档
    // 会用启动那天的日期命名。失败提示由 syncRunLogLevel 自己弹，这里不重复。
    await Promise.all([refreshLogs(), syncRunLogLevel()]);
  }

  /** 清空运行日志。清空是有破坏性的，二次确认由界面负责。 */
  async function clearLogs() {
    try {
      await clearRunLog();
      Message.success(t("backup.logsCleared"));
      await refreshLogs();
    } catch (e) {
      Message.error(t("backup.clearFailed", { error: String(e) }));
    }
  }

  /** 在系统文件管理器里打开日志所在目录 */
  async function openLogDir() {
    try {
      await openRunLogDir();
    } catch (e) {
      Message.error(t("backup.openDirFailed", { error: String(e) }));
    }
  }

  return {
    backupOpen,
    backups,
    backupLoading,
    logs,
    logLoading,
    logLevel,
    logOpen,
    logMaxLines: LOG_MAX_LINES,
    formatTime,
    refreshBackups,
    refreshLogs,
    syncRunLogLevel,
    setLogLevel,
    onRestore,
    onDelete,
    openBackups,
    openLogs,
    clearLogs,
    openLogDir,
  };
}

export type BackupsSlice = ReturnType<typeof createBackupsSlice>;
