/**
 * 3D 预览与方块统计域：场景加载、视角/亮度/导出、方块渲染图缓存、
 * 方块统计表的列宽与文本导出。
 *
 * 这些状态都服务于右侧详情区：选中一个投影后，场景、统计、渲染图
 * 一起出现、一起失效（换投影时 resetBlockIcons 清空），放在一起
 * 才能看清这条「选中 → 加载 → 展示」链路的完整生命周期。
 *
 * 与其他域的协作：经 ScenePreviewDeps 读文件列表域的 selected/analysis，
 * 批量导出读取文件列表域的勾选项。依赖只声明本域用到的成员，
 * 而不是整棵 store，避免类型上的循环引用（见 store/fileList.ts）。
 */
import { computed, nextTick, ref, watch } from "vue";
import * as THREE from "three";
import type { ComputedRef, Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { TableColumnData } from "@arco-design/web-vue";
import { open, save } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import { displayName, t } from "../i18n";
import { buildScene, renderBlockIcons } from "../api";
import type SceneViewer from "../components/SceneViewer.vue";
import {
  OffscreenRenderer,
  anglesFromPosition,
  exportPixelSize,
  isBuiltinView,
  isPresetNameUsable,
  presetIsOrtho,
  sanitizeViewPreset,
} from "../render/scene3d";
import type { ExportAspect, SavedViewPreset } from "../render/scene3d";
import { NamePool, joinPath, previewStem } from "../render/export_name";
import type { AnalysisResult, BlockCount, BlockRow, SceneData, SchematicSummary } from "../types";
import { itemsForState } from "../utils/material_items";
import { formatStacks } from "../utils/format";

/** 3D 预览与方块统计域用到的其他域成员（循环引用回避见 fileList.ts） */
export interface ScenePreviewDeps {
  /** 文件列表域的当前选中；加载场景、导出命名都围绕它 */
  selected: Ref<SchematicSummary | null>;
  /** 文件列表域的方块分析结果；统计表与图标预取的数据源 */
  analysis: Ref<AnalysisResult | null>;
  /** 文件列表域已勾选且无解析错误的文件；批量导出的对象 */
  checkedFiles: ComputedRef<SchematicSummary[]>;
  /** 新文件选中时清除上一个文件的子区选择 */
  resetRegion: () => void;
}

export interface SceneCameraSnapshot {
  position: [number, number, number];
  target: [number, number, number];
  up?: [number, number, number];
  ortho?: boolean;
  zoom?: number;
  frustum?: { left: number; right: number; top: number; bottom: number };
}

export interface SceneEditorSnapshot {
  scene: SceneData | null;
  sceneError: string;
  activeRegion: string | null;
  activeView: string;
  orthoView: boolean;
  brightness: number;
  camera: SceneCameraSnapshot | null;
}

/**
 * 单个方块的块数超过这个数时，默认不生成 3D 图形。
 *
 * 三万块以上的场景要在浏览器里建几何并上传显存，会明显卡顿甚至让界面
 * 失去响应；等用户点「开始渲染」明确同意后再做，比默默卡住好。
 *
 * 口径是「统计表里的那一行」——即「方块」页签数量列看到的数：按物品合计、
 * 忽略朝向一类的状态差异。用户看到的数字与这里的判断因此是同一个。
 */
export const SCENE_BLOCK_LIMIT = 30000;

/** 按名称比较，结果与运行环境的语言数据无关。
 *
 * 不用 localeCompare：它随环境语言数据变化，同一份数据在不同机器上会排出
 * 不同顺序，用户换个系统就发现名次变了。
 */
function compareNames(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

export function createScenePreviewSlice(s: ScenePreviewDeps) {
  const viewer = ref<InstanceType<typeof SceneViewer> | null>(null);
  const scene = ref<SceneData | null>(null);
  const sceneLoading = ref(false);
  const sceneError = ref("");
  const brightness = ref(1.45);
  const activeView = ref("left45");
  const activeRegion = ref<string | null>(null);
  let sceneGeneration = 0;
  let cameraToRestore: SceneCameraSnapshot | null = null;
  /** 预览区右键菜单的位置；open 为 false 时不显示。
   *
   * 与投影列表的行右键菜单分开存放：那个作用于文件行，这个作用于 3D 画布，
   * 两者同时在屏上没有任何意义，混用一份状态会让它们互相顶掉。
   */
  const viewMenu = ref<{ open: boolean; x: number; y: number }>({
    open: false,
    x: 0,
    y: 0,
  });
  /** 当前是否正射投影。与 activeView 分开记录：用户可以手动切投影而
   *  不换角度，此时 activeView 仍然是原来那个键。 */
  const orthoView = ref(false);
  /** 用户保存的自定义视角预设 */
  const savedViews = ref<SavedViewPreset[]>([]);
  /** 「保存为预设」面板是否打开 */
  const viewPresetOpen = ref(false);
  const exportSize = ref<number>(1024);
  /** 导出尺寸的宽高比；与 exportSize（短边像素）一起决定实际宽高 */
  const exportAspect = ref<ExportAspect>("1x1");
  /** 当前导出尺寸换算出的像素宽高 */
  const exportPixels = computed(() => exportPixelSize(exportAspect.value, exportSize.value));
  const exporting = ref(false);
  /** 导出设置面板是否打开 */
  const exportOpen = ref(false);
  /** 本次打开导出面板是为了导出当前文件，还是批量导出勾选项 */
  const exportBatchMode = ref(false);
  /** 批量导出的进度（null 表示未在批量导出） */
  const exportingProgress = ref<{ done: number; total: number } | null>(null);

  /** 用户手动点过「开始渲染」的文件路径。
   *
   * 记路径而不是布尔：换了文件它自然失效，不必额外在切文件时清理——
   * 这正对应「离开该文件后自行停止渲染」。
   */
  const manualRenderPath = ref("");

  function resetRegion() {
    activeRegion.value = null;
  }

  /** 恢复活动视角；场景和组件都就绪后再调用，避免画布尚未挂载。 */
  async function restoreView() {
    await nextTick();
    if (cameraToRestore && viewer.value?.restoreCameraPose) {
      viewer.value.restoreCameraPose(cameraToRestore);
      cameraToRestore = null;
    } else if (activeView.value.startsWith("custom:")) {
      applySavedViewByName(activeView.value.slice(7));
      viewer.value?.setOrtho(orthoView.value);
    } else {
      viewer.value?.applyViewPreset(activeView.value);
      viewer.value?.setOrtho(orthoView.value);
    }
  }

  // 场景加载失败不弹消息：错误长期展示在预览区里（sceneError），
  // 与「点一下、弹一条就消失」的瞬时操作不同
  async function loadScene(record: SchematicSummary) {
    const regionName = activeRegion.value;
    const generation = ++sceneGeneration;
    scene.value = null;
    sceneError.value = "";
    // 方块数过大且用户还没点过「开始渲染」：不生成几何，预览区给出按钮。
    if (sceneGate.value) {
      sceneLoading.value = false;
      return;
    }
    sceneLoading.value = true;
    try {
      const data = await buildScene(record.path, regionName ?? undefined);
      if (
        generation !== sceneGeneration ||
        s.selected.value?.path !== record.path ||
        activeRegion.value !== regionName
      ) {
        return;
      }
      scene.value = data;
      await restoreView();
    } catch (e) {
      if (generation === sceneGeneration && s.selected.value?.path === record.path) {
        sceneError.value = String(e);
      }
    } finally {
      if (generation === sceneGeneration) sceneLoading.value = false;
    }
  }

  function setActiveRegion(name: string | null) {
    if (activeRegion.value === name) return;
    activeRegion.value = name;
    const cur = s.selected.value;
    if (cur) void loadScene(cur);
  }

  function captureSceneEditor(): SceneEditorSnapshot {
    sceneGeneration++;
    sceneLoading.value = false;
    return {
      scene: scene.value,
      sceneError: sceneError.value,
      activeRegion: activeRegion.value,
      activeView: activeView.value,
      orthoView: orthoView.value,
      brightness: brightness.value,
      camera: viewer.value?.cameraPose() ?? null,
    };
  }

  function restoreSceneEditor(snapshot: SceneEditorSnapshot) {
    sceneGeneration++;
    scene.value = snapshot.scene;
    sceneError.value = snapshot.sceneError;
    activeRegion.value = snapshot.activeRegion;
    activeView.value = snapshot.activeView;
    orthoView.value = snapshot.orthoView;
    brightness.value = snapshot.brightness;
    sceneLoading.value = false;
    manualRenderPath.value = "";
    cameraToRestore = snapshot.camera;
    void restoreView();
  }

  function invalidateScene() {
    sceneGeneration++;
    scene.value = null;
    sceneError.value = "";
    sceneLoading.value = false;
  }

  function clearSceneEditor() {
    invalidateScene();
    activeRegion.value = null;
    manualRenderPath.value = "";
    cameraToRestore = null;
  }

  /** 用户明确要求渲染当前文件（方块数超过阈值时用） */
  async function startRender() {
    const cur = s.selected.value;
    if (!cur) return;
    manualRenderPath.value = cur.path;
    await loadScene(cur);
  }

  // 离开该文件后手动渲染的授权随之失效：回来要重新点一次。
  // 对应「渲染持续到玩家离开该文件预览为止」——不这么做的话，授权会一直
  // 挂着，下次再打开同一个大文件时又会自动渲染起来。
  watch(
    () => s.selected.value?.path ?? "",
    (path) => {
      if (manualRenderPath.value && manualRenderPath.value !== path) {
        manualRenderPath.value = "";
      }
    },
  );

  const sceneInfo = computed(() => {
    const cur = scene.value;
    // 三角形数改到 3D 视图右下角常驻显示，这条只留「有哪些方块没贴图」
    // 这类需要提醒的异常，正常时整条不显示
    if (!cur || cur.unresolvedBlocks <= 0) return "";
    return t("export.unresolved", { count: cur.unresolvedBlocks });
  });

  /** 切到某个视角预设。
   *
   * 投影方式由预设自己决定（真等距是正射，其余是透视），因此统一走
   * applyViewPreset 而不是 applyPreset——否则切到「真等距」时只会换角度、
   * 投影仍是上一次的。
   */
  function setView(name: string) {
    activeView.value = name;
    // 记下这次切换带上了投影方式：右键菜单里「正射投影」的勾选状态、
    // 以及下次保存自定义预设都要读它
    orthoView.value = presetIsOrtho(name);
    viewer.value?.applyViewPreset(name);
  }

  /** 手动切换正射/透视，不改角度。
   *
   * 与 setView 的区别：它只换投影方式，当前朝向保持不变。
   * 用户想「用同一个角度看看正射什么效果」时用这个。
   */
  function setOrthoView(ortho: boolean) {
    orthoView.value = ortho;
    viewer.value?.setOrtho(ortho);
  }

  /** 在预览区某处打开右键菜单 */
  function openViewMenu(x: number, y: number) {
    viewMenu.value = { open: true, x, y };
  }

  function closeViewMenu() {
    if (viewMenu.value.open) viewMenu.value = { ...viewMenu.value, open: false };
  }

  /** 把当前视角存成一条自定义预设。
   *
   * 位姿可传可不传：不传就取当前相机的快照（「照我现在看到的存」），
   * 传了就按传入的值存（面板里改过角度之后存的就是改后的值）。
   *
   * 候选预设必须过一遍 `sanitizeViewPreset`——那是从磁盘读回来时用的同一套
   * 校验。写读两侧用同一条规则，才不会出现「存得进、重启后被静默丢弃」：
   * 位置与目标重合、坐标里有 NaN 这类值当次看着正常，重开就没了。
   *
   * 返回失败原因（用于提示），成功返回空字符串。名字不可用、或场景还没就绪
   * 时不能存——存一条点下去没反应的预设比不存更糟。
   */
  function saveCurrentView(
    name: string,
    edited?: {
      ortho: boolean;
      position: [number, number, number];
      target: [number, number, number];
    },
  ): string {
    if (!scene.value) return t("view.saveNotReady");
    const snapshot = viewer.value?.cameraPose() ?? null;
    const pose = edited ?? snapshot;
    if (!pose) return t("view.saveNotReady");
    const ortho = edited ? edited.ortho : orthoView.value;
    if (!isPresetNameUsable(name, savedViews.value)) {
      return savedViews.value.some((p) => p.name === name.trim())
        ? t("view.saveDuplicate")
        : t("view.saveInvalidName");
    }
    const { yaw, pitch } = anglesFromPosition(
      new THREE.Vector3(...pose.position),
      new THREE.Vector3(...pose.target),
    );
    const candidate = sanitizeViewPreset({
      name: name.trim(),
      ortho,
      position: pose.position,
      target: pose.target,
      yaw,
      pitch,
    });
    if (!candidate) return t("view.saveInvalidPose");
    savedViews.value = [...savedViews.value, candidate];
    return "";
  }

  /** 删除一条自定义预设 */
  function removeSavedView(name: string) {
    savedViews.value = savedViews.value.filter((p) => p.name !== name);
  }

  /** 应用一条自定义预设 */
  function applySavedViewByName(name: string) {
    const preset = savedViews.value.find((p) => p.name === name);
    if (!preset) return;
    orthoView.value = preset.ortho;
    activeView.value = `custom:${name}`;
    viewer.value?.applySavedView(preset);
  }

  /** 导出当前视角为透明背景 PNG（单个文件）。
   *
   * 尺寸由导出面板确认后传入：面板里既能选预设，也能手填宽高。
   * 不传时退回当前记住的尺寸，供「不改设置直接再导一次」这类调用使用。
   */
  async function exportPreview(size?: { w: number; h: number }) {
    const cur = s.selected.value;
    if (!cur || !scene.value) return;
    const px = size ?? exportPixels.value;
    const stem = cur.fileName.replace(/\.litematic$/i, "");
    // 同 pickDirectory：保存对话框失败时必须给出提示，
    // 放在 try 外会让这个 Promise 静默 reject，用户点了导出却没反应
    let target: unknown;
    try {
      target = await save({
        title: t("export.previewTitle"),
        defaultPath: `${stem}_preview_${px.w}x${px.h}.png`,
        filters: [{ name: t("export.pngFilter"), extensions: ["png"] }],
      });
    } catch (e) {
      Message.error(t("export.saveDialogFailed", { error: String(e) }));
      return;
    }
    if (typeof target !== "string") return;

    exporting.value = true;
    try {
      const bytes = await viewer.value?.exportPng(px.w, px.h);
      if (!bytes) throw new Error(t("export.notReady"));
      await writeFile(target, bytes);
      Message.success(t("export.previewExported", { w: px.w, h: px.h }));
    } catch (e) {
      Message.error(t("export.failed", { error: String(e) }));
    } finally {
      exporting.value = false;
    }
  }

  /** 选择批量导出的目标目录 */
  async function pickExportDir(): Promise<string | null> {
    try {
      const dir = await open({
        directory: true,
        multiple: false,
        title: t("export.chooseDir"),
      });
      return typeof dir === "string" ? dir : null;
    } catch (e) {
      // 同其它对话框：失败必须提示，否则用户点了没反应
      Message.error(t("export.pickDirFailed", { error: String(e) }));
      return null;
    }
  }

  /**
   * 批量导出该用哪个视角。
   *
   * `activeView` 有两种取值：内置预设的键，或 `custom:<名字>`。
   * 后者要去 `savedViews` 里查出实际位姿——把 `custom:xxx` 当内置键传下去
   * 会被查表兜底成正面透视，用户拿到一整批角度不对的图却没有任何提示。
   *
   * 查不到（预设被删、状态脏了）时返回 null，由调用方明确报错。
   */
  function currentExportView(): string | SavedViewPreset | null {
    const name = activeView.value;
    if (!name.startsWith("custom:")) {
      return isBuiltinView(name) ? name : null;
    }
    const preset = savedViews.value.find((p) => `custom:${p.name}` === name);
    return preset ?? null;
  }

  /**
   * 批量导出预览图。
   *
   * 用独立的离屏渲染器逐个渲染，而不是把界面切到每个文件再导出：
   * 后者会反复冲掉用户正在看的画面，还要等每次界面重建，明显更慢。
   *
   * 单个文件失败不中断整批——某个投影读不出来（被占用、格式异常）时
   * 其余文件仍应导出成功，最后把失败清单如实报出来。
   */
  async function exportBatch(size?: { w: number; h: number }) {
    const list = s.checkedFiles.value.filter((f) => !f.error);
    if (list.length === 0) {
      Message.warning(t("export.noChecked"));
      return;
    }
    const dir = await pickExportDir();
    if (!dir) return;

    const px = size ?? exportPixels.value;
    // 当前视角可能是一条自定义预设：那时 activeView 是 `custom:名字`，
    // 不能当内置视角名传下去，否则会静默退回正面透视
    const view = currentExportView();
    if (view === null) {
      Message.warning(t("export.viewUnavailable"));
      return;
    }
    exporting.value = true;
    exportingProgress.value = { done: 0, total: list.length };
    let renderer: OffscreenRenderer | null = null;
    const failures: string[] = [];
    let ok = 0;
    // 不同目录下可能有同名投影，直接按文件名写会互相覆盖——
    // 导出 10 张最后只剩 1 张，而且没有任何提示。因此用带序号的命名池。
    const names = new NamePool();

    try {
      renderer = new OffscreenRenderer();
      for (const f of list) {
        try {
          const data = await buildScene(f.path);
          const bytes = await renderer.renderPng(data, px.w, px.h, view);
          const name = names.take(previewStem(f.fileName, px.w, px.h), ".png");
          await writeFile(joinPath(dir, name), bytes);
          ok += 1;
        } catch (e) {
          failures.push(t("export.batchFailureEntry", { name: f.fileName, error: String(e) }));
        }
        exportingProgress.value = { done: ok + failures.length, total: list.length };
      }
    } catch (e) {
      // 离屏渲染器都建不起来（例如 WebGL 上下文耗尽），整批无法进行
      Message.error(t("export.batchFailed", { error: String(e) }));
      return;
    } finally {
      renderer?.dispose();
      exporting.value = false;
      exportingProgress.value = null;
    }

    if (failures.length === 0) {
      Message.success(t("export.batchDone", { count: ok, w: px.w, h: px.h, dir }));
    } else {
      Message.warning({
        content: t("export.batchPartial", {
          ok,
          fail: failures.length,
          list: failures.join("\n"),
        }),
        duration: 8000,
      });
    }
  }

  // ---------------------------------------------------------------- 方块统计与渲染图

  /** 方块栏列。
   *
   * 中文名在前、ID 在后：玩家认的是「橡树树叶」而不是「minecraft:oak_leaves」，
   * 但出问题时仍需要看到 ID，因此两者都保留。
   * 中文名查不到时由 slot 回退显示 ID（见模板里的 zh 插槽）。
   *
   * 状态列只在「区分属性」开启时出现：按物品合计的那些行里，一个状态只是
   * 该行的代表状态，数量却是这一行所有状态的总和，写出来反而误导。
   *
   * 最后一列是固定的操作列，每行一个「替换」按钮（入口跟着它作用的那一行，
   * 不必先选中再去找表头按钮）。固定的列没有拖动手柄——它的宽度不参与
   * 布局计算，固定值是列定义里的常量。
   */
  /** 列定义随界面语言变化，用 computed 包住，切语言后表头跟着变。
   *  名字列走展示插槽：显示哪一套名字由界面语言决定，后端两套都给了。 */
  const blockColumns = computed<TableColumnData[]>(() => {
    const cols: TableColumnData[] = [
      { title: t("stats.icon"), slotName: "icon", width: 60 },
      {
        title: t("displayName.column"),
        slotName: "display",
        width: 150,
        ellipsis: true,
        tooltip: true,
      },
      // 方块名走插槽：按物品合计时一个物品可能跨了几种方块形态（火把与墙上的
      // 火把），而状态列在合计视图下不显示，跨形态这件事只能标在这一列上
      {
        title: t("stats.block"),
        dataIndex: "name",
        slotName: "name",
        ellipsis: true,
        // 这一列自己给 title（跨形态时列出具体是哪几个方块名），
        // Arco 的 tooltip 会把整段文字再套一层，两个提示会叠在一起
        tooltip: false,
      },
    ];
    if (blockSplitStates.value) {
      cols.push({
        title: t("stats.state"),
        dataIndex: "state",
        ellipsis: true,
        tooltip: true,
      });
    }
    cols.push(
      { title: t("stats.count"), dataIndex: "count", width: 90 },
      { title: t("stats.stacks"), slotName: "stacks", width: 110 },
      {
        title: t("stats.action"),
        slotName: "act",
        // 英文界面要放得下 "Action" 与 "Replace"：单元格左右各有 16px 内边距，
        // 太窄会折行（中文字少，窄一点反而好看）
        width: 96,
        fixed: "right",
      },
    );
    return cols;
  });

  /** 方块栏列宽：由用户拖动表头调整，与侧栏同一套做法。
   *
   * 单独一份而不是共用侧栏的 colWidths：两张表的列完全不同，
   * 共用会让拖动侧栏把方块栏的宽度也改掉。
   */
  const blockColWidths = ref<Record<string, number>>({});
  /** 方块栏表格的 key：复位列宽靠重建（Arco 内部记了宽度，外部改不生效） */
  const blockTableKey = ref(0);

  function onBlockColumnResize(dataIndex: string, width: number) {
    if (!dataIndex) return;
    blockColWidths.value = { ...blockColWidths.value, [dataIndex]: width };
  }

  /** 实际渲染用的方块栏列：把用户调整过的宽度套回去 */
  const blockTableColumns = computed<TableColumnData[]>(() =>
    blockColumns.value.map((c) => {
      const key = String(c.dataIndex ?? "");
      const w = key ? blockColWidths.value[key] : undefined;
      return w ? { ...c, width: w } : c;
    }),
  );

  function resetBlockColumnWidths() {
    blockColWidths.value = {};
    blockTableKey.value += 1;
  }

  /** 方块栏每页行数。渲染图按页加载，所以这个值同时决定了加载节奏。 */
  const BLOCK_PAGE_SIZE = 50;
  /** 一次请求多少张渲染图。远小于后端的单批上限，避免一次塞进过大的 IPC 负载。 */
  const ICON_BATCH = 128;
  const blockPage = ref(1);

  /** 是否「区分属性」：开启后每种方块状态单独一行，关闭则按物品合计。 */
  const blockSplitStates = ref(false);

  /** 按物品合计的行：「建造这个东西一共要备多少」。 */
  function mergedBlockRows(all: BlockCount[]): BlockRow[] {
    const out: BlockRow[] = [];
    const at = new Map<string, number>();
    // 该物品当前的代表行是否已经是「基础形态」（方块名就等于物品名）。
    // 行的 name 一律是物品名（水折成水桶、耕地折成泥土），所以不能靠比较
    // name 判断代表行取自哪种摆法——墙上的火把与火把的物品名相同。
    const baseForm = new Set<string>();
    for (const b of all) {
      for (const item of itemsForState(b.state)) {
        const add = b.count * item.count;
        const i = at.get(item.name);
        if (i === undefined) {
          at.set(item.name, out.length);
          out.push({
            ...b,
            key: item.name,
            name: item.name,
            count: add,
            stateCount: 1,
            forms: [b.name],
          });
          if (b.name === item.name) baseForm.add(item.name);
          continue;
        }
        const row = out[i];
        row.count += add;
        row.stateCount += 1;
        if (!row.forms.includes(b.name)) row.forms.push(b.name);
        // 基础形态优先当代表：中文名与渲染图取它的，用户看到的是「火把」
        // 而不是「墙上的火把」
        if (b.name === item.name && !baseForm.has(item.name)) {
          baseForm.add(item.name);
          row.zhName = b.zhName;
          row.enName = b.enName;
          row.state = b.state;
        }
      }
    }
    // 合计后的数量与后端给出的单状态数量不同，名次要重排：否则「用了 8000 个
    // 的红石粉」会排在「用了 12 个的石头」后面。数量相同按名字排，结果稳定
    //（不用 localeCompare：它随环境语言数据变化，同一份数据在不同机器上会排出不同顺序）。
    out.sort((a, b) => b.count - a.count || compareNames(a.name, b.name));
    return out;
  }

  /** 按状态分行的行：一行就是一个方块状态，数量是处于该状态的方块数。 */
  function splitStateRows(all: BlockCount[]): BlockRow[] {
    const out = all.map<BlockRow>((b) => ({
      ...b,
      key: b.state,
      stateCount: 1,
      forms: [b.name],
    }));
    // 不依赖后端返回顺序：切换开关时行要按同一套规则重排，否则同一个方块在
    // 两种视图里的名次会不同，用户会以为数量变了
    out.sort((a, b) => b.count - a.count || compareNames(a.state, b.state));
    return out;
  }

  /** 方块统计表的行。
   *
   * 默认按**物品**合计，照投影模组 Litematica 的材料清单口径：后端给的是
   * 「每个方块状态用了多少个」（红石粉的强度、铁轨的朝向与通电、楼梯的朝向……
   * 一个方块可以占几十行），先把状态折算成「建造它所需的物品与个数」
   * （见 utils/material_items.ts），再按物品累加，数量是要备的物品数。
   *
   * 打开「区分属性」后改为每种状态一行，数量是该状态的方块数——这是替换规则
   * 的实际命中口径（替换按方块名与属性精确匹配），与材料口径并列存在。
   * 两种视图的差别不只在分组：数量本身含义不同，因此界面上要分别说明。
   */
  const blockRows = computed<BlockRow[]>(() => {
    const all = s.analysis.value?.blocks ?? [];
    return blockSplitStates.value ? splitStateRows(all) : mergedBlockRows(all);
  });

  // 行集合整个换了，停在旧页码会落到一个不知从何而来的位置
  watch(blockSplitStates, () => {
    blockPage.value = 1;
  });

  /** 数量最多的那一行方块（与统计表同一口径，也就是用户看到的那个数） */
  const heaviestBlock = computed(() => {
    const rows = blockRows.value;
    if (rows.length === 0) return null;
    return rows.reduce((a, b) => (b.count > a.count ? b : a));
  });

  /** 需要用户先点「开始渲染」的方块；不需要时为 null */
  const sceneGate = computed(() => {
    const heavy = heaviestBlock.value;
    const cur = s.selected.value;
    if (!heavy || !cur || heavy.count <= SCENE_BLOCK_LIMIT) return null;
    return manualRenderPath.value === cur.path ? null : heavy;
  });

  /** 当前页的行。渲染图只给可见的这批，整表一次性加载会有几 MB 的 data URL。 */
  const blockPageRows = computed<BlockRow[]>(() => {
    const start = (blockPage.value - 1) * BLOCK_PAGE_SIZE;
    return blockRows.value.slice(start, start + BLOCK_PAGE_SIZE);
  });

  /** 状态键 → data URL。用普通 Map 而不是 reactive：
   * 这里会有几百上千项，逐项做响应式代理得不偿失，
   * 改动后统一靠 iconVersion 触发重绘。 */
  const iconCache = new Map<string, string>();
  /** 状态键 → 渲染失败原因。同样需要缓存，否则每次翻页都会重试同一个坏方块。 */
  const iconErrors = new Map<string, string>();
  /** 渲染图缓存的版本号：Map 本身不是响应式的，靠它通知模板重新求值 */
  const iconVersion = ref(0);
  /** 换投影后旧请求的结果必须丢弃，否则会把上一个投影的图贴到新表上 */
  let iconGeneration = 0;

  /** 取某行的渲染图；没有时返回空串。 */
  function iconOf(state: string): string {
    void iconVersion.value; // 建立响应式依赖
    return iconCache.get(state) ?? "";
  }

  /** 取某行渲染图的失败原因；成功或尚未加载时返回空串。 */
  function iconErrorOf(state: string): string {
    void iconVersion.value;
    return iconErrors.get(state) ?? "";
  }

  /** 按需加载这一页里还没有渲染图的行。 */
  async function ensureBlockIcons(rows: BlockRow[]) {
    const dataVersion = s.selected.value?.dataVersion ?? 0;
    if (dataVersion <= 0) return;
    const missing = rows
      .map((r) => r.state)
      .filter((st) => !iconCache.has(st) && !iconErrors.has(st));
    if (missing.length === 0) return;

    const gen = iconGeneration;
    for (let i = 0; i < missing.length; i += ICON_BATCH) {
      const chunk = missing.slice(i, i + ICON_BATCH);
      let icons;
      try {
        icons = await renderBlockIcons(dataVersion, chunk);
      } catch (e) {
        if (gen !== iconGeneration) return;
        // 整批失败通常是方块库出了问题。把这一批标记成已处理，
        // 否则每次翻回这一页都会重试并再弹一次同样的错误。
        const reason = t("export.iconRenderFailed", { error: String(e) });
        for (const st of chunk) iconErrors.set(st, reason);
        iconVersion.value += 1;
        Message.error(reason);
        return;
      }
      if (gen !== iconGeneration) return;
      for (const icon of icons) {
        if (icon.error) iconErrors.set(icon.state, icon.error);
        else if (icon.dataUrl) iconCache.set(icon.state, icon.dataUrl);
      }
      iconVersion.value += 1;
    }
  }

  /** 换了投影就把渲染图与页码一起重置：缓存是按状态键存的，
   * 跨投影复用看起来没问题，但不同版本的同一状态可能贴图不同。
   *
   * 必须先把 generation 加一，正在飞的那批请求才会被丢弃；
   * 否则它们的回调会按老投影的图写进刚清空的缓存。 */
  function resetBlockIcons() {
    iconGeneration += 1;
    iconCache.clear();
    iconErrors.clear();
    iconVersion.value += 1;
    blockPage.value = 1;
  }

  watch(blockPageRows, (rows) => void ensureBlockIcons(rows), { immediate: true });

  /** 导出方块统计为文本表格（制表符分隔，可直接粘进 Excel）。
   *
   * 用制表符而不是逗号：方块名里含 `:` 与 `[`，但不会有制表符；
   * 名字里若真有制表符或换行会被替换掉，避免一行数据被拆成两行。
   * 行取合并后的 blockRows，与界面上看到的一致——导出与显示对不上
   * 会让人以为导出漏了方块。
   */
  function blockTableText(): string {
    const rows: string[] = [];
    const head = [t("export.tableName"), t("export.tableId")];
    // 状态列与界面同步：界面上没有状态列时不导出它，否则拿到文件的人会以为
    // 界面上也按状态分了行、数量口径与界面不同
    if (blockSplitStates.value) head.push(t("export.tableState"));
    head.push(t("export.tableCount"), t("export.tableStacks"));
    rows.push(head.join("\t"));
    for (const b of blockRows.value) {
      // 导出的是当前界面语言看得懂的表格，但截断/落盘后语言不会再变，
      // 所以按导出时的语言取名字
      const name = displayName(b).replace(/[\t\r\n]/g, " ");
      const cells = [name, b.name];
      if (blockSplitStates.value) cells.push(b.state.replace(/[\t\r\n]/g, " "));
      cells.push(String(b.count), formatStacks(b.count));
      rows.push(cells.join("\t"));
    }
    return rows.join("\r\n");
  }

  /** 把方块统计导出为 .txt 文件（用户可选保存位置） */
  async function exportBlockTable() {
    const blocks = blockRows.value;
    if (blocks.length === 0) {
      Message.warning(t("export.tableEmpty"));
      return;
    }
    const stem =
      s.selected.value?.fileName?.replace(/\.litematic$/i, "") || t("export.tableStem");
    const target = await save({
      defaultPath: `${stem}${t("export.tableSuffix")}`,
      filters: [{ name: t("export.textFilter"), extensions: ["txt"] }],
    });
    if (!target) return; // 用户取消
    try {
      await writeFile(target, new TextEncoder().encode(blockTableText()));
      Message.success(t("export.tableExported", { count: blocks.length, path: target }));
    } catch (e) {
      Message.error(t("export.failed", { error: String(e) }));
    }
  }

  /** 复制方块表到剪贴板 */
  async function copyBlockTable() {
    const blocks = blockRows.value;
    if (blocks.length === 0) {
      Message.warning(t("export.tableCopyEmpty"));
      return;
    }
    try {
      await navigator.clipboard.writeText(blockTableText());
      Message.success(t("export.tableCopied", { count: blocks.length }));
    } catch (e) {
      Message.error(t("export.copyFailed", { error: String(e) }));
    }
  }

  /** 实体栏列。与方块栏同理：名字列在前，ID 保留。 */
  const entityColumns = computed<TableColumnData[]>(() => [
    {
      title: t("displayName.column"),
      slotName: "display",
      width: 150,
      ellipsis: true,
      tooltip: true,
    },
    { title: t("stats.entityId"), dataIndex: "id" },
    { title: t("stats.count"), dataIndex: "count", width: 90 },
  ]);

  return {
    viewer,
    scene,
    sceneLoading,
    sceneError,
    brightness,
    activeView,
    activeRegion,
    setActiveRegion,
    resetRegion,
    captureSceneEditor,
    restoreSceneEditor,
    invalidateScene,
    clearSceneEditor,
    restoreView,
    viewMenu,
    orthoView,
    savedViews,
    viewPresetOpen,
    saveCurrentView,
    removeSavedView,
    applySavedViewByName,
    setOrthoView,
    openViewMenu,
    closeViewMenu,
    exportSize,
    exportAspect,
    exportPixels,
    exportOpen,
    exportBatchMode,
    exporting,
    exportingProgress,
    loadScene,
    startRender,
    sceneGate,
    sceneBlockLimit: SCENE_BLOCK_LIMIT,
    sceneInfo,
    setView,
    exportPreview,
    pickExportDir,
    exportBatch,
    blockColumns,
    blockColWidths,
    blockTableKey,
    onBlockColumnResize,
    blockTableColumns,
    resetBlockColumnWidths,
    BLOCK_PAGE_SIZE,
    blockPage,
    blockSplitStates,
    blockRows,
    blockPageRows,
    iconOf,
    iconErrorOf,
    resetBlockIcons,
    blockTableText,
    exportBlockTable,
    copyBlockTable,
    entityColumns,
  };
}

export type ScenePreviewSlice = ReturnType<typeof createScenePreviewSlice>;
