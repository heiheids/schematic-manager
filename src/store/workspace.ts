import { computed, ref, watch } from "vue";
import type { ComputedRef, Ref } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import { t } from "../i18n";
import type { SchematicSummary } from "../types";
import type { FileEditorSnapshot } from "./fileList";
import type { SceneEditorSnapshot } from "./scenePreview";
import type { ReplaceEditorSnapshot } from "./blockReplace";
import type { ConvertEditorSnapshot } from "./versionConvert";

export type WorkspaceTabKind = "files" | "settings" | "editor";

export interface WorkspaceTab {
  id: string;
  kind: WorkspaceTabKind;
  path?: string;
  title: string;
}

interface EditorSession {
  tabId: string;
  record: SchematicSummary;
  snapshot: {
    file: FileEditorSnapshot;
    scene: SceneEditorSnapshot;
    dialogs: { replace: ReplaceEditorSnapshot; convert: ConvertEditorSnapshot };
  } | null;
  generation: number;
}

export interface WorkspaceDeps {
  selected: Ref<SchematicSummary | null>;
  resetDraft: (record: SchematicSummary | null) => void;
  saving: Ref<boolean>;
  dirty: ComputedRef<boolean>;
  dirItems: Ref<Record<string, SchematicSummary[]>>;
  files: ComputedRef<SchematicSummary[]>;
  selectRecord: (
    record: SchematicSummary,
    options?: { confirm?: boolean },
  ) => Promise<boolean>;
  loadAnalysis: (record: SchematicSummary) => Promise<boolean>;
  invalidateAnalysis: () => void;
  captureFileEditor: () => FileEditorSnapshot | null;
  restoreFileEditor: (snapshot: FileEditorSnapshot) => void;
  clearEditor: () => void;
  captureSceneEditor: () => SceneEditorSnapshot;
  restoreSceneEditor: (snapshot: SceneEditorSnapshot) => void;
  invalidateScene: () => void;
  clearSceneEditor: () => void;
  loadScene: (record: SchematicSummary) => void;
  replaceOpen: Ref<boolean>;
  convOpen: Ref<boolean>;
  exportOpen: Ref<boolean>;
  backupOpen: Ref<boolean>;
  viewPresetOpen: Ref<boolean>;
  closeViewMenu: () => void;
  captureReplaceEditor: () => ReplaceEditorSnapshot;
  restoreReplaceEditor: (snapshot: ReplaceEditorSnapshot) => void;
  clearReplaceEditor: () => void;
  captureConvertEditor: () => ConvertEditorSnapshot;
  restoreConvertEditor: (snapshot: ConvertEditorSnapshot) => void;
  clearConvertEditor: () => void;
  invalidateCardPreviews: (paths?: string[]) => void;
}

function tabTitle(record: SchematicSummary): string {
  return record.fileName || record.name || record.path;
}

function isPathInside(path: string, root: string): boolean {
  const normalize = (value: string) => value.replace(/\\/g, "/").replace(/\/+$/, "");
  const normalizedPath = normalize(path);
  const normalizedRoot = normalize(root);
  return normalizedPath === normalizedRoot || normalizedPath.startsWith(`${normalizedRoot}/`);
}

export function createWorkspaceSlice(s: WorkspaceDeps) {
  const tabs = ref<WorkspaceTab[]>([
    { id: "files", kind: "files", title: t("workspace.files") },
    { id: "settings", kind: "settings", title: t("workspace.settings") },
  ]);
  const activeTabId = ref("files");
  const sessions = new Map<string, EditorSession>();
  let nextEditorId = 1;

  const activeTab = computed(() => tabs.value.find((tab) => tab.id === activeTabId.value));
  const activeEditorTab = computed(() =>
    activeTab.value?.kind === "editor" ? activeTab.value : null,
  );
  const activeSession = computed(() =>
    activeEditorTab.value ? sessions.get(activeEditorTab.value.id) ?? null : null,
  );
  const editorTabs = computed(() => tabs.value.filter((tab) => tab.kind === "editor"));

  function closeDialogs() {
    s.replaceOpen.value = false;
    s.convOpen.value = false;
    s.exportOpen.value = false;
    s.backupOpen.value = false;
    s.viewPresetOpen.value = false;
    s.closeViewMenu();
  }

  function canMove() {
    if (!s.saving.value) return true;
    Message.warning(t("workspace.saveInProgress"));
    return false;
  }

  function captureActive() {
    const session = activeSession.value;
    if (!session) return;
    const file = s.captureFileEditor();
    if (!file) return;
    session.record = file.record;
    session.snapshot = {
      file,
      scene: s.captureSceneEditor(),
      dialogs: {
        replace: s.captureReplaceEditor(),
        convert: s.captureConvertEditor(),
      },
    };
    session.generation++;
  }

  function insertFixedTab(id: "files" | "settings") {
    const existing = tabs.value.find((tab) => tab.id === id);
    if (existing) return existing;
    const tab: WorkspaceTab = {
      id,
      kind: id,
      title: t(id === "files" ? "workspace.files" : "workspace.settings"),
    };
    const index = id === "files" ? 0 : tabs.value.findIndex((x) => x.kind === "editor");
    const at = index < 0 ? tabs.value.length : index;
    tabs.value = [...tabs.value.slice(0, at), tab, ...tabs.value.slice(at)];
    return tab;
  }

  function restoreFixedTabs(kinds: WorkspaceTabKind[]) {
    const enabled = new Set(kinds.filter((kind) => kind === "files" || kind === "settings"));
    const fixed = (["files", "settings"] as const)
      .filter((kind) => enabled.has(kind))
      .map((kind) => ({
        id: kind,
        kind,
        title: t(kind === "files" ? "workspace.files" : "workspace.settings"),
      }));
    const editors = tabs.value.filter((tab) => tab.kind === "editor");
    tabs.value = [...fixed, ...editors];
    if (activeTab.value?.kind === "editor" && sessions.has(activeTab.value.id)) return;
    activeTabId.value = fixed[0]?.id ?? editors[0]?.id ?? "";
  }

  function openFiles() {
    insertFixedTab("files");
    return activateTab("files");
  }

  function openSettings() {
    insertFixedTab("settings");
    return activateTab("settings");
  }

  async function restoreSession(session: EditorSession) {
    const snapshot = session.snapshot;
    if (!snapshot) {
      s.clearEditor();
      s.clearSceneEditor();
      s.clearReplaceEditor();
      s.clearConvertEditor();
      await s.selectRecord(session.record, { confirm: false });
      return;
    }
    session.generation++;
    const generation = session.generation;
    s.restoreFileEditor(snapshot.file);
    s.restoreSceneEditor(snapshot.scene);
    s.restoreReplaceEditor(snapshot.dialogs.replace);
    s.restoreConvertEditor(snapshot.dialogs.convert);
    // 快照在请求尚未完成时可能只有部分内容，恢复后重新请求缺失部分。
    if (!snapshot.file.analysis && !session.record.error) {
      if (await s.loadAnalysis(session.record)) {
        if (generation === session.generation && activeTabId.value === session.tabId) {
          void s.loadScene(session.record);
        }
      }
    } else if (!snapshot.scene.scene && !snapshot.scene.sceneError) {
      void s.loadScene(session.record);
    }
  }

  async function activateTab(id: string): Promise<boolean> {
    if (id === activeTabId.value) return true;
    const target = tabs.value.find((tab) => tab.id === id);
    if (!target || !canMove()) return false;
    captureActive();
    closeDialogs();
    activeTabId.value = id;
    if (target.kind === "editor") {
      const session = sessions.get(id);
      if (session) await restoreSession(session);
    }
    return true;
  }

  async function openEditor(record: SchematicSummary): Promise<boolean> {
    const existing = editorTabs.value.find((tab) => tab.path === record.path);
    if (existing) return activateTab(existing.id);
    if (!canMove()) return false;
    captureActive();
    closeDialogs();
    const id = `editor-${nextEditorId++}`;
    const session: EditorSession = {
      tabId: id,
      record,
      snapshot: null,
      generation: 0,
    };
    sessions.set(id, session);
    tabs.value = [
      ...tabs.value,
      { id, kind: "editor", path: record.path, title: tabTitle(record) },
    ];
    activeTabId.value = id;
    return s.selectRecord(record, { confirm: false });
  }

  function activeEditorToken(): string {
    const session = activeSession.value;
    return session?.tabId ?? "";
  }

  function sessionDirty(session: EditorSession): boolean {
    if (activeTabId.value === session.tabId) return s.dirty.value;
    const snapshot = session.snapshot;
    return Boolean(snapshot && (snapshot.file.dirtyMeta || snapshot.file.pendingReplace));
  }

  async function confirmSessionClose(session: EditorSession): Promise<boolean> {
    if (!sessionDirty(session)) return true;
    try {
      await Modal.confirm({
        title: t("files.unsavedTitle"),
        content: t("workspace.closeUnsaved", { name: tabTitle(session.record) }),
        okText: t("files.unsavedDiscard"),
        cancelText: t("files.unsavedCancel"),
      });
      return true;
    } catch {
      return false;
    }
  }

  function removeSession(session: EditorSession) {
    session.generation++;
    if (activeTabId.value === session.tabId) {
      s.captureFileEditor();
      s.captureSceneEditor();
      s.captureReplaceEditor();
      s.captureConvertEditor();
    }
    sessions.delete(session.tabId);
    tabs.value = tabs.value.filter((tab) => tab.id !== session.tabId);
  }

  async function closeTab(id: string): Promise<boolean> {
    const tab = tabs.value.find((item) => item.id === id);
    if (!tab || !canMove()) return false;
    if (tab.kind === "editor") {
      const session = sessions.get(id);
      if (!session) return false;
      if (!(await confirmSessionClose(session))) return false;
    }
    const wasActive = activeTabId.value === id;
    const oldIndex = tabs.value.findIndex((item) => item.id === id);
    if (tab.kind === "editor") {
      const session = sessions.get(id);
      if (session) removeSession(session);
    } else {
      tabs.value = tabs.value.filter((item) => item.id !== id);
    }
    if (wasActive) {
      closeDialogs();
      const next = tabs.value[Math.min(oldIndex, tabs.value.length - 1)];
      activeTabId.value = next?.id ?? "";
      if (next?.kind === "editor") {
        const session = sessions.get(next.id);
        if (session) await restoreSession(session);
      } else if (!next) {
        s.clearEditor();
        s.clearSceneEditor();
      }
    }
    return true;
  }

  async function confirmDiscardAll(): Promise<boolean> {
    if (s.saving.value) {
      Message.warning(t("workspace.saveInProgress"));
      return false;
    }
    const dirtySessions = [...sessions.values()].filter(sessionDirty);
    if (dirtySessions.length === 0) return true;
    try {
      await Modal.confirm({
        title: t("files.unsavedTitle"),
        content: t("workspace.closeAllUnsaved", {
          names: dirtySessions.map((session) => tabTitle(session.record)).join("、"),
        }),
        okText: t("files.unsavedDiscard"),
        cancelText: t("files.unsavedCancel"),
      });
      return true;
    } catch {
      return false;
    }
  }

  function renameEditorPath(oldPath: string, newPath: string, newName: string) {
    for (const session of sessions.values()) {
      if (session.record.path !== oldPath) continue;
      session.record = { ...session.record, path: newPath, fileName: newName };
      if (session.snapshot) {
        session.snapshot.file.record = session.record;
      }
      const tab = tabs.value.find((item) => item.id === session.tabId);
      if (tab) {
        tab.path = newPath;
        tab.title = newName;
      }
    }
    tabs.value = [...tabs.value];
  }

  function activateFallback() {
    closeDialogs();
    const next = tabs.value[0];
    activeTabId.value = next?.id ?? "";
    if (next?.kind === "editor") {
      const session = sessions.get(next.id);
      if (session) void restoreSession(session);
    } else {
      s.clearEditor();
      s.clearSceneEditor();
    }
  }

  async function confirmRemoveEditorPaths(paths: string[]): Promise<boolean> {
    const gone = new Set(paths);
    const dirtySessions = [...sessions.values()].filter(
      (session) => gone.has(session.record.path) && sessionDirty(session),
    );
    if (dirtySessions.length === 0) return true;
    try {
      await Modal.confirm({
        title: t("files.unsavedTitle"),
        content: t("workspace.deleteUnsaved", {
          names: dirtySessions.map((session) => tabTitle(session.record)).join("、"),
        }),
        okText: t("files.unsavedDiscard"),
        cancelText: t("files.unsavedCancel"),
      });
      return true;
    } catch {
      return false;
    }
  }

  function removeEditorPaths(paths: string[]) {
    const gone = new Set(paths);
    for (const session of [...sessions.values()]) {
      if (gone.has(session.record.path)) removeSession(session);
    }
    if (!tabs.value.some((tab) => tab.id === activeTabId.value)) activateFallback();
  }

  /** 基础库变化后使所有会话的派生数据失效，激活时再重新加载。 */
  function invalidateEditors() {
    s.invalidateCardPreviews();
    for (const session of sessions.values()) {
      session.generation++;
      if (session.snapshot) {
        session.snapshot.file.analysis = null;
        session.snapshot.scene.scene = null;
        session.snapshot.scene.sceneError = "";
      }
    }
    const cur = s.selected.value;
    const tabId = activeEditorTab.value?.id;
    const session = tabId ? sessions.get(tabId) : undefined;
    const generation = session?.generation;
    if (cur && tabId && session) {
      s.invalidateAnalysis();
      s.invalidateScene();
      void s.loadAnalysis(cur).then((ok) => {
        if (
          ok &&
          activeTabId.value === tabId &&
          session.generation === generation &&
          s.selected.value?.path === cur.path
        ) {
          void s.loadScene(cur);
        }
      });
    }
  }

  /** 文件内容变化后只刷新发起操作的会话，隐藏会话在重新激活时加载。 */
  function refreshEditorContent(path: string, tabId = "") {
    const session = tabId
      ? sessions.get(tabId)
      : [...sessions.values()].find((item) => item.record.path === path);
    if (!session || session.record.path !== path) return;
    session.generation++;
    const generation = session.generation;
    if (session.snapshot) {
      session.snapshot.file.analysis = null;
      session.snapshot.scene.scene = null;
      session.snapshot.scene.sceneError = "";
    }
    if (activeTabId.value !== session.tabId) return;
    s.invalidateAnalysis();
    s.invalidateScene();
    void s.loadAnalysis(session.record).then((ok) => {
      if (ok && generation === session.generation && activeTabId.value === session.tabId) {
        void s.loadScene(session.record);
      }
    });
  }

  const missingConfirmationPaths = new Set<string>();

  async function reconcileMissingSessions(paths: string[]) {
    const missing = new Set(paths);
    const sessionsToClose = [...sessions.values()].filter((session) => missing.has(session.record.path));
    const dirtySessions = sessionsToClose.filter(sessionDirty);
    if (dirtySessions.length > 0) {
      try {
        await Modal.confirm({
          title: t("files.unsavedTitle"),
          content: t("workspace.missingUnsaved", {
            names: dirtySessions.map((session) => tabTitle(session.record)).join("、"),
          }),
          okText: t("files.unsavedDiscard"),
          cancelText: t("files.unsavedCancel"),
        });
      } catch {
        for (const path of missing) missingConfirmationPaths.delete(path);
        return;
      }
    }
    const roots = Object.keys(s.dirItems.value);
    const records = Object.values(s.dirItems.value).flat();
    const stillMissing = [...missing].filter(
      (path) =>
        roots.some((root) => isPathInside(path, root)) &&
        !records.some((record) => record.path === path),
    );
    removeEditorPaths(stillMissing);
    for (const path of missing) missingConfirmationPaths.delete(path);
  }

  // 扫描只更新摘要；无草稿会话同步表单，有草稿会话保留用户输入。
  watch(s.dirItems, () => {
    const roots = Object.keys(s.dirItems.value);
    const all = Object.values(s.dirItems.value).flat();
    const missingPaths: string[] = [];
    for (const session of sessions.values()) {
      const hit = all.find((record) => record.path === session.record.path);
      if (!hit) {
        if (
          roots.some((root) => isPathInside(session.record.path, root)) &&
          !missingConfirmationPaths.has(session.record.path)
        ) {
          missingConfirmationPaths.add(session.record.path);
          missingPaths.push(session.record.path);
        }
        continue;
      }
      missingConfirmationPaths.delete(session.record.path);
      session.record = hit;
      if (session.snapshot) {
        session.snapshot.file.record = hit;
        if (!sessionDirty(session)) {
          session.snapshot.file.draft = {
            fileName: hit.fileName,
            name: hit.name,
            author: hit.author,
            description: hit.description,
          };
          session.snapshot.file.dirtyMeta = false;
        }
      }
      if (activeTabId.value === session.tabId) {
        s.selected.value = hit;
        if (!s.dirty.value) s.resetDraft(hit);
      }
      const tab = tabs.value.find((item) => item.id === session.tabId);
      if (tab) tab.title = tabTitle(hit);
    }
    tabs.value = [...tabs.value];
    if (missingPaths.length > 0) void reconcileMissingSessions(missingPaths);
  });

  // 当前文件改名时同步活动标签；非活动标签由 renameEditorPath 处理。
  watch(
    () => s.selected.value?.path ?? "",
    (path, previous) => {
      const session = activeSession.value;
      if (!session || !path || path === previous) return;
      session.record = s.selected.value ?? session.record;
      const tab = tabs.value.find((item) => item.id === session.tabId);
      if (tab) {
        tab.path = path;
        tab.title = tabTitle(session.record);
      }
      if (session.snapshot) session.snapshot.file.record = session.record;
      tabs.value = [...tabs.value];
    },
  );

  return {
    tabs,
    activeTabId,
    activeTab,
    activeEditorTab,
    editorTabs,
    activeEditorToken,
    restoreFixedTabs,
    openFiles,
    openSettings,
    openEditor,
    activateTab,
    closeTab,
    confirmDiscardAll,
    confirmRemoveEditorPaths,
    renameEditorPath,
    removeEditorPaths,
    invalidateEditors,
    refreshEditorContent,
    captureActive,
  };
}

export type WorkspaceSlice = ReturnType<typeof createWorkspaceSlice>;
