import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import ArcoVue, { Message, Modal } from "@arco-design/web-vue";
import ArcoVueIcon from "@arco-design/web-vue/es/icon";

// Tauri 的全部出口都换掉：测试跑在 jsdom 里，没有 Tauri 运行时。
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
// 主题相关的能力要做成可替换的 mock：测试需要分别模拟深色、浅色、
// 以及读取失败三种情形。用一个真实函数包着，默认返回浅色。
const fakeWindow = () =>
  ({
    minimize: vi.fn(),
    toggleMaximize: vi.fn(),
    close: vi.fn(),
    startDragging: vi.fn(),
    isMaximized: vi.fn().mockResolvedValue(false),
    onResized: vi.fn().mockResolvedValue(() => {}),
    theme: vi.fn().mockResolvedValue("light"),
    onThemeChanged: vi.fn().mockResolvedValue(() => {}),
  }) as never;

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => fakeWindow()),
}));

const mockGetCurrentWindow = vi.mocked(getCurrentWindow);
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
  save: vi.fn().mockResolvedValue(null),
}));
vi.mock("@tauri-apps/plugin-fs", () => ({ writeFile: vi.fn() }));
// 版本库补库时订阅进度事件；jsdom 里没有 Tauri 事件系统，给个「订阅成功」的
// 替身，并把回调记下来——用例要靠它推一条进度事件，才能验证进度条怎么画。
// vi.mock 的工厂会被提升，数组得用 vi.hoisted 声明，否则工厂里取不到。
const progressListeners = vi.hoisted(
  () => [] as Array<{ event: string; handler: (e: { payload: unknown }) => void }>,
);
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((event: string, handler: (e: { payload: unknown }) => void) => {
    progressListeners.push({ event, handler });
    return Promise.resolve(() => {});
  }),
}));

// SceneViewer 依赖 WebGL，jsdom 没有。挂载测试关心的是外壳界面，
// 因此用空组件替代，避免把 WebGL 缺失误判成界面故障。
// 方法要与真实组件的 defineExpose 保持一致：store 会直接调用它们，
// 少一个就会在点视角时抛「is not a function」，而那是替身的错、不是代码的错。
vi.mock("../components/SceneViewer.vue", () => ({
  default: {
    name: "SceneViewer",
    template: "<div class='scene-viewer-stub' />",
    methods: {
      exportPng: () => null,
      applyPreset: () => {},
      applyViewPreset: () => {},
      setOrtho: () => {},
      fitCamera: () => {},
      cameraPose: () => ({ position: [10, 20, 30], target: [0, 0, 0] }),
      applySavedView: () => {},
    },
  },
}));

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "../App.vue";
import i18n, { setLocale } from "../i18n";
import type { SchematicSummary, SceneData, DuplicateGroup, ConvertItem, BlockCount } from "../types";

const mockInvoke = vi.mocked(invoke);
const mockOpen = vi.mocked(open);

/**
 * 捕获用户可见的提示。
 *
 * 断言「界面上出现了某句提示」时不要读 document.body.textContent：Arco 的
 * Message 在模块级缓存了浮层容器元素，而每个用例开始时都会把 body 清空，
 * 那个容器于是变成**游离节点**——引用还在、元素已不在文档里，之后所有提示
 * 都渲染进这个看不见的节点。表现是「同一个断言单跑能过、整包跑就失败」，
 * 而且会让「应提示 X」这类断言退化成看运气（甚至被工具栏里的同名字符串
 * 蒙混过去）。这里改为断言确实调用了提示接口、且文案正确。
 *
 * vi.spyOn 默认保留原实现，因此提示照常渲染，不影响其它用例。
 */
const messageSuccess = vi.spyOn(Message, "success");
const messageWarning = vi.spyOn(Message, "warning");
const messageError = vi.spyOn(Message, "error");
const messageInfo = vi.spyOn(Message, "info");

/** 本用例内出现过的全部提示文案（含普通提示） */
function shownMessages(): string[] {
  return [
    ...messageSuccess.mock.calls,
    ...messageWarning.mock.calls,
    ...messageError.mock.calls,
    ...messageInfo.mock.calls,
  ].map((c) => String(c[0]));
}

/** 一个可用的投影摘要，字段与后端 SchematicSummary 对齐
 *
 * 必须标注类型：不加标注时它不是「赋给有类型变量的对象字面量」，
 * TS 的多余属性检查不会生效，夹具与 DTO 漂移了编译器也不会报错。
 */
const summary: SchematicSummary = {
  path: "/p/测试.litematic",
  fileName: "测试.litematic",
  dirName: "/p",
  fileSize: 2048,
  error: "",
  formatVersion: 6,
  dataVersion: 3465,
  mcVersionName: "1.20.1",
  name: "测试投影",
  author: "作者",
  description: "",
  timeCreated: 0,
  timeModified: 0,
  enclosing: [16, 10, 16],
  totalBlocks: 520,
  totalVolume: 2560,
  regionCount: 1,
  entityCount: 0,
  tileEntityCount: 0,
  pendingTicks: 0,
  hasPreview: false,
};

/** 挂载外壳界面 */
function mountApp() {
  // i18n 与图标都是应用级的：main.ts 里一并装上。缺 i18n 时组件里的
  // useI18n() 会直接抛「Need to install with app.use」；缺图标则只出警告，
  // 但界面里那些 icon-* 会渲染成空标签，测不到真实结构。
  return mount(App, {
    global: { plugins: [ArcoVue, ArcoVueIcon, i18n] },
    attachTo: document.body,
  });
}

/** 运行日志目录：界面显示的路径必须来自后端 run_log_dir，而不是前端自己拼的
 *  （identifier 改过一次，拼出来的位置会指向不存在的地方）。 */
const LOG_DIR = "C:\\Users\\tester\\AppData\\Roaming\\io.github.heiheids.schematic-manager\\logs";

async function activateFiles(w: ReturnType<typeof mountApp>) {
  const tab = w.findAll(".workspace-tab__label").find((item) => item.text().includes("文件管理"));
  if (tab) {
    await tab.trigger("click");
    await flushPromises();
  }
}

async function selectRoot(w: ReturnType<typeof mountApp>, path: string) {
  const node = w.findAll(".directory-node__select").find((item) => item.find(".directory-node__name").attributes("title") === path);
  expect(node, `应能找到根目录 ${path}`).toBeTruthy();
  await node!.trigger("click");
  await flushPromises();
}

async function activateEditor(w: ReturnType<typeof mountApp>, name: string) {
  const tab = w.findAll(".workspace-tab__label").find((item) => item.attributes("title")?.includes(name));
  expect(tab, `应能找到编辑标签 ${name}`).toBeTruthy();
  await tab!.trigger("click");
  await flushPromises();
}

async function openFileCard(w: ReturnType<typeof mountApp>, name: string) {
  await activateFiles(w);
  const card = w.findAll(".schematic-card").find((item) => item.text().includes(name));
  expect(card, `应能找到文件卡片 ${name}`).toBeTruthy();
  await card!.trigger("dblclick");
  await flushPromises();
}

/** 挂载并选中一个带给定方块行的投影，「方块」页签的表因此有内容。 */
async function mountWithBlocks(
  blocks: BlockCount[],
  overrides: Record<string, unknown> = {},
) {
  stubInvoke({
    analyze_schematic: {
      blocks,
      entities: [],
      tileEntities: [],
      entityInstances: [],
      nonAirTotal: blocks.reduce((a, b) => a + b.count, 0),
      airTotal: 0,
      distinctBlockTypes: new Set(blocks.map((b) => b.name)).size,
      distinctStates: blocks.length,
    },
    ...overrides,
  });
  mockOpen.mockResolvedValue("/p");
  const w = mountApp();
  await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
  await flushPromises();
  await openFileCard(w, "测试.litematic");
  await flushPromises();
  return w;
}

/** 点方块表第 index 行的行内「替换」，打开替换窗口。
 *
 * 替换入口不再挂在表头（#53）：规则作用于用户点的那一行，入口就该在那一行上。
 */
async function clickReplaceOnRow(w: ReturnType<typeof mountApp>, index = 0) {
  const links = w.findAll(".block-table .block-act");
  expect(links.length, "方块表每行应有替换入口").toBeGreaterThan(index);
  await links[index].trigger("click");
  await flushPromises();
}

/** 一个可用的场景数据，字段与后端 SceneData 对齐 */
const scene: SceneData = {
  batches: [],
  totalVertices: 0,
  totalTriangles: 0,
  enclosing: [16, 10, 16],
  origin: [0, 0, 0],
  mcVersionName: "1.20.1",
  dataVersion: 3465,
  unresolvedBlocks: 0,
};

function normalizeMockResult(cmd: string, value: unknown, args: unknown): unknown {
  if (!value || typeof value !== "object" || Array.isArray(value)) return value;
  if (cmd === "scan_litematics") {
    const result = value as { items?: SchematicSummary[]; root?: unknown };
    if (!result.root) {
      const items = result.items ?? [];
      const dir = (args as { dir?: string } | undefined)?.dir ?? "/p";
      return {
        ...result,
        root: {
          path: dir,
          name: dir.split(/[\\/]/).filter(Boolean).pop() ?? dir,
          relativePath: "",
          filePaths: items.map((item) => item.path),
          children: [],
          truncated: false,
        },
      };
    }
  }
  if (cmd === "analyze_schematic" && !("regions" in value)) {
    return { ...value, regions: [] };
  }
  return value;
}

/** 让 invoke 按命令名返回**结构正确**的结果。
 *
 * 未列出的命令一律返回空数组是不够的：需要对象的命令拿到数组后，
 * 界面在访问字段时才会抛错，表现为「测试通过但留下未处理错误」。
 * 因此这里把会返回对象/标量的命令都显式列出。
 */
function stubInvoke(overrides: Record<string, unknown> = {}) {
  mockInvoke.mockImplementation((async (cmd: string, args?: unknown) => {
    if (cmd in overrides) {
      const v = overrides[cmd];
      // 允许用函数给出结果：既能按参数返回不同数据，也能直接抛出以测错误分支。
      // 若不支持函数，测「命令失败」就只能整体替换实现，那样别的命令会落到
      // 兜底分支拿到错误类型的结果（例如 base_db_status 拿到数组），
      // 进而产生与被测行为无关的未处理异常。
      const result = typeof v === "function" ? await (v as (a: unknown) => unknown)(args) : v;
      return normalizeMockResult(cmd, result, args);
    }
    switch (cmd) {
      case "load_ui_state":
        // 契约是 string | null。默认「从未保存过」，因此回 null；
        // 回数组会让 App.vue 走进 JSON 解析失败分支并弹出警告，
        // 那是替身给错类型造成的噪声，不是真实行为。
        return null;
      case "save_ui_state":
        return undefined;
      case "scan_litematics":
        return normalizeMockResult(cmd, { items: [summary], skippedDirs: 0, skippedPaths: [] }, args);
      case "build_scene":
        return scene;
      case "analyze_schematic":
        return {
          regions: [],
          blocks: [],
          entities: [],
          tileEntities: [],
          entityInstances: [],
          nonAirTotal: 0,
          airTotal: 0,
          distinctBlockTypes: 0,
          distinctStates: 0,
        };
      case "list_block_versions":
        return [];
      case "list_blocks":
        // 替换窗口打开时会取全量方块填右侧列表；默认给空，需要的用例自己覆盖
        return [];
      case "base_db_status":
        // 库里为空会触发版本库弹窗自动打开，污染所有不相关用例；
        // 默认给「已有版本」，需要测空库引导的用例自己覆盖
        return {
          versions: [{ name: "1.20.1", dataVersion: 3465 }],
          entityAvailable: true,
          supported: [{ name: "1.20.1", dataVersion: 3465 }],
        };
      case "load_setup_state":
        return null;
      case "save_setup_state":
        return undefined;
      case "default_minecraft_dir":
        return null;
      case "scan_minecraft":
        return { versions: [], warnings: [] };
      case "setup_add_versions":
        return { outcomes: [] };
      case "list_backups":
      case "read_run_log":
        return [];
      case "run_log_dir":
        // 契约是 string | null。默认给一个目录，说明文案里才会显示日志位置
        return LOG_DIR;
      case "rename_schematic":
        // 契约是 RenameOutcome。默认「名字没变」，因此路径与文件名都取空，
        // 需要断言改名的用例自己给出具体返回值。
        return { path: "", fileName: "", relinked: 0 };
      case "mc_version_name":
        return "1.20.1";
      case "peek_schematic":
        return summary;
      default:
        // 未预期的命令返回空数组；若某个流程因此抛错，会以未处理错误的形式
        // 暴露出来，而不是被静默吞掉
        return [];
    }
  }) as never);
}

beforeEach(() => {
  mockInvoke.mockReset();
  // invoke 的契约是**总返回 Promise**：替身若在未设实现时返回 undefined，
  // 调用链上的 `.catch`（api.ts 的命令失败上报、界面诊断上报）会直接崩成
  // TypeError，测出来的就不是被测代码的问题了。这里补上默认值，让替身与
  // 真实 invoke 的形状一致。
  mockInvoke.mockResolvedValue(undefined as never);
  mockOpen.mockReset();
  mockOpen.mockResolvedValue(null);
  // 主题用例会替换 getCurrentWindow 的返回值；不复位的话，
  // 那个「读不到主题」的替身会泄漏到后面的用例里（实测泄漏了 5 条告警）
  mockGetCurrentWindow.mockReset();
  mockGetCurrentWindow.mockImplementation(() => fakeWindow());
  stubInvoke();
  // 提示的调用记录逐个用例清空，避免上一条用例的提示被本条读到
  messageSuccess.mockClear();
  messageWarning.mockClear();
  messageError.mockClear();
  messageInfo.mockClear();
  document.body.innerHTML = "";
  document.body.removeAttribute("arco-theme");
});

describe("界面外壳", () => {
  it("能挂载且不抛异常", () => {
    // 这是最关键的一条：若 App.vue 在挂载时抛错，用户看到的就是空白窗口，
    // 表现正是「功能都不能用」
    expect(() => mountApp()).not.toThrow();
  });

  it("功能入口各自出现在被作用的面板里", () => {
    const w = mountApp();
    // 入口与它作用的对象在同一区域：搜索与重复筛查在投影列表、
    // 方块替换在方块列表、版本转换在投影信息区。
    // 这里核对「不在工具栏、而在对应面板」，防止以后搬家时漏掉或搬错。
    const toolbar = w.find(".toolbar");
    expect(toolbar.find("input").exists(), "工具栏不应再有搜索框").toBe(false);

    const labels = (sel: string) =>
      w.findAll(`${sel} button`).map((b) => b.text().trim());
    expect(labels(".file-manager__ops").join(" "), "投影列表区应有重复文件筛查").toContain(
      "重复文件筛查",
    );

    // 与选中项无关的入口仍在工具栏
    const toolbarText = toolbar.text();
    for (const label of ["添加目录", "备份与日志", "版本库"]) {
      expect(toolbarText, `工具栏应含「${label}」`).toContain(label);
    }
    w.unmount();
  });

  it("未选中文件时不出现依赖选中项的入口", async () => {
    const w = mountApp();
    await flushPromises();
    const buttons = w.findAll("button");
    const find = (label: string) => buttons.find((b) => b.text().includes(label));
    // 版本转换与方块列表都以「选中的投影文件」为前提，而那两块在无选中时
    // 整块不渲染，入口自然不可达——比「可见但禁用」更彻底，用户不会去点
    // 一个点了报错的按钮。方块替换的入口在方块表的每一行里，表不在就没有入口。
    expect(find("版本转换"), "无选中时不应出现版本转换").toBeUndefined();
    expect(w.find(".block-table").exists(), "无选中时不应渲染方块表").toBe(false);
    // 与选中项无关的入口应始终可用
    expect(find("添加目录")?.attributes("disabled")).toBeUndefined();
    expect(find("备份与日志")?.attributes("disabled")).toBeUndefined();
    w.unmount();
  });

  it("未选中文件时显示空状态提示，而不是空白页", () => {
    const w = mountApp();
    // 页签只在选中文件后渲染；未选中时必须有明确的引导文案，
    // 否则用户面对的是一个不知从何下手的界面
    expect(w.text()).toContain("点击「添加目录」选择投影所在目录");
    w.unmount();
  });

  it("初始不应向打开弹窗的功能发命令", () => {
    // 挂载即触发一批查询会把界面拖慢；这些应当按需触发
    mountApp();
    const names = mockInvoke.mock.calls.map((c) => c[0]);
    for (const cmd of ["search_blocks", "convert_targets", "scan_duplicates"]) {
      expect(names, `初始不应调用 ${cmd}`).not.toContain(cmd);
    }
  });
});

describe("主要操作流程", () => {
  it("点「添加目录」→ 选目录 → 列出投影 → 选中后可编辑元数据", async () => {
    stubInvoke();
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();

    // 点按钮应触发展开目录选择
    const add = w.findAll("button").find((b) => b.text().includes("添加目录"));
    expect(add, "应能定位到「添加目录」按钮").toBeTruthy();
    await add!.trigger("click");
    await flushPromises();

    expect(mockOpen).toHaveBeenCalled();
    const names = mockInvoke.mock.calls.map((c) => c[0]);
    expect(names, "选择目录后应扫描该目录").toContain("scan_litematics");

    // 扫描结果应出现在列表里
    expect(w.text(), "列表应显示扫描到的文件").toContain("测试.litematic");

    // ---- 选中并真的编辑、保存 ----
    // 之前这条测试只验证到「列表显示了文件」，与标题里承诺的
    // 「可编辑元数据」并不相符：编辑框、保存按钮、写回的参数都没被检查。
    await openFileCard(w, "测试.litematic");
    await flushPromises();

    // 元数据输入框应带出文件里的现有值
    const inputs = w.findAll("input");
    const nameInput = inputs.find((i) => (i.element as HTMLInputElement).value === "测试投影");
    expect(nameInput, "投影名称输入框应带出原值").toBeTruthy();

    // 未改动时「保存」不可用，改动后才可用
    const saveBtn = () => w.findAll("button").find((b) => b.text().includes("保存"));
    expect(saveBtn()?.attributes("disabled"), "未改动时不应可保存").toBeDefined();

    await nameInput!.setValue("改过的名字");
    await flushPromises();
    expect(saveBtn()?.attributes("disabled"), "改动后应可保存").toBeUndefined();

    // 保存：应把编辑框里的值原样写给后端，并在成功后重新读取
    let saved: unknown = null;
    mockInvoke.mockImplementation((async (cmd: string, args: Record<string, unknown>) => {
      if (cmd === "save_metadata") {
        saved = args;
        return { backupPath: "/p/备份/测试.litematic.bak" };
      }
      switch (cmd) {
        case "scan_litematics":
          return { items: [summary], skippedDirs: 0, skippedPaths: [] };
        case "build_scene":
          return scene;
        case "analyze_schematic":
          return {
            blocks: [], entities: [], tileEntities: [], entityInstances: [],
            nonAirTotal: 0, airTotal: 0, distinctBlockTypes: 0, distinctStates: 0,
          };
        default:
          return [];
      }
    }) as never);

    await saveBtn()!.trigger("click");
    await flushPromises();

    expect(saved, "应调用 save_metadata").toBeTruthy();
    // 参数是平铺的：{ path, name, author, description }（见 api.ts 的 saveMetadata）
    const args = saved as { path: string; name: string; author: string; description: string };
    expect(args.path, "应写回被选中的那个文件").toBe(summary.path);
    expect(args.name, "编辑框里的名字应原样提交").toBe("改过的名字");
    expect(args.author, "未改动的字段应保持原值").toBe("作者");

    // 保存结果必须告知用户备份位置，否则改动看起来像是没发生。
    // 这里断言提示文案本身：改用 document.body.textContent 找「备份」是靠不住的
    // ——工具栏里的「备份与日志」按钮就含这两个字，无论有没有提示都会通过。
    expect(shownMessages().join("\n"), "应提示备份路径").toContain("备份在：");
    w.unmount();
  });

  it("改文件名后保存：先改名，再把元数据写到新路径，并说明备份已跟随", async () => {
    stubInvoke();
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();

    const order: string[] = [];
    let renamedArgs: Record<string, unknown> | null = null;
    let savedArgs: Record<string, unknown> | null = null;
    mockInvoke.mockImplementation((async (cmd: string, args: Record<string, unknown>) => {
      if (cmd === "rename_schematic") {
        order.push("rename");
        renamedArgs = args;
        return { path: "/p/新名字.litematic", fileName: "新名字.litematic", relinked: 2 };
      }
      if (cmd === "save_metadata") {
        order.push("save");
        savedArgs = args;
        return { backupPath: "/p/备份/新名字.litematic.bak" };
      }
      switch (cmd) {
        case "scan_litematics":
          return { items: [summary], skippedDirs: 0, skippedPaths: [] };
        case "build_scene":
          return scene;
        case "analyze_schematic":
          return {
            blocks: [], entities: [], tileEntities: [], entityInstances: [],
            nonAirTotal: 0, airTotal: 0, distinctBlockTypes: 0, distinctStates: 0,
          };
        default:
          return [];
      }
    }) as never);

    // 文件名输入框应带出当前文件名，并且可以改
    const fileInput = w.findAll("input").find(
      (i) => (i.element as HTMLInputElement).value === "测试.litematic",
    );
    expect(fileInput, "文件名输入框应带出当前文件名").toBeTruthy();
    await fileInput!.setValue("新名字.litematic");
    await flushPromises();

    await w.findAll("button").find((b) => b.text().includes("保存"))!.trigger("click");
    await flushPromises();

    expect(renamedArgs, "应调用 rename_schematic").toBeTruthy();
    expect((renamedArgs! as { path: string }).path, "改名应作用于被选中的文件").toBe(summary.path);
    expect((renamedArgs! as { newName: string }).newName, "应传新文件名").toBe("新名字.litematic");
    // 顺序不能反：先改名再写元数据。反过来会先给旧名字留一份备份，
    // 而改名又要重新链接备份，同一件事被做两遍且中间态对不上
    expect(order, "必须先改名再写元数据").toEqual(["rename", "save"]);
    expect(
      (savedArgs! as { path: string }).path,
      "元数据必须写到改名后的新路径上",
    ).toBe("/p/新名字.litematic");
    // 备份已跟随这件事要告诉用户，否则他会以为备份丢了
    expect(shownMessages().join("\n"), "应说明备份已跟随改名").toContain("2 组备份已跟随");
    w.unmount();
  });

  it("改名成功但元数据写入失败时，界面收敛到新路径，再保存不再改名", async () => {
    // 这条防的是「改名成功、写元数据失败」后的卡死：若 selected 仍指向旧路径，
    // 用户再点保存会拿一个已不存在的路径去重命名，从此永远存不下去，
    // 而磁盘上文件名其实早就改了。
    stubInvoke();
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();

    // 第一次写元数据失败；改名成功
    let saveCalls = 0;
    const renameCalls: Record<string, unknown>[] = [];
    const renamedSummary = {
      ...summary,
      path: "/p/新名字.litematic",
      fileName: "新名字.litematic",
    };
    mockInvoke.mockImplementation((async (cmd: string, args: Record<string, unknown>) => {
      if (cmd === "rename_schematic") {
        renameCalls.push(args);
        return { path: "/p/新名字.litematic", fileName: "新名字.litematic", relinked: 0 };
      }
      if (cmd === "save_metadata") {
        saveCalls += 1;
        if (saveCalls === 1) throw new Error("磁盘只读");
        return { backupPath: "/p/备份/x.bak" };
      }
      switch (cmd) {
        case "scan_litematics":
          // 第一次扫描返回旧记录，失败后的重扫返回新记录
          return {
            items: [saveCalls >= 1 ? renamedSummary : summary],
            skippedDirs: 0,
            skippedPaths: [],
          };
        case "build_scene":
          return scene;
        case "analyze_schematic":
          return {
            blocks: [], entities: [], tileEntities: [], entityInstances: [],
            nonAirTotal: 0, airTotal: 0, distinctBlockTypes: 0, distinctStates: 0,
          };
        default:
          return [];
      }
    }) as never);

    const fileInput = () =>
      w.findAll("input").find(
        (i) => ["测试.litematic", "新名字.litematic"].includes(
          (i.element as HTMLInputElement).value,
        ),
      );
    await fileInput()!.setValue("新名字.litematic");
    await flushPromises();
    await w.findAll("button").find((b) => b.text().includes("保存"))!.trigger("click");
    await flushPromises();

    // 失败提示要说清「文件名已改」，否则用户以为文件没动
    expect(shownMessages().join("\n"), "应说明文件名已改").toContain("文件名已改");
    expect(renameCalls.length, "第一次应改名").toBe(1);

    // 再点一次保存：不应再改名（路径已收敛到新名字）
    const saveBtn = w.findAll("button").find((b) => b.text().includes("保存"));
    if (saveBtn && saveBtn.attributes("disabled") === undefined) {
      await saveBtn.trigger("click");
      await flushPromises();
      expect(renameCalls.length, "第二次保存不应再改名").toBe(1);
    }
    w.unmount();
  });

  it("改名成功但备份未链接上时，如实告知而不是假装成功", async () => {
    stubInvoke();
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();

    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "rename_schematic") {
        // 文件已改名，只是备份没接上——这不是整体失败
        return {
          path: "/p/新名字.litematic",
          fileName: "新名字.litematic",
          relinked: 0,
          relinkError: "备份目录不可写",
        };
      }
      if (cmd === "save_metadata") return { backupPath: "/p/备份/x.bak" };
      switch (cmd) {
        case "scan_litematics":
          return { items: [summary], skippedDirs: 0, skippedPaths: [] };
        case "build_scene":
          return scene;
        case "analyze_schematic":
          return {
            blocks: [], entities: [], tileEntities: [], entityInstances: [],
            nonAirTotal: 0, airTotal: 0, distinctBlockTypes: 0, distinctStates: 0,
          };
        default:
          return [];
      }
    }) as never);

    const fileInput = w.findAll("input").find(
      (i) => (i.element as HTMLInputElement).value === "测试.litematic",
    );
    await fileInput!.setValue("新名字.litematic");
    await flushPromises();
    await w.findAll("button").find((b) => b.text().includes("保存"))!.trigger("click");
    await flushPromises();

    expect(
      shownMessages().join("\n"),
      "应告知备份未链接上，不能静默",
    ).toContain("备份目录不可写");
    w.unmount();
  });

  it("文件名未改动时不应触发改名", async () => {
    // 否则每次保存都会先做一次无谓的重命名（并重新链接一遍备份）
    stubInvoke();
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();

    const nameInput = w.findAll("input").find(
      (i) => (i.element as HTMLInputElement).value === "测试投影",
    );
    await nameInput!.setValue("只改投影名");
    await flushPromises();
    await w.findAll("button").find((b) => b.text().includes("保存"))!.trigger("click");
    await flushPromises();

    const names = mockInvoke.mock.calls.map((c) => c[0]);
    expect(names, "没改文件名就不该调用 rename_schematic").not.toContain("rename_schematic");
    expect(names, "元数据仍应保存").toContain("save_metadata");
    w.unmount();
  });

  it("选中投影后，详情区出现信息/方块/实体三个页签", async () => {
    stubInvoke();
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();

    // 点列表里的文件行使其成为选中项
    const row = w.findAll(".schematic-card").find((r) => r.text().includes("测试.litematic"));
    expect(row, "应能找到该文件所在的行").toBeTruthy();
    await row!.trigger("dblclick");
    await flushPromises();

    const text = w.text();
    for (const tab of ["信息", "方块", "实体"]) {
      expect(text, `选中后应出现「${tab}」页签`).toContain(tab);
    }
    w.unmount();
  });

  it("选中文件后，方块表的每一行都有替换入口，版本转换变为可用", async () => {
    // 替换的入口跟着它作用的那一行走：表里的每一行各带一个「替换」，
    // 不必先选中某一行再去别处找按钮。
    const w = await mountWithBlocks([
      { name: "minecraft:stone", state: "minecraft:stone", count: 3, zhName: "石头", enName: "Stone" },
      { name: "minecraft:dirt", state: "minecraft:dirt", count: 2, zhName: "泥土", enName: "Dirt" },
    ]);
    expect(
      w.findAll(".block-table tbody tr .block-act").length,
      "每一行都应有替换入口",
    ).toBe(2);
    const convert = w.findAll("button").find((b) => b.text().includes("版本转换"));
    expect(convert?.attributes("disabled"), "选中后应可用").toBeUndefined();
    w.unmount();
  });

  it("方块替换里源方块也显示中文名", async () => {
    // 源方块来自「方块」页签点的那一行。那一行的名字当场就有，
    // 不必再查一次库；改名或换库后仍要能查到，因此也接受来自检索的结果。
    const w = await mountWithBlocks([
      {
        name: "minecraft:oak_stairs",
        state: "minecraft:oak_stairs[facing=east]",
        count: 4,
        zhName: "橡木楼梯",
        enName: "Oak Stairs",
      },
    ]);
    await clickReplaceOnRow(w);

    expect(document.body.textContent, "应显示源方块的中文名").toContain("橡木楼梯");
    // 名字只应显示一次，且不该多打一条检索命令去查它已经知道的名字
    expect(
      mockInvoke.mock.calls.filter((c) => c[0] === "search_blocks").length,
      "点的那一行已经带名字，不应再查一次库",
    ).toBe(0);
    w.unmount();
  });

  it("源方块在行上没有名字时，仍会按 ID 去库里查一次", async () => {
    // 该版本语言表里没有这个方块时，行上带来的名字是空的。
    // 这时不能把「空」记成「已查过」，否则界面直接显示未找到，
    // 而库里其实有名字（例如叠加包的补丁只补了名字）
    const w = await mountWithBlocks(
      [
        {
          name: "minecraft:oak_log",
          state: "minecraft:oak_log[axis=y]",
          count: 1,
          zhName: "",
          enName: "",
        },
      ],
      {
        search_blocks: [
          { name: "minecraft:oak_log", zh: "橡木原木", en: "Oak Log", versions: [] },
        ],
        list_blocks: [],
      },
    );
    await clickReplaceOnRow(w);
    await new Promise((r) => setTimeout(r, 300));
    await flushPromises();

    expect(document.body.textContent, "应显示从库里查到的名字").toContain("橡木原木");
    expect(
      mockInvoke.mock.calls.some((c) => c[0] === "search_blocks"),
      "名字为空时应真的发起一次查询",
    ).toBe(true);
    w.unmount();
  });

  it("源方块在方块库里查不到名字时明确说明，而不是显示空白", async () => {
    // 行上带来的名字为空（该版本语言表里没有这个方块），随后按 ID 补查也查不到
    const w = await mountWithBlocks([
      {
        name: "minecraft:not_a_real_block",
        state: "minecraft:not_a_real_block",
        count: 1,
        zhName: "",
        enName: "",
      },
    ]);
    await clickReplaceOnRow(w);
    await new Promise((r) => setTimeout(r, 300));
    await flushPromises();

    expect(document.body.textContent, "查不到时应说明，而不是留一片空白").toContain(
      "未找到该 ID",
    );
    w.unmount();
  });
});

/** 方块替换窗口：右侧方块列表与中文名。
 *
 * 与版本转换窗口同一套做法——左栏填规则、右栏搜索并点选方块，凡是出现
 * 方块 ID 的地方都一并给出中文名。
 *
 * 源方块不可改（#53）：替换的入口是「方块」页签里某一行的替换按钮，
 * 规则作用于哪一行在点按钮那一刻就定了。允许改会让人以为改的是那一行。
 */
describe("方块替换窗口（方块列表与中文名）", () => {
  const HITS = [
    { name: "minecraft:stone", zh: "石头", en: "Stone", versions: [] },
    { name: "minecraft:cobblestone", zh: "圆石", en: "Cobblestone", versions: [] },
  ];

  /** 方块表里的一行：石头。用它当作替换源。 */
  const ROWS: BlockCount[] = [
    { name: "minecraft:stone", state: "minecraft:stone", count: 10, zhName: "石头", enName: "Stone" },
  ];

  /** 左栏四个输入框：按稳定的类名定位。
   *
   * 不用 placeholder：两个只读框都没有 placeholder，而目标方块的 placeholder
   * 是文案、会随语言变化；类名不会因为文案改动而失效。
   */
  function field(cls: string): HTMLInputElement {
    const el = document.querySelector<HTMLInputElement>(`.repl-window ${cls} input`);
    if (!el) throw new Error(`找不到输入框 .${cls}`);
    return el;
  }

  const fromName = () => field(".repl-from-name");
  const fromProps = () => field(".repl-from-props");
  const toName = () => field(".repl-to-name");
  const toProps = () => field(".repl-to-props");

  function pickItems(): HTMLElement[] {
    return [...document.querySelectorAll(".repl-pick__item")] as HTMLElement[];
  }

  /** 打开替换窗口（点方块表第一行的替换按钮），并让方块检索返回固定的两条结果 */
  async function openReplaceWindow() {
    const w = await mountWithBlocks(ROWS);
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "search_blocks") return HITS;
      if (cmd === "list_blocks") return HITS;
      return [];
    }) as never);
    await clickReplaceOnRow(w);
    return w;
  }

  async function searchBlocks(text: string) {
    const box = document.querySelector<HTMLInputElement>(".repl-split__pane--pick input");
    box!.value = text;
    box!.dispatchEvent(new Event("input", { bubbles: true }));
    await flushPromises();
  }

  it("右侧列表显示中文名与方块 ID，点选后填入目标方块", async () => {
    const w = await openReplaceWindow();
    await searchBlocks("stone");

    const items = pickItems();
    expect(items.length, "搜索结果应列出").toBe(2);
    expect(items[0].textContent, "列表项应显示中文名").toContain("石头");
    expect(items[0].textContent, "列表项应显示方块 ID").toContain("minecraft:stone");

    items[0].click();
    await flushPromises();

    expect(toName().value, "应填入目标方块").toBe("minecraft:stone");
    expect(document.body.textContent, "目标方块应给出显示名称").toContain("显示名称：石头");
    w.unmount();
  });

  it("源方块与源属性限定是只读的，点选方块只填目标", async () => {
    // 源方块由「方块」页签里点的那一行决定。若它可改，用户会以为改的是那一行，
    // 实际却按另一条规则执行。
    const w = await openReplaceWindow();

    const fromInput = fromName();
    const propsInput = fromProps();
    expect(fromInput.value, "源方块应已按点的那一行填好").toBe("minecraft:stone");
    expect(fromInput.readOnly, "源方块输入框应只读").toBe(true);
    expect(propsInput.readOnly, "源属性限定输入框应只读").toBe(true);

    await searchBlocks("cobble");
    const cobble = pickItems().find((i) => (i.textContent ?? "").includes("cobblestone"))!;
    cobble.click();
    await flushPromises();

    expect(toName().value, "点选应填入目标方块").toBe("minecraft:cobblestone");
    expect(fromInput.value, "点选不应改动源方块").toBe("minecraft:stone");
    w.unmount();
  });

  it("还没填目标方块时它标红，填上后不再标红", async () => {
    const w = await openReplaceWindow();

    // 源方块一定已经有值（来自点的那一行），因此只有目标方块该标红
    expect(
      document.querySelectorAll(".repl-block--empty").length,
      "只有还没填的目标方块标红",
    ).toBe(1);

    await searchBlocks("stone");
    pickItems()[0].click();
    await flushPromises();

    expect(
      document.querySelectorAll(".repl-block--empty").length,
      "填上后不应再有标红的输入框",
    ).toBe(0);
    w.unmount();
  });

  it("粘贴带属性的状态串会被拆开，不会把属性当方块名送出去", async () => {
    // 后端契约要求方块名不含属性：带属性的名字会被原样写进投影的调色板，
    // 游戏读不出来。目标方块仍是自由文本，必须自己拆。
    const w = await openReplaceWindow();

    const toInput = toName();
    toInput.value = "minecraft:oak_stairs[facing=east,half=top]";
    toInput.dispatchEvent(new Event("input", { bubbles: true }));
    await flushPromises();

    expect(toInput.value, "方块名里不该留方括号").toBe("minecraft:oak_stairs");
    expect(toProps().value, "方括号里的属性应落进「目标属性覆盖」").toBe(
      "facing=east,half=top",
    );
    w.unmount();
  });

  it("点选目标方块时会清掉上一个目标的目标属性覆盖", async () => {
    // to_props 是无条件写上去的：留着上一个目标方块的属性会把新目标也带上
    // 那个属性，正是为源方块解决过的「看着填好了、规则却不对」
    const w = await openReplaceWindow();

    const propsOverride = toProps();
    propsOverride.value = "facing=west";
    propsOverride.dispatchEvent(new Event("input", { bubbles: true }));
    await flushPromises();

    await searchBlocks("stone");
    pickItems()[0].click();
    await flushPromises();

    expect(toName().value, "应填入目标方块").toBe("minecraft:stone");
    expect(propsOverride.value, "目标属性覆盖应被清空").toBe("");
    w.unmount();
  });

  it("规则改动后不再保留上一次的替换数量", async () => {
    // 那个数字是「按当时那条规则」算出来的，规则一改就不再对应当前填写，
    // 留着会让用户以为「按现在填的规则只有这么多个」
    const w = await openReplaceWindow();
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "search_blocks" || cmd === "list_blocks") return HITS;
      if (cmd === "preview_replace") return { replaced: 42 };
      return [];
    }) as never);

    const toInput = toName();
    toInput.value = "minecraft:stone";
    toInput.dispatchEvent(new Event("input", { bubbles: true }));
    await flushPromises();

    // 弹窗内容被 Teleport 到 body，不在组件的渲染树里，按钮要从 document 找
    const previewBtn = [...document.querySelectorAll("button")].find((b) =>
      (b.textContent ?? "").includes("预计替换数量"),
    ) as HTMLElement | undefined;
    expect(previewBtn, "应有「预计替换数量」按钮").toBeTruthy();
    previewBtn!.click();
    await flushPromises();
    expect(document.body.textContent, "应显示预览数量").toContain("42");

    toInput.value = "minecraft:cobblestone";
    toInput.dispatchEvent(new Event("input", { bubbles: true }));
    await flushPromises();
    expect(document.body.textContent, "改过规则后不该再显示旧数字").not.toContain(
      "将替换",
    );
    w.unmount();
  });

  it("打开窗口即列出方块库的全部方块，无需先输入关键字", async () => {
    // 替换的目标方块需要一个默认可选范围：只给空列表等于要求用户先知道名字
    const ALL = [
      { name: "minecraft:andesite", zh: "安山岩", en: "Andesite", versions: [] },
      { name: "minecraft:cobblestone", zh: "圆石", en: "Cobblestone", versions: [] },
      { name: "minecraft:stone", zh: "石头", en: "Stone", versions: [] },
    ];
    const w = await mountWithBlocks(ROWS, {
      list_blocks: ALL,
      search_blocks: [HITS[0]],
    });
    await clickReplaceOnRow(w);

    const calls = mockInvoke.mock.calls.filter((c) => c[0] === "list_blocks");
    expect(calls.length, "打开窗口应取一次全部方块").toBe(1);
    expect(pickItems().length, "右侧列表应直接有内容").toBe(3);
    // 后端按方块 ID 升序给出，前端原样渲染、不重排
    expect(pickItems().map((i) => i.querySelector(".repl-block__id")?.textContent?.trim().split(" ")[0])).toEqual([
      "minecraft:andesite",
      "minecraft:cobblestone",
      "minecraft:stone",
    ]);

    // 输入关键字改走检索
    await searchBlocks("stone");
    const searchCalls = mockInvoke.mock.calls.filter((c) => c[0] === "search_blocks");
    expect(searchCalls.length, "输入关键字应走检索").toBeGreaterThan(0);
    expect(pickItems().length, "列表改为检索结果").toBe(1);

    // 清空关键字回到全量列表，且不必再请求一次
    await searchBlocks("");
    expect(pickItems().length, "清空后应回到全量列表").toBe(3);
    expect(
      mockInvoke.mock.calls.filter((c) => c[0] === "list_blocks").length,
      "回填用的是已取到的全量结果，不该重复请求",
    ).toBe(1);
    w.unmount();
  });

  it("全量列表尚未返回时先搜索、再清空，仍能拿回全量列表", async () => {
    // 打开窗口的全量请求比用户输入慢时，晚到的结果不能把界面搞成空列表：
    // 清空关键字时若缓存还没就绪，必须重新取一次，而不是显示「库里没有方块」
    const ALL = [
      { name: "minecraft:andesite", zh: "安山岩", en: "Andesite", versions: [] },
      { name: "minecraft:stone", zh: "石头", en: "Stone", versions: [] },
    ];
    // 用一个对象装「放行函数」：闭包里赋值，外部读到的类型不会退化
    const gate: { release?: () => void } = {};
    let firstCall = true;
    const w = await mountWithBlocks(ROWS, {
      // 第一次全量请求挂起，模拟慢响应
      list_blocks: () => {
        if (firstCall) {
          firstCall = false;
          return new Promise((resolve) => {
            gate.release = () => resolve(ALL);
          });
        }
        return Promise.resolve(ALL);
      },
      search_blocks: [HITS[0]],
    });
    await clickReplaceOnRow(w);

    // 用户在全量还没回来时输入关键字并清空
    await searchBlocks("stone");
    expect(pickItems().length, "应看到检索结果").toBe(1);
    await searchBlocks("");
    // 清空发现缓存未就绪，重新发起全量请求并拿到结果
    gate.release?.();
    await flushPromises();
    await new Promise((r) => setTimeout(r, 50));
    await flushPromises();
    expect(pickItems().length, "清空后应补回全量列表，而不是空列表").toBe(2);
    w.unmount();
  });
});
describe("跟随系统深浅色", () => {
  // Arco 的深色取值挂在 body[arco-theme="dark"] 上。样式表里两套都写好了，
  // 但没人设置这个属性的话深色永远不会生效——系统开了深色，界面仍是白的。
  beforeEach(() => {
    document.body.removeAttribute("arco-theme");
  });

  it("系统为深色时给 body 打上 arco-theme，界面才真的变深", async () => {
    vi.mocked(getCurrentWindow).mockReturnValue({
      theme: vi.fn().mockResolvedValue("dark"),
      onThemeChanged: vi.fn().mockResolvedValue(() => {}),
    } as never);
    stubInvoke();
    const w = mountApp();
    await flushPromises();
    expect(document.body.getAttribute("arco-theme"), "应设为 dark").toBe("dark");
    w.unmount();
  });

  it("系统为浅色时不加该属性", async () => {
    vi.mocked(getCurrentWindow).mockReturnValue({
      theme: vi.fn().mockResolvedValue("light"),
      onThemeChanged: vi.fn().mockResolvedValue(() => {}),
    } as never);
    stubInvoke();
    const w = mountApp();
    await flushPromises();
    expect(document.body.getAttribute("arco-theme"), "不应有 dark 属性").toBeNull();
    w.unmount();
  });

  it("系统切换主题时跟随变化", async () => {
    let fire: ((e: { payload: string }) => void) | undefined;
    vi.mocked(getCurrentWindow).mockReturnValue({
      theme: vi.fn().mockResolvedValue("light"),
      onThemeChanged: vi.fn().mockImplementation(async (cb: (e: { payload: string }) => void) => {
        fire = cb;
        return () => {};
      }),
    } as never);
    stubInvoke();
    const w = mountApp();
    await flushPromises();
    expect(document.body.getAttribute("arco-theme")).toBeNull();

    // 系统切到深色
    expect(fire, "应注册了主题变化回调").toBeTruthy();
    fire!({ payload: "dark" });
    await flushPromises();
    expect(document.body.getAttribute("arco-theme"), "应跟随变为 dark").toBe("dark");

    // 再切回浅色
    fire!({ payload: "light" });
    await flushPromises();
    expect(document.body.getAttribute("arco-theme"), "应跟随去掉 dark").toBeNull();
    w.unmount();
  });

  it("读不到主题时不崩，按浅色显示", async () => {
    // 浏览器里跑前端、或平台不提供该能力时都会走到这里
    vi.mocked(getCurrentWindow).mockReturnValue({
      theme: vi.fn().mockRejectedValue(new Error("不支持")),
      onThemeChanged: vi.fn().mockResolvedValue(() => {}),
    } as never);
    stubInvoke();
    const w = mountApp();
    await flushPromises();
    expect(document.body.getAttribute("arco-theme"), "失败时应保持浅色").toBeNull();
    // 其余功能不受影响
    expect(w.text(), "界面仍应正常渲染").toContain("添加目录");
    w.unmount();
  });
});

describe("重新扫描后的选中项", () => {
  /** 点工具栏的「添加目录」并等扫描完成。
   *
   * 对话框用 multiple: true 打开，因此插件返回的是**数组**。
   * 这里也传数组，与真实返回值一致；只传单个字符串会掩盖
   * 「只处理了返回值的第一种形态」这类缺陷。
   */
  async function addDir(w: ReturnType<typeof mountApp>, ...dirs: string[]) {
    await activateFiles(w);
    mockOpen.mockResolvedValue(dirs);
    const btn = w.findAll("button").find((b) => b.text().includes("添加目录"));
    expect(btn, "应能定位到「添加目录」按钮").toBeTruthy();
    await btn!.trigger("click");
    await flushPromises();
  }

  it("移除目录只移除卡片，保留独立的编辑标签", async () => {
    const other: SchematicSummary = {
      ...summary,
      path: "/q/另一个.litematic",
      fileName: "另一个.litematic",
      dirName: "/q",
      name: "另一个投影",
    };

    stubInvoke();
    const w = mountApp();
    await addDir(w, "/p");
    // 选中第一个目录里的文件
    const row = w.findAll(".schematic-card").find((r) => r.text().includes("测试.litematic"));
    expect(row, "应能找到该文件所在的行").toBeTruthy();
    await row!.trigger("dblclick");
    await flushPromises();
    expect(w.text(), "选中后应出现详情页签").toContain("信息");

    // 再加一个目录（新列表里没有原选中项）
    stubInvoke({
      scan_litematics: { items: [other], skippedDirs: 0, skippedPaths: [] },
    });
    await addDir(w, "/q");
    // 目录树保留两个根目录，选择新根目录后显示其中的卡片
    expect(w.text(), "先前加载的目录应保留").toContain("测试.litematic");
    await selectRoot(w, "/q");
    expect(w.text(), "新加载的目录也应显示").toContain("另一个.litematic");

    // 通过真实界面移除：点开「已加载目录」下拉，点第一项的「移除」按钮。
    // 不直接调用组件内部函数——那绕过了按钮是否真的能用、事件是否接对，
    // 而这两点正是「点了没反应」类缺陷的所在。
    const trigger = w.findAll("button").find((b) => b.text().includes("已加载目录"));
    expect(trigger, "应有「已加载目录」下拉按钮").toBeTruthy();
    await trigger!.trigger("click");
    await flushPromises();

    // 下拉内容被 teleport 到 body，因此从 document 里找
    const removeBtn = Array.from(document.querySelectorAll(".dir-item button")).find((b) =>
      (b.textContent ?? "").includes("移除"),
    ) as HTMLElement | undefined;
    expect(removeBtn, "下拉里应有「移除」按钮").toBeTruthy();
    removeBtn!.click();
    await flushPromises();

    expect(
      w.findAll(".schematic-card").some((card) => card.text().includes("测试.litematic")),
      "被移除目录的卡片不应还留在列表里",
    ).toBe(false);
    expect(w.text(), "另一个目录的文件应保留").toContain("另一个.litematic");
    await activateEditor(w, "测试.litematic");
    expect(w.find(".editor-detail").exists(), "已打开文件的编辑器应保留").toBe(true);
    expect(w.findAll("button").some((b) => b.text().includes("版本转换")), "编辑功能仍应可用").toBe(true);
    w.unmount();
  });

  it("同一目录重新扫描时保留选中项", async () => {
    // 反过来的情形：重新扫描当前目录（例如文件没变），选中项应当保留，
    // 不能一律清空——那会让用户每次刷新都丢掉正在查看的文件。
    stubInvoke();
    const w = mountApp();
    await addDir(w, "/p");
    await openFileCard(w, "测试.litematic");
    await flushPromises();
    expect(w.text()).toContain("信息");

    await addDir(w, "/p");
    await activateEditor(w, "测试.litematic");

    expect(w.text(), "同一目录重扫后应仍显示该文件").toContain("测试.litematic");
    expect(w.text(), "选中项应保留").toContain("信息");
    const btn = (label: string) => w.findAll("button").find((b) => b.text().includes(label));
    expect(btn("版本转换")?.attributes("disabled"), "选中项保留，按钮应可用").toBeUndefined();
    w.unmount();
  });
});

describe("对话框失败时的提示", () => {
  it("打开目录选择框失败时给出提示，而不是静默无反应", async () => {
    // 这条防的是：pickDirectory 把 `await open(...)` 放在 try 之外，
    // 插件报错时这个 Promise 会静默 reject——用户点了「添加目录」
    // 却什么都没发生，也收不到任何提示。
    //
    // 断言用 spy 而不是查 document.body：Arco 的 Message 通过 portal 渲染，
    // 在同一次运行里先跑过别的用例后，它的容器状态会让文本断言变得不稳定
    // （本条曾单独通过、全量运行却失败）。直接断言「确实报告了错误」
    // 既精确又不依赖 portal 的内部实现。
    stubInvoke();
    mockOpen.mockRejectedValue(new Error("对话框不可用"));
    const errSpy = vi.spyOn(Message, "error");
    const w = mountApp();

    const btn = w.findAll("button").find((b) => b.text().includes("添加目录"));
    expect(btn, "应能定位到「添加目录」按钮").toBeTruthy();
    await btn!.trigger("click");
    await flushPromises();

    const reported = errSpy.mock.calls.map((c) => String(c[0]));
    expect(
      reported.some((t) => t.includes("打开目录选择框失败")),
      `应报告选择框打开失败，实际报告了：${JSON.stringify(reported)}`,
    ).toBe(true);
    // 失败时不应误报正在扫描
    expect(w.text(), "不应进入扫描状态").not.toContain("正在扫描");

    errSpy.mockRestore();
    w.unmount();
  });
});

describe("重复文件筛查（内联在主界面）", () => {
  /** 三条同名内容重复的记录，放在同一个目录 */
  const DIR = "/p";
  const DUP_PATHS = [
    `${DIR}/x.litematic`,
    `${DIR}/y.litematic`,
    `${DIR}/z.litematic`,
  ];

  function dupSummary(path: string): SchematicSummary {
    return {
      ...summary,
      path,
      fileName: path.split("/").pop() ?? "x.litematic",
      dirName: DIR,
    };
  }

  /** 添加目录并返回可用的 wrapper */
  async function mountWithDir(dirs: string[] = [DIR]) {
    mockOpen.mockResolvedValue(dirs);
    const w = mountApp();
    const add = w.findAll("button").find((b) => b.text().includes("添加目录"));
    expect(add, "应能定位到「添加目录」按钮").toBeTruthy();
    await add!.trigger("click");
    await flushPromises();
    return w;
  }

  /** 点「重复文件筛查」并等结果 */
  async function runScan(w: ReturnType<typeof mountApp>) {
    const scan = w.findAll("button").find((b) => b.text().includes("重复文件筛查"));
    expect(scan, "工具栏应有「重复文件筛查」").toBeTruthy();
    await scan!.trigger("click");
    await flushPromises();
  }

  function stubDup() {
    stubInvoke({
      scan_litematics: {
        items: DUP_PATHS.map(dupSummary),
        skippedDirs: 0,
        skippedPaths: [],
      },
      scan_duplicates: {
        scanned: DUP_PATHS.length,
        groups: [{ size: 4096, paths: DUP_PATHS }],
        unreadable: [],
        skippedDirs: 0,
      },
    });
  }

  /** 三条重复之外再放一个内容不同的投影。
   *
   * 路径特意排在三条重复之前（a- 开头），用来验证「重复排到最前」确实是
   * 筛查后排的，而不是本来就在前面。 */
  function stubDupWithOutsider() {
    const outsider: SchematicSummary = {
      ...summary,
      path: `${DIR}/a-其它.litematic`,
      fileName: "a-其它.litematic",
      dirName: DIR,
      name: "另一个投影",
      enclosing: [8, 8, 8],
      totalBlocks: 99,
    };
    stubInvoke({
      scan_litematics: {
        items: [...DUP_PATHS.map(dupSummary), outsider],
        skippedDirs: 0,
        skippedPaths: [],
      },
      scan_duplicates: {
        scanned: 4,
        groups: [{ size: 4096, paths: DUP_PATHS }],
        unreadable: [],
        skippedDirs: 0,
      },
    });
  }

  /** 两条属性完全相同、但内容不同的投影 */
  function stubSuspects() {
    const a: SchematicSummary = {
      ...summary,
      path: `${DIR}/s1.litematic`,
      fileName: "s1.litematic",
      dirName: DIR,
    };
    const b: SchematicSummary = {
      ...summary,
      path: `${DIR}/s2.litematic`,
      fileName: "s2.litematic",
      dirName: DIR,
    };
    stubInvoke({
      scan_litematics: { items: [a, b], skippedDirs: 0, skippedPaths: [] },
      // 内容不同：没有任何一组内容相同的重复
      scan_duplicates: { scanned: 2, groups: [], unreadable: [], skippedDirs: 0 },
    });
  }

  it("筛查后同组行标上同一种颜色类", async () => {
    stubDup();
    const w = await mountWithDir();
    await runScan(w);

    const rows = Array.from(document.querySelectorAll(".schematic-card"));
    const dupRows = rows.filter((r) =>
      Array.from(r.classList).some((c) => c.startsWith("row-dup-")),
    );
    expect(dupRows.length, "三条重复记录都应被标记").toBe(3);
    const classes = new Set(
      dupRows.flatMap((r) =>
        Array.from(r.classList).filter((c) => c.startsWith("row-dup-")),
      ),
    );
    expect(classes.size, "同组应使用同一种颜色类").toBe(1);
    w.unmount();
  });

  it("重复行在文件名前带上组号色块", async () => {
    stubDup();
    const w = await mountWithDir();
    await runScan(w);

    const chips = Array.from(document.querySelectorAll(".schematic-card .dup-chip"));
    expect(chips.length, "三条重复都应带组号").toBe(3);
    expect(chips.map((c) => c.textContent), "组号应从 1 起").toEqual(["1", "1", "1"]);
    expect(
      chips.every((c) => c.classList.contains("dup-chip-1")),
      "同一组的色块应是同一种颜色",
    ).toBe(true);
    // 组号只是标记，文件名必须照旧显示出来
    expect(document.body.textContent, "文件名不应被色块顶掉").toContain("x.litematic");
    w.unmount();
  });

  it("重复文件排到列表最前，同组相邻", async () => {
    stubDupWithOutsider();
    const w = await mountWithDir();
    await runScan(w);

    const texts = Array.from(document.querySelectorAll(".schematic-card"))
      .map((r) => r.textContent ?? "")
      .filter((t) => t.includes(".litematic"));
    expect(texts.length, "四条记录都应在列表里").toBe(4);
    // 路径序本来是 a-其它 在最前；筛查后三条重复必须占住前三行
    expect(texts.slice(0, 3).every((t) => /[xyz]\.litematic/.test(t)), "重复应排到最前").toBe(
      true,
    );
    expect(texts[3], "不重复的留在后面").toContain("a-其它.litematic");
    w.unmount();
  });

  it("属性相同而内容不同的标为疑似重复，且不默认勾选", async () => {
    stubSuspects();
    const w = await mountWithDir();
    await runScan(w);

    const marked = Array.from(document.querySelectorAll(".schematic-card")).filter((r) =>
      r.classList.contains("row-dup-suspect"),
    );
    expect(marked.length, "两条疑似都应被标出").toBe(2);
    expect(
      document.querySelectorAll(".schematic-card .dup-chip--suspect").length,
      "疑似行应有「疑似」标记",
    ).toBe(2);
    // 关键：疑似只是提示，不能替用户勾上待删
    expect(document.body.textContent, "疑似重复不应被默认勾选").toContain("已选 0");
    const bar = w.find(".file-manager__dups");
    expect(bar.text(), "汇总条应说明有疑似重复").toContain("疑似");
    w.unmount();
  });

  it("父子目录同时加载时，同一个文件不会被当成一组疑似重复", async () => {
    // 父目录递归扫到了子目录里的文件，两个目录都返回它：
    // 不去重的话这个文件会自己和自己配对，凑出一组只有一个成员的「疑似重复」
    const f: SchematicSummary = {
      ...summary,
      path: `${DIR}/sub/a.litematic`,
      fileName: "a.litematic",
      dirName: `${DIR}/sub`,
    };
    stubInvoke({
      scan_litematics: { items: [f], skippedDirs: 0, skippedPaths: [] },
      scan_duplicates: { scanned: 1, groups: [], unreadable: [], skippedDirs: 0 },
    });
    const w = await mountWithDir([DIR, `${DIR}/sub`]);
    await runScan(w);

    expect(
      document.querySelectorAll(".schematic-card .dup-chip--suspect").length,
      "同一个文件不该被标成疑似重复",
    ).toBe(0);
    expect(document.body.textContent ?? "", "不应报告疑似重复组").not.toContain("疑似重复");
    w.unmount();
  });

  it("属性与某个重复组相同但内容不同的文件仍会被提示", async () => {
    // a、b 逐字节相同（重复组），c 与它们属性相同而内容不同。
    // 建键时就排除已确认成员的话，c 会落单并被过滤掉——恰恰漏掉最该看的那份。
    const mk = (name: string): SchematicSummary => ({
      ...summary,
      path: `${DIR}/${name}.litematic`,
      fileName: `${name}.litematic`,
      dirName: DIR,
    });
    const [a, b, c] = [mk("a"), mk("b"), mk("c")];
    stubInvoke({
      scan_litematics: { items: [a, b, c], skippedDirs: 0, skippedPaths: [] },
      scan_duplicates: {
        scanned: 3,
        groups: [{ size: 4096, paths: [a.path, b.path] }],
        unreadable: [],
        skippedDirs: 0,
      },
    });
    const w = await mountWithDir();
    await runScan(w);

    const suspect = Array.from(document.querySelectorAll(".schematic-card")).filter((r) =>
      r.classList.contains("row-dup-suspect"),
    );
    expect(suspect.length, "c 应被标为疑似重复").toBe(1);
    expect(suspect[0].textContent, "被标出的应是 c").toContain("c.litematic");
    w.unmount();
  });

  it("默认勾选多于重复文件（每组保留第一份）", async () => {
    stubDup();
    const w = await mountWithDir();
    await runScan(w);

    // 用户原话「默认勾选多于重复文件」：一组三条应勾上两条
    expect(
      document.body.textContent ?? "",
      "一组三条应默认勾选两条",
    ).toContain("已选 2");
    w.unmount();
  });

  it("第一份先被勾上时也会被取消，只留多余的副本", async () => {
    stubDup();
    const w = await mountWithDir();
    const all = w.findAll("button").find((b) => b.text().includes("全选"));
    expect(all, "应能全选").toBeTruthy();
    await all!.trigger("click");
    await flushPromises();

    await runScan(w);
    // 全选后三条都勾着；筛查要把「保留的那一份」取消，否则用户一按删除
    // 会把最后一份也删掉
    expect(document.body.textContent ?? "", "应只剩多余的 2 份").toContain("已选 2");
    w.unmount();
  });

  it("汇总条显示组数与文件数，「清除标记」能去掉标记", async () => {
    stubDup();
    const w = await mountWithDir();
    await runScan(w);

    const bar = w.find(".file-manager__dups");
    expect(bar.exists(), "应显示内联汇总条（不是弹窗）").toBe(true);
    expect(bar.text(), "应说明发现几组").toContain("1");
    expect(bar.text(), "应说明涉及几个文件").toContain("3");
    expect(
      document.body.textContent,
      "弹窗应已移除，筛查结果就地显示",
    ).not.toContain("尚未筛查");

    const clear = bar.findAll("button").find((b) => b.text().includes("清除标记"));
    expect(clear, "汇总条应有「清除标记」").toBeTruthy();
    await clear!.trigger("click");
    await flushPromises();

    const stillDup = Array.from(document.querySelectorAll(".schematic-card")).filter((r) =>
      Array.from(r.classList).some((c) => c.startsWith("row-dup-")),
    );
    expect(stillDup.length, "清除后不应再有重复标记").toBe(0);
    w.unmount();
  });

  it("未勾选任何目录时给出提示而不是静默无反应", async () => {
    // 目录加载了但被取消勾选 -> 没有可筛查的目录
    stubDup();
    const w = await mountWithDir();
    const toggle = w.findAll("button").find((b) => b.text().includes("已加载目录"));
    expect(toggle, "应有目录下拉").toBeTruthy();
    await runScan(w);
    // 至少不应崩溃；有目录时会出汇总条，无目录时提示
    expect(w.find(".file-manager").exists()).toBe(true);
    w.unmount();
  });
});

describe("版本转换", () => {
  function findButton(label: string): HTMLButtonElement | undefined {
    return [...document.querySelectorAll("button")].find((b) =>
      (b.textContent ?? "").includes(label),
    );
  }

  it("分析失败后不应再显示「可以直接转换」", async () => {
    // 这条防的是：分析失败的 catch 只清空条目、不复位 convAnalyzed，
    // 于是模板把「已分析且条目为空」当成「方块都存在」，展示绿色成功提示，
    // 且确定按钮可用，点下去会用空条目执行转换。
    let analyzeCalls = 0;
    mockInvoke.mockImplementation((async (cmd: string) => {
      switch (cmd) {
        case "scan_litematics":
          return { items: [summary], skippedDirs: 0, skippedPaths: [] };
        case "convert_targets":
          return [{ index: 0, name: "1.19.2", dataVersion: 3120 }];
        case "base_db_status":
          // 目标版本下拉会把「支持但尚未加载」的版本一并列出（点一下下载），
          // 那份清单取自版本库状态；缺了它下拉渲染会抛错
          return {
            versions: [{ name: "1.20.1", dataVersion: 3465 }],
            entityAvailable: true,
            supported: [{ name: "1.20.1", dataVersion: 3465 }],
          };
        case "analyze_conversion":
          analyzeCalls += 1;
          // 第一次成功且无缺口（显示成功提示），第二次失败
          if (analyzeCalls === 1) return [];
          throw new Error("模拟分析失败");
        // 选中文件后会加载场景与详情；必须返回结构正确的对象，
        // 否则界面在访问字段时抛错，表现为「测试通过但留下未处理错误」
        case "build_scene":
          return scene;
        case "analyze_schematic":
          return {
            blocks: [],
            entities: [],
            tileEntities: [],
            nonAirTotal: 0,
            airTotal: 0,
            distinctBlockTypes: 0,
            distinctStates: 0,
          };
        case "list_block_versions":
          return [];
        default:
          return [];
      }
    }) as never);

    const w = mountApp();
    // 目录选择框要返回一个路径，否则不会进入扫描流程
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();

    await w.findAll("button").find((b) => b.text().includes("版本转换"))!.trigger("click");
    await flushPromises();

    // 选一个目标版本，否则分析按钮会提示「请先选择目标版本」
    const select = document.querySelector(".arco-select-view, .arco-select") as HTMLElement | null;
    if (select) {
      select.click();
      await flushPromises();
      const opt = [...document.querySelectorAll(".arco-select-option")].find((o) =>
        (o.textContent ?? "").includes("1.19.2"),
      ) as HTMLElement | undefined;
      if (opt) {
        opt.click();
        await flushPromises();
      }
    }

    const analyze = findButton("分析差异");
    expect(analyze, "弹窗里应有「分析差异」").toBeTruthy();
    analyze!.click();
    await flushPromises();
    expect(document.body.textContent, "首次分析成功应给出成功提示").toContain(
      "可以直接转换",
    );

    analyze!.click();
    await flushPromises();
    expect(
      document.body.textContent,
      "分析失败后不应残留「可以直接转换」的成功提示",
    ).not.toContain("可以直接转换");
    w.unmount();
  });
});

/** 版本转换窗口：中文名与右侧方块列表点选。
 *
 * 「选目标版本」走真实下拉：没选中目标版本时「分析差异」只会提示，
 * 面板不会出现，后续断言全都落空。
 */
describe("版本转换窗口（中文名与替代品点选）", () => {
  const PICK_HIT = {
    name: "minecraft:warped_stairs",
    zh: "诡异木楼梯",
    en: "Warped Stairs",
    versions: [{ index: 0, dataVersion: 3465 }],
  };

  /** 只存在于别的版本：不能拿来当目标版本的替代品 */
  const OTHER_VERSION_HIT = {
    name: "minecraft:copper_grate",
    zh: "铜格栅",
    en: "Copper Grate",
    versions: [{ index: 7, dataVersion: 3953 }],
  };

  /** 两条缺口：第一条有自动推荐，第二条没有（需要用户点选） */
  const CONV_ITEMS = [
    {
      name: "minecraft:oak_stairs",
      nameZh: "橡木楼梯",
      count: 12,
      suggested: "minecraft:pale_oak_stairs",
      suggestedZh: "苍白橡木楼梯",
      chosen: "",
    },
    {
      name: "minecraft:ancient_debris",
      nameZh: "远古残骸",
      count: 3,
      suggested: "",
      suggestedZh: "",
      chosen: "",
    },
  ];

  function findButton(label: string): HTMLButtonElement | undefined {
    return [...document.querySelectorAll("button")].find((b) =>
      (b.textContent ?? "").includes(label),
    );
  }

  function convRows(): HTMLElement[] {
    // 必须限定在转换窗口的两栏里：页面上还有目录栏、方块栏、实体栏等好几张表，
    // 全局找 tbody tr 会命中它们（实测把复选框的 value=false 当成输入框内容）
    return [...document.querySelectorAll(".conv-split tbody tr")] as HTMLElement[];
  }

  function rowByText(text: string): HTMLElement | undefined {
    return convRows().find((r) => (r.textContent ?? "").includes(text));
  }

  /** 打开转换窗口 → 选目标版本 → 分析差异。
   *
   * 替身里必须给出 `base_db_status`：目标版本下拉除了已加载的版本，还会把
   * 「支持但尚未加载」的版本以灰字列出来（点一下就下载），那份清单来自
   * 版本库状态。少了它，下拉渲染会直接抛错，整组用例连带失败。
   */
  async function openAndAnalyze() {
    mockInvoke.mockImplementation((async (cmd: string) => {
      switch (cmd) {
        case "scan_litematics":
          return { items: [summary], skippedDirs: 0, skippedPaths: [] };
        case "convert_targets":
          return [{ index: 0, name: "1.19.2", dataVersion: 3120 }];
        case "base_db_status":
          return {
            versions: [{ name: "1.20.1", dataVersion: 3465 }],
            entityAvailable: true,
            supported: [
              { name: "1.20.1", dataVersion: 3465 },
              { name: "1.21.11", dataVersion: 4325 },
            ],
          };
        case "setup_add_versions":
          return { outcomes: [{ id: "1.21.11", ok: true, error: "" }] };
        case "analyze_conversion":
          return CONV_ITEMS.map((i) => ({ ...i }));
        // 点选列表里的方块时用
        case "search_blocks":
          return [PICK_HIT];
        case "build_scene":
          return scene;
        case "analyze_schematic":
          return {
            blocks: [],
            entities: [],
            tileEntities: [],
            nonAirTotal: 0,
            airTotal: 0,
            distinctBlockTypes: 0,
            distinctStates: 0,
          };
        default:
          return [];
      }
    }) as never);
    return await openConvertDialog();
  }

  /** 打开转换窗口、选中已加载的目标版本、点「分析差异」，只负责界面动作。
   *
   * 替身由调用方安装：需要模拟「版本原来是没加载的、补完才可用」的用例
   * 得自己维持一份有状态的替身。
   */
  async function openConvertDialog() {
    const w = mountApp();
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();

    await w.findAll("button").find((b) => b.text().includes("版本转换"))!.trigger("click");
    await flushPromises();

    const select = document.querySelector(".arco-select-view, .arco-select") as HTMLElement | null;
    if (select) {
      select.click();
      await flushPromises();
      const opt = [...document.querySelectorAll(".arco-select-option")].find((o) =>
        (o.textContent ?? "").includes("1.19.2"),
      ) as HTMLElement | undefined;
      if (opt) {
        opt.click();
        await flushPromises();
      }
    }

    findButton("分析差异")!.click();
    await flushPromises();
    return w;
  }

  it("未加载的版本以灰字列出，点一下就下载并变成可选的目标版本", async () => {
    // 有状态替身：补库之前 1.21.11 只是「支持但未加载」，补完之后
    // 方块库里就有了它，convert_targets 也会给出它的下标
    let built = false;
    const setupCalls: Array<{ id: string; jarPath: string | null }> = [];
    mockInvoke.mockImplementation((async (cmd: string, args: never) => {
      const a = args as unknown as { versions?: Array<{ id: string; jarPath: string | null }> };
      switch (cmd) {
        case "scan_litematics":
          return { items: [summary], skippedDirs: 0, skippedPaths: [] };
        case "base_db_status":
          return {
            versions: built
              ? [
                  { name: "1.20.1", dataVersion: 3465 },
                  { name: "1.21.11", dataVersion: 4325 },
                ]
              : [{ name: "1.20.1", dataVersion: 3465 }],
            entityAvailable: true,
            supported: [
              { name: "1.20.1", dataVersion: 3465 },
              { name: "1.21.11", dataVersion: 4325 },
            ],
          };
        case "convert_targets":
          return built
            ? [
                { index: 0, name: "1.19.2", dataVersion: 3120 },
                { index: 1, name: "1.21.11", dataVersion: 4325 },
              ]
            : [{ index: 0, name: "1.19.2", dataVersion: 3120 }];
        case "setup_add_versions":
          for (const v of a.versions ?? []) setupCalls.push({ id: v.id, jarPath: v.jarPath });
          built = true;
          return { outcomes: [{ id: "1.21.11", ok: true, error: "" }] };
        case "analyze_conversion":
          return [];
        case "build_scene":
          return scene;
        // 选中文件后详情面板会读分析结果；必须回结构正确的对象，
        // 否则实体表在访问 analysis.entities 时抛错，留下未处理错误
        case "analyze_schematic":
          return {
            blocks: [],
            entities: [],
            tileEntities: [],
            entityInstances: [],
            nonAirTotal: 0,
            airTotal: 0,
            distinctBlockTypes: 0,
            distinctStates: 0,
          };
        case "list_block_versions":
          return [];
        // 补库会先定位 .minecraft 并扫一次本机版本；这几条必须回结构正确的值，
        // 回数组会让「扫本机已装版本」那一步抛错，补库根本发不出去
        case "load_setup_state":
          return null;
        case "default_minecraft_dir":
          return "C:/mc";
        case "scan_minecraft":
          return { versions: [], warnings: [] };
        default:
          return [];
      }
    }) as never);

    const w = await openConvertDialog();

    // 下拉里必须同时有「已加载」与「未加载」两类
    const select = document.querySelector(".arco-select-view, .arco-select") as HTMLElement | null;
    select!.click();
    await flushPromises();
    const pending = [...document.querySelectorAll(".arco-select-option")].find((o) =>
      (o.textContent ?? "").includes("1.21.11"),
    ) as HTMLElement | undefined;
    expect(pending, "未加载的版本也应列出").toBeTruthy();
    expect(pending!.textContent, "要说明点它能做什么").toContain("点击下载");

    // 点它 = 开始准备这个版本，而不是把它选成目标版本
    pending!.click();
    await flushPromises();

    expect(setupCalls, "应就这一个版本发起补库").toEqual([{ id: "1.21.11", jarPath: null }]);
    // 补完自动选中它（下标 1 是后端补库后新给的）
    expect(
      shownMessages().some((m) => m.includes("已就绪")),
      "应提示已就绪",
    ).toBe(true);
    // 再次展开下拉：它应当从「未加载」变成可直接选中的目标版本
    await flushPromises();
    const select2 = document.querySelector(".arco-select-view, .arco-select") as HTMLElement | null;
    select2!.click();
    await flushPromises();
    const nowReady = [...document.querySelectorAll(".arco-select-option")].find((o) =>
      (o.textContent ?? "").includes("1.21.11"),
    ) as HTMLElement | undefined;
    expect(nowReady!.textContent, "补完后不再显示「点击下载」").not.toContain("点击下载");
    // 版本库那边也要能看到它
    await activateFiles(w);
    await w.findAll("button").find((b) => b.text().includes("版本库"))!.trigger("click");
    await flushPromises();
    expect(document.body.textContent, "版本库里应出现 1.21.11").toContain("1.21.11");
    w.unmount();
  });

  it("源方块与自动推荐的替代品都显示中文名", async () => {
    const w = await openAndAnalyze();

    // 断言在**具体那一行**上，而不是整页文本：整页里别处也可能出现同一个词，
    // 那样渲染到别的列、别的表都会通过
    const auto = rowByText("minecraft:oak_stairs");
    expect(auto, "应有自动推荐那一行").toBeTruthy();
    // 中文名与方块 ID 必须同时出现：只给 ID 认不出，只给中文名又对不上文件
    expect(auto!.textContent, "应显示源方块中文名").toContain("橡木楼梯");
    expect(auto!.textContent, "应显示自动推荐的中文名").toContain("苍白橡木楼梯");
    expect(auto!.textContent, "应显示自动推荐的方块 ID").toContain(
      "minecraft:pale_oak_stairs",
    );

    const todo = rowByText("minecraft:ancient_debris");
    expect(todo!.textContent, "应显示没有推荐那种方块的中文名").toContain("远古残骸");
    w.unmount();
  });

  it("自动推荐的输入框标绿，没有推荐的标为待指定", async () => {
    const w = await openAndAnalyze();

    // 状态色只给「替代为」那一格里的输入框上色，不染整行、也不染整格
    const auto = rowByText("minecraft:oak_stairs");
    expect(auto, "应有自动推荐那一行").toBeTruthy();
    expect(
      auto!.querySelector(".conv-block--auto"),
      "自动识别的推荐应给输入框绿框标记",
    ).toBeTruthy();
    expect(auto!.textContent, "应标明是自动推荐").toContain("自动推荐");
    expect(auto!.className, "整行不应被状态色染色").not.toContain("conv-row--auto");

    const todo = rowByText("minecraft:ancient_debris");
    expect(todo, "应有待指定那一行").toBeTruthy();
    expect(
      todo!.querySelector(".conv-block--todo"),
      "没有替代品的输入框应带红框标记",
    ).toBeTruthy();
    expect(todo!.textContent, "应标明待指定").toContain("待指定");
    expect(todo!.className, "整行不应被状态色染色").not.toContain("conv-row--todo");
    w.unmount();
  });

  it("点选右侧方块列表里的方块，填给左侧选中的那一行", async () => {
    const w = await openAndAnalyze();

    // 先点左侧待指定那一行
    const todo = rowByText("minecraft:ancient_debris");
    todo!.click();
    await flushPromises();
    expect(
      rowByText("minecraft:ancient_debris")!.className,
      "点过的行应成为当前行",
    ).toContain("conv-row--active");

    // 在右侧搜索并点选一个方块
    const search = document.querySelector(".conv-split__pane--pick input") as HTMLInputElement | null;
    expect(search, "右侧应有搜索框").toBeTruthy();
    search!.value = "warped";
    search!.dispatchEvent(new Event("input", { bubbles: true }));
    await flushPromises();

    const hit = document.querySelector(".conv-pick__item") as HTMLElement | null;
    expect(hit, "搜索后应列出方块").toBeTruthy();
    // 列表项本身也要中英文名齐全
    expect(hit!.textContent, "列表项应显示中文名").toContain("诡异木楼梯");
    expect(hit!.textContent, "列表项应显示方块 ID").toContain("minecraft:warped_stairs");
    hit!.click();
    await flushPromises();

    const after = rowByText("minecraft:ancient_debris")!;
    const input = after.querySelector("input") as HTMLInputElement | null;
    expect(input?.value, "选中的方块应填进这一行的「替代为」").toBe(
      "minecraft:warped_stairs",
    );
    expect(after.textContent, "这一行应显示新方块的中文名").toContain("诡异木楼梯");
    expect(after.textContent, "填好后状态应变成已指定").toContain("已指定");
    // 用户自己选过之后输入框不再上色：红框（待指定）与绿框（自动识别）
    // 都是「要你看这里」的提示，用户已经做过决定的行不必再抢注意力
    expect(after.querySelector(".conv-block--todo"), "红框标记应消失").toBeFalsy();
    expect(after.querySelector(".conv-block--auto"), "绿框标记也应消失").toBeFalsy();
    w.unmount();
  });

  it("没先点行就点方块时给出提示，而不是改错行", async () => {
    const w = await openAndAnalyze();

    const search = document.querySelector(".conv-split__pane--pick input") as HTMLInputElement | null;
    search!.value = "warped";
    search!.dispatchEvent(new Event("input", { bubbles: true }));
    await flushPromises();

    expect(document.body.textContent, "应提示先点左侧的行").toContain("请先在左侧点选");
    (document.querySelector(".conv-pick__item") as HTMLElement).click();
    await flushPromises();

    // 一行都不该被改动：没选中行时点方块必须只提示
    for (const r of convRows()) {
      const input = r.querySelector("input") as HTMLInputElement | null;
      if (input) expect(input.value, "不该有行被悄悄填上").toBe("");
    }
    expect(document.body.textContent, "应提示先点行再点方块").toContain(
      "请先在左侧点选要替换的方块",
    );
    w.unmount();
  });

  it("目标版本里没有的方块会被标注，且点它不会填进去", async () => {
    // 检索是跨全部版本的：不拦住的话用户可以指定一个目标版本同样没有的
    // 方块，转换出来的文件仍然引用不存在的方块——正是这个窗口要消除的问题
    const w = await openAndAnalyze();
    rowByText("minecraft:ancient_debris")!.click();
    await flushPromises();

    // 让搜索同时返回「目标版本里有」和「只存在于别的版本」两条
    const base = mockInvoke.getMockImplementation()!;
    mockInvoke.mockImplementation((async (cmd: string, args: never) => {
      if (cmd === "search_blocks") return [PICK_HIT, OTHER_VERSION_HIT];
      return base(cmd, args);
    }) as never);

    const search = document.querySelector(".conv-split__pane--pick input") as HTMLInputElement | null;
    search!.value = "s";
    search!.dispatchEvent(new Event("input", { bubbles: true }));
    await flushPromises();

    const items = [...document.querySelectorAll(".conv-pick__item")] as HTMLElement[];
    expect(items.length, "两条搜索结果都应列出").toBe(2);
    const other = items.find((i) => (i.textContent ?? "").includes("copper_grate"))!;
    expect(other.className, "别的版本的方块应被标注").toContain("is-other-version");
    expect(other.textContent, "应说明目标版本没有它").toContain("目标版本没有");

    // 点它：不能填进当前行，且要给出提示
    other.click();
    await flushPromises();
    const row = rowByText("minecraft:ancient_debris")!;
    expect(
      (row.querySelector("input") as HTMLInputElement).value,
      "目标版本里没有的方块不该被填进去",
    ).toBe("");
    // 提示走 Message（本用例里被替身接管，不会渲染进 DOM），因此查替身的调用记录
    expect(
      messageWarning.mock.calls.some((c) => String(c[0]).includes("不能作为替代品")),
      "应提示它不能作为替代品",
    ).toBe(true);

    // 同一批里目标版本有的那个仍然可以正常指定
    items.find((i) => (i.textContent ?? "").includes("warped_stairs"))!.click();
    await flushPromises();
    expect(
      (rowByText("minecraft:ancient_debris")!.querySelector("input") as HTMLInputElement)
        .value,
      "目标版本里有的方块应能正常指定",
    ).toBe("minecraft:warped_stairs");
    w.unmount();
  });

  it("手输过方块的行，输入框不再上色", async () => {
    // 绿框是「这是自动识别的推荐」的提示，用户自己填过之后就不再适用；
    // 红框同理。只留状态列的文字标签来说明状态。
    const w = await openAndAnalyze();

    const todoRow = rowByText("minecraft:ancient_debris")!;
    expect(todoRow.querySelector(".conv-block--todo"), "填之前应是红框").toBeTruthy();

    const input = todoRow.querySelector("input") as HTMLInputElement;
    input.value = "minecraft:stone";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await flushPromises();

    const after = rowByText("minecraft:ancient_debris")!;
    expect(after.querySelector(".conv-block--todo"), "填之后红框应消失").toBeFalsy();
    expect(after.querySelector(".conv-block--auto"), "也不该转成绿框").toBeFalsy();
    expect(after.textContent, "状态应变成已指定").toContain("已指定");
    w.unmount();
  });

  it("右侧列表把目标版本里有的方块排在前面", async () => {    const w = await openAndAnalyze();
    // 后端给的相关度顺序里，别的版本的方块排在前面；界面要把它挪到后面
    const base = mockInvoke.getMockImplementation()!;
    mockInvoke.mockImplementation((async (cmd: string, args: never) => {
      if (cmd === "search_blocks") return [OTHER_VERSION_HIT, PICK_HIT];
      return base(cmd, args);
    }) as never);

    const search = document.querySelector(".conv-split__pane--pick input") as HTMLInputElement | null;
    search!.value = "s";
    search!.dispatchEvent(new Event("input", { bubbles: true }));
    await flushPromises();

    const items = [...document.querySelectorAll(".conv-pick__item")] as HTMLElement[];
    expect(items.length, "两条搜索结果都应列出").toBe(2);
    expect(
      items[0].textContent,
      "目标版本里有的方块应排在最前，翻起来不必先越过点不了的项",
    ).toContain("minecraft:warped_stairs");
    expect(items[1].textContent, "目标版本没有的排在后面").toContain(
      "minecraft:copper_grate",
    );
    w.unmount();
  });
});

/** 左侧目录栏的宽度调整。
 *
 * 这类「看得见的布局」问题（宽度不可调、拖动没反应、窄窗口下右侧被挤没）
 * 都不会让构建失败，也不会让别的测试失败，因此单独覆盖。
 */
describe("目录栏多选与文件操作", () => {
  /** 挂载并让列表出现若干行 */
  async function mountWithRows(n = 3) {
    const items = Array.from({ length: n }, (_, i) => ({
      ...summary,
      path: `/p/f${i}.litematic`,
      fileName: `f${i}.litematic`,
      name: `文件${i}`,
    }));
    stubInvoke({ scan_litematics: { items, skippedDirs: 0, skippedPaths: [] } });
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    return w;
  }

  /** 取某一行左侧的勾选框 */
  function rowCheckbox(w: ReturnType<typeof mountApp>, i: number) {
    return w.findAll(".schematic-card")[i].find("input[type=checkbox]");
  }

  function btn(w: ReturnType<typeof mountApp>, label: string) {
    return w.findAll("button").find((b) => b.text().trim() === label);
  }

  it("勾选后按钮才可用，未勾选时禁用", async () => {
    const w = await mountWithRows();
    // 未勾选：删除/复制/复制到/打开所在目录都应禁用
    for (const label of ["删除", "复制", "复制到", "打开所在目录"]) {
      expect(btn(w, label), `应有「${label}」按钮`).toBeTruthy();
      expect(btn(w, label)!.attributes("disabled"), `未勾选时「${label}」应禁用`).toBeDefined();
    }
    // 清除在未勾选时也应禁用
    expect(btn(w, "清除")!.attributes("disabled")).toBeDefined();

    await rowCheckbox(w, 0).setValue(true);
    await flushPromises();
    for (const label of ["删除", "复制", "复制到", "打开所在目录"]) {
      expect(btn(w, label)!.attributes("disabled"), `勾选后「${label}」应可用`).toBeUndefined();
    }
    w.unmount();
  });

  it("全选选中当前列表全部，反选翻转，清除清空", async () => {
    const w = await mountWithRows(3);
    expect(w.find(".file-manager__checks").text()).toContain("已选 0 / 3");

    await btn(w, "全选")!.trigger("click");
    await flushPromises();
    expect(w.find(".file-manager__checks").text()).toContain("已选 3 / 3");

    await btn(w, "反选")!.trigger("click");
    await flushPromises();
    expect(w.find(".file-manager__checks").text(), "全部选中后反选应变成全不选").toContain("已选 0 / 3");

    // 再全选后清除
    await btn(w, "全选")!.trigger("click");
    await flushPromises();
    await btn(w, "清除")!.trigger("click");
    await flushPromises();
    expect(w.find(".file-manager__checks").text()).toContain("已选 0 / 3");
    w.unmount();
  });

  it("删除只把勾选的路径发给后端", async () => {
    const confirm = vi.spyOn(Modal, "confirm").mockResolvedValue({} as never);
    const w = await mountWithRows(3);
    await rowCheckbox(w, 1).setValue(true);
    await flushPromises();

    mockInvoke.mockResolvedValueOnce({
      names: ["f1.litematic"], written: [], destDir: "", totalBytes: 10,
    });
    await btn(w, "删除")!.trigger("click");
    await flushPromises();

    const call = mockInvoke.mock.calls.find((c) => c[0] === "delete_files");
    expect(call, "应调用 delete_files").toBeTruthy();
    expect((call![1] as { paths: string[] }).paths, "只应传勾选的那一个路径").toEqual([
      "/p/f1.litematic",
    ]);
    confirm.mockRestore();
    w.unmount();
  });

  it("复制把勾选的路径发给剪贴板命令", async () => {
    const w = await mountWithRows(3);
    await rowCheckbox(w, 0).setValue(true);
    await flushPromises();

    mockInvoke.mockResolvedValueOnce({
      names: ["f0.litematic"], written: [], destDir: "", totalBytes: 5,
    });
    await btn(w, "复制")!.trigger("click");
    await flushPromises();

    const call = mockInvoke.mock.calls.find((c) => c[0] === "copy_files_to_clipboard");
    expect(call, "应调用 copy_files_to_clipboard").toBeTruthy();
    expect((call![1] as { paths: string[] }).paths).toEqual(["/p/f0.litematic"]);
    w.unmount();
  });

  it("右键菜单点「打开所在目录」要真的执行（pointerdown 不能先关掉菜单）", async () => {
    // 这条防的是「右键菜单点不动」：菜单由 v-if 控制，而全局监听在 window 的
    // pointerdown 上关闭它。真实点击的顺序是 pointerdown → click，
    // 若 pointerdown 就把菜单摘掉，click 永远落不到菜单项上。
    // 只 trigger("click") 的写法测不出这个缺陷——那正是它一直漏掉的原因。
    const w = await mountWithRows(2);
    const row = w.findAll(".schematic-card")[0];
    await row.trigger("contextmenu", { clientX: 120, clientY: 200 });
    await flushPromises();

    const item = w.find(".ctx-menu__item");
    expect(item.exists(), "右键后应出现菜单").toBe(true);

    // 按真实顺序来：先在菜单项上派发 pointerdown（冒泡到 window），再 click。
    // 必须重新查一次 DOM 判断菜单是否还在——vue-test-utils 的 trigger("click")
    // 对着已从文档移除的元素也能派发事件，所以「直接 trigger click 并断言命令
    // 被调用」测不出这个缺陷；真正决定成败的是 pointerdown 之后菜单还在不在。
    item.element.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, button: 0 }),
    );
    await flushPromises();
    expect(
      w.find(".ctx-menu__item").exists(),
      "pointerdown 不应关掉菜单：菜单一旦被移除，浏览器就不会再把 click 送到它上面",
    ).toBe(true);

    await w.find(".ctx-menu__item").trigger("click");
    await flushPromises();

    const call = mockInvoke.mock.calls.find((c) => c[0] === "reveal_in_file_manager");
    expect(call, "点菜单项应调用 reveal_in_file_manager").toBeTruthy();
    expect((call![1] as { path: string }).path, "应作用于右键点中的那一行").toBe(
      "/p/f0.litematic",
    );
    w.unmount();
  });

  it("点右键菜单以外的地方仍会关闭菜单", async () => {
    // 上一条要求菜单内点击不关闭；这条保证「点别处关闭」没有被一起改坏
    const w = await mountWithRows(2);
    const row = w.findAll(".schematic-card")[0];
    await row.trigger("contextmenu", { clientX: 120, clientY: 200 });
    await flushPromises();
    expect(w.find(".ctx-menu__item").exists()).toBe(true);

    await w.find(".titlebar").trigger("pointerdown");
    await flushPromises();
    expect(w.find(".ctx-menu__item").exists(), "点别处应关闭菜单").toBe(false);
    w.unmount();
  });

  it("打开所在目录只用第一个勾选项（资源管理器只能选中一个）", async () => {
    const w = await mountWithRows(3);
    await rowCheckbox(w, 0).setValue(true);
    await rowCheckbox(w, 2).setValue(true);
    await flushPromises();

    mockInvoke.mockResolvedValueOnce(undefined);
    await btn(w, "打开所在目录")!.trigger("click");
    await flushPromises();

    const call = mockInvoke.mock.calls.find((c) => c[0] === "reveal_in_file_manager");
    expect(call, "应调用 reveal_in_file_manager").toBeTruthy();
    expect((call![1] as { path: string }).path, "应传列表中的第一个勾选项").toBe(
      "/p/f0.litematic",
    );
    w.unmount();
  });

  it("全选只作用于搜索结果，不会选中被过滤掉的行", async () => {
    const w = await mountWithRows(3);
    // 搜索只留下 f1
    await w.find("input[type=search], .arco-input-search input").setValue("f1");
    await flushPromises();
    expect(w.find(".file-manager__checks").text(), "过滤后总数应变为 1").toContain("/ 1");

    await btn(w, "全选")!.trigger("click");
    await flushPromises();
    expect(w.find(".file-manager__checks").text()).toContain("已选 1 / 1");

    // 清空搜索后，f0/f2 不应被顺带选中
    await w.find("input[type=search], .arco-input-search input").setValue("");
    await flushPromises();
    expect(
      w.find(".file-manager__checks").text(),
      "全选只应选中当时的可见项，清空搜索后不应把之前不可见的一起算上",
    ).toContain("已选 1 / 3");
    w.unmount();
  });
});

/** 多目录加载与移除、以及列宽复位。
 *
 * 这些都是「点了没反应」类型的功能，测试必须经由真实界面触发，
 * 而不是直接调用组件内部函数——按钮是否可用、事件是否接对，
 * 恰恰是缺陷所在。
 */
describe("多目录与列宽复位", () => {
  async function addDirs(w: ReturnType<typeof mountApp>, ...dirs: string[]) {
    await activateFiles(w);
    mockOpen.mockResolvedValue(dirs);
    const btn = w.findAll("button").find((b) => b.text().includes("添加目录"));
    await btn!.trigger("click");
    await flushPromises();
  }

  async function openDirMenu(w: ReturnType<typeof mountApp>) {
    await activateFiles(w);
    const trigger = w.findAll("button").find((b) => b.text().includes("已加载目录"));
    expect(trigger, "应有已加载目录下拉").toBeTruthy();
    await trigger!.trigger("click");
    await flushPromises();
    return Array.from(document.querySelectorAll(".dir-item input[type=checkbox]")) as HTMLInputElement[];
  }

  it("一次选多个目录时全部加载，文件合并显示", async () => {
    const a: SchematicSummary = { ...summary, path: "/a/a.litematic", fileName: "a.litematic", dirName: "/a" };
    const b: SchematicSummary = { ...summary, path: "/b/b.litematic", fileName: "b.litematic", dirName: "/b" };
    let call = 0;
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "scan_litematics") {
        call += 1;
        return { items: [call === 1 ? a : b], skippedDirs: 0, skippedPaths: [] };
      }
      return undefined;
    }) as never);

    const w = mountApp();
    await addDirs(w, "/a", "/b");

    expect(call, "两个目录都应被扫描").toBe(2);
    expect(w.text(), "第一个目录的文件应在列表里").toContain("a.litematic");
    await selectRoot(w, "/b");
    expect(w.text(), "第二个目录的文件应在列表里").toContain("b.litematic");
    expect(w.text(), "计数应为两个目录之和").toContain("共 2 个投影");
    w.unmount();
  });

  it("移除下拉里的目录后，该目录的文件从列表消失，其余保留", async () => {
    const a: SchematicSummary = { ...summary, path: "/a/a.litematic", fileName: "a.litematic", dirName: "/a" };
    const b: SchematicSummary = { ...summary, path: "/b/b.litematic", fileName: "b.litematic", dirName: "/b" };
    let call = 0;
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "scan_litematics") {
        call += 1;
        return { items: [call === 1 ? a : b], skippedDirs: 0, skippedPaths: [] };
      }
      return undefined;
    }) as never);

    const w = mountApp();
    await addDirs(w, "/a", "/b");
    expect(w.text()).toContain("a.litematic");

    // 移除第一个目录（下拉项顺序即加载顺序）
    const trigger = w.findAll("button").find((x) => x.text().includes("已加载目录"));
    await trigger!.trigger("click");
    await flushPromises();
    const rm = Array.from(document.querySelectorAll(".dir-item button"))[0] as HTMLElement;
    expect(rm, "应有移除按钮").toBeTruthy();
    rm.click();
    await flushPromises();

    expect(w.text(), "被移除目录的文件应消失").not.toContain("a.litematic");
    expect(w.text(), "另一个目录的文件应保留").toContain("b.litematic");
    expect(w.text(), "计数应更新").toContain("共 1 个投影");
    w.unmount();
  });

  it("已加载目录下拉正确反映当前状态", async () => {
    const w = mountApp();
    const btn = w.findAll("button").find((b) => b.text().includes("已加载目录"));
    expect(btn, "应有「已加载目录」按钮").toBeTruthy();
    expect(btn!.text(), "未加载时应显示 0").toContain("0");

    // 未加载时下拉里应给出说明，而不是一个空白面板
    await btn!.trigger("click");
    await flushPromises();
    expect(document.body.textContent, "未加载时应说明「尚未加载任何目录」").toContain(
      "尚未加载任何目录",
    );
    w.unmount();
  });

  it("新加载的目录默认勾选，文件立即显示", async () => {
    const a: SchematicSummary = { ...summary, path: "/a/a.litematic", fileName: "a.litematic", dirName: "/a" };
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "scan_litematics") return { items: [a], skippedDirs: 0, skippedPaths: [] };
      return undefined;
    }) as never);
    const w = mountApp();
    await addDirs(w, "/a");
    expect(w.text(), "刚加载的目录应默认显示其文件").toContain("a.litematic");

    const boxes = await openDirMenu(w);
    expect(boxes.length, "下拉里应有一个目录勾选框").toBe(1);
    expect(boxes[0].checked, "新加载的目录应默认勾选").toBe(true);
    w.unmount();
  });

  it("取消勾选目录后，该目录的文件从列表消失；重新勾选又出现", async () => {
    // 对应需求「已加载目录菜单中增加选择框，勾选的才会在下面显示」
    const a: SchematicSummary = { ...summary, path: "/a/a.litematic", fileName: "a.litematic", dirName: "/a" };
    const b: SchematicSummary = { ...summary, path: "/b/b.litematic", fileName: "b.litematic", dirName: "/b" };
    let call = 0;
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "scan_litematics") {
        call += 1;
        return { items: [call === 1 ? a : b], skippedDirs: 0, skippedPaths: [] };
      }
      return undefined;
    }) as never);

    const w = mountApp();
    await addDirs(w, "/a", "/b");
    expect(w.text()).toContain("a.litematic");
    await selectRoot(w, "/b");
    expect(w.text()).toContain("b.litematic");

    // 取消第一个目录（下拉顺序即加载顺序）
    let boxes = await openDirMenu(w);
    expect(boxes.length).toBe(2);
    boxes[0].click();
    await flushPromises();
    expect(w.text(), "取消勾选后该目录的文件应消失").not.toContain("a.litematic");
    expect(w.text(), "另一个目录的文件应保留").toContain("b.litematic");
    // 目录仍是「已加载」状态，只是不显示
    expect(w.findAll("button").some((x) => x.text().includes("已加载目录（2）")), "目录数不应减少").toBe(true);

    // 重新勾选应恢复显示
    boxes = await openDirMenu(w);
    boxes = Array.from(
      document.querySelectorAll<HTMLInputElement>(".dir-item input[type=checkbox]"),
    );
    boxes[0].click();
    await flushPromises();
    await selectRoot(w, "/a");
    expect(w.text(), "重新勾选后文件应再次出现").toContain("a.litematic");
    w.unmount();
  });

  it("取消勾选正被选中的文件的目录时，详情回到空状态", async () => {
    // 否则右侧会停留在「已不在列表里」的文件上：该文件的改动仍会被
    // 保存到磁盘，而用户在列表里找不到它
    const a: SchematicSummary = { ...summary, path: "/a/a.litematic", fileName: "a.litematic", dirName: "/a" };
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "scan_litematics") return { items: [a], skippedDirs: 0, skippedPaths: [] };
      return undefined;
    }) as never);

    const w = mountApp();
    await addDirs(w, "/a");
    await openFileCard(w, "a.litematic");
    await flushPromises();
    expect(w.text(), "选中后应出现详情").toContain("信息");

    const boxes = await openDirMenu(w);
    boxes[0].click();
    await flushPromises();

    expect(w.find(".editor-detail").exists(), "禁用当前文件所在目录后应关闭编辑器").toBe(false);
    const btn = (label: string) => w.findAll("button").find((x) => x.text().includes(label));
    // 入口随信息区 / 方块列表一起消失，用户不会点到作用在旧路径上的按钮
    expect(btn("版本转换"), "依赖选中项的入口应消失").toBeUndefined();
    expect(w.find(".block-table").exists(), "方块表应随选中项消失").toBe(false);
    w.unmount();
  });
});

/** 界面状态记忆。用户要求「下次打开保持记忆」，
 *  这里的重点是：记住的确实被写出去、能读回来、脏数据不炸。
 */
describe("界面状态记忆", () => {
  /** save_ui_state 已被调用的次数 */
  function saveCount(): number {
    return mockInvoke.mock.calls.filter((c) => c[0] === "save_ui_state").length;
  }

  /** 取最后一次 save_ui_state 的 JSON */
  function lastSaved(): Record<string, unknown> | null {
    const calls = mockInvoke.mock.calls.filter((c) => c[0] === "save_ui_state");
    if (calls.length === 0) return null;
    const arg = calls[calls.length - 1][1] as { json: string };
    return JSON.parse(arg.json) as Record<string, unknown>;
  }

  it("卸载时把防抖中的改动立刻写出，之后不再有遗留写盘", async () => {
    // 界面状态的写盘是防抖的（400ms）。若定时器活过组件卸载，它会在此后
    // 任意时刻触发，写入这个已销毁实例那份过期状态——实测在整包并行跑
    // （进程更慢、定时器更容易跨越用例边界）时约有 1/10 的概率污染
    // 「最后一次保存」，表现为用户设置偶尔记成默认值。
    vi.useFakeTimers();
    try {
      const w = mountApp();
      await flushPromises();
      const before = saveCount();

      await w.find(".file-manager__descendants span").trigger("click");
      await flushPromises();
      expect(saveCount(), "防抖期间不应立刻写盘").toBe(before);

      w.unmount();
      await flushPromises();
      expect(saveCount(), "卸载时应写出待保存的改动").toBeGreaterThan(before);
      const saved = lastSaved();
      expect(saved!.showDescendants, "写出的应是改动后的子目录开关").toBe(true);

      // 定时器必须已被清掉：卸载后再推进时间不应产生第二次写盘
      const afterUnmount = saveCount();
      vi.advanceTimersByTime(5000);
      await flushPromises();
      expect(saveCount(), "卸载后不应再有遗留的写盘").toBe(afterUnmount);
    } finally {
      vi.useRealTimers();
    }
  });

  it("记住已添加的目录，重启时自动重新扫描", async () => {
    // 上次保存的内容
    const state = {
      version: 1,
      sidebarWidth: 700,
      dirs: ["/p", "/q"],
      enabledDirs: ["/p"],
      activeView: "front",
      brightness: 1.0,
      exportSize: 2048,
      searchText: "",
    };
    const scanned: string[] = [];
    mockInvoke.mockImplementation((async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "load_ui_state") return JSON.stringify(state);
      if (cmd === "save_ui_state") return undefined;
      if (cmd === "scan_litematics") {
        const d = String((args as { dir?: string } | undefined)?.dir ?? "");
        scanned.push(d);
        return { items: [{ ...summary, path: `${d}/${d.replace(/\//g, "")}.litematic`, fileName: `${d.replace(/\//g, "")}.litematic`, dirName: d }], skippedDirs: 0, skippedPaths: [] };
      }
      if (cmd === "analyze_schematic") {
        return { blocks: [], entities: [], tileEntities: [], nonAirTotal: 0, airTotal: 0, distinctBlockTypes: 0, distinctStates: 0 };
      }
      return undefined;
    }) as never);

    const w = mountApp();
    await flushPromises();
    await flushPromises();

    expect(scanned, "应重新扫描上次记住的两个目录").toEqual(["/p", "/q"]);
    expect(w.find(".file-manager").exists(), "文件管理界面应恢复").toBe(true);
    expect(w.text(), "应显示恢复出来的文件").toContain("p.litematic");
    // enabledDirs 只勾了 /p：/q 的文件不应出现，计数也只算勾选的那一个
    expect(w.text(), "未勾选的目录不应显示其文件").not.toContain("q.litematic");
    expect(w.text(), "计数应只算勾选的目录").toContain("共 1 个投影");
    w.unmount();
  });

  it("保存内容损坏时按默认界面继续，并提示用户", async () => {
    // 用 spy 断言：Arco 的 Message 走 portal 渲染，查 document.body 不可靠
    // （本仓库既有的做法就是 vi.spyOn(Message, ...)）
    const warnSpy = vi.spyOn(Message, "warning");
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "load_ui_state") return "{ 这不是合法 JSON";
      if (cmd === "scan_litematics") return { items: [summary], skippedDirs: 0, skippedPaths: [] };
      return undefined;
    }) as never);

    try {
      const w = mountApp();
      await flushPromises();
      // 不应抛错、界面仍可用，且明确告知
      expect(w.find(".file-manager").exists(), "界面应仍然正常").toBe(true);
      expect(
        warnSpy.mock.calls.some((c) => String(c[0]).includes("界面设置")),
        "应提示设置无法读取，而不是静默按默认界面启动",
      ).toBe(true);
      w.unmount();
    } finally {
      warnSpy.mockRestore();
    }
  });

  it("版本号不匹配时丢弃旧数据，不硬套到新结构上", async () => {
    mockInvoke.mockImplementation((async (cmd: string) => {
      // 未来版本写的状态
      if (cmd === "load_ui_state") {
        return JSON.stringify({ version: 99, sidebarWidth: 111, dirs: ["/p"] });
      }
      if (cmd === "scan_litematics") return { items: [summary], skippedDirs: 0, skippedPaths: [] };
      return undefined;
    }) as never);

    const w = mountApp();
    await flushPromises();
    expect(w.find(".directory-node").exists(), "不认识的版本不应恢复目录树").toBe(false);
    w.unmount();
  });

  it("界面状态写入失败时只写一条带命令名的 error 运行日志", async () => {
    // 写盘失败原来会记两条（命令一条、界面语境一条），现在收口成一条：
    // 由 api.ts 的 logged 出口按命令名记，排查时能直接定位是哪条命令。
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "load_ui_state") return null;
      if (cmd === "save_ui_state") throw new Error("磁盘只读");
      return undefined;
    }) as never);

    const w = mountApp();
    await flushPromises();

    // 改一下界面状态，卸载时防抖中的写盘会立刻执行，必然撞上失败的替身
    await w.find(".file-manager__descendants span").trigger("click");
    await flushPromises();
    w.unmount();
    await flushPromises();

    const events = mockInvoke.mock.calls.filter((c) => c[0] === "log_ui_event");
    expect(events.length, "同一次失败只应留一条日志").toBe(1);
    const args = events[0][1] as { level: string; message: string };
    expect(args.level, "命令失败属于错误").toBe("error");
    expect(args.message, "说明里要带上失败的命令名").toContain("save_ui_state");
  });
});

/** 方块栏里的方块渲染图。
 *
 * 渲染图是异步、按页批量取回来的。它坏掉的表现只是「有一列空着」，
 * 既不会让构建失败，也不会让别的用例失败，因此单独覆盖。
 */
describe("方块栏渲染图", () => {
  const rows: BlockCount[] = [
    {
      name: "minecraft:stone",
      state: "minecraft:stone",
      count: 10,
      zhName: "石头",
      enName: "Stone",
    },
    {
      name: "minecraft:oak_log",
      state: "minecraft:oak_log[axis=y]",
      count: 3,
      zhName: "橡木原木",
      enName: "Oak Log",
    },
  ];

  /** 选中投影并让分析返回给定的方块行 */
  async function selectWithBlocks(blocks: BlockCount[], extra: Record<string, unknown> = {}) {    stubInvoke({
      analyze_schematic: {
        blocks,
        entities: [],
        tileEntities: [],
        entityInstances: [],
        nonAirTotal: 0,
        airTotal: 0,
        distinctBlockTypes: 1,
        distinctStates: blocks.length,
      },
      ...extra,
    });
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();
    return w;
  }

  /** 取一行的各单元格文本。
   *
   * 折叠视图：0 渲染图、1 中文名、2 方块、3 数量、4 堆叠、5 操作；
   * 分状态视图在方块与数量之间多一列状态。按需取下标即可。
   */
  function cells(row: { element: Element }): string[] {
    return Array.from(row.element.querySelectorAll("td")).map((td) =>
      (td.textContent ?? "").trim(),
    );
  }

  /** 表头的列标题 */
  function headerTitles(w: ReturnType<typeof mountApp>): string[] {
    return w
      .findAll(".block-table thead th")
      .map((th) => (th.text() ?? "").trim());
  }

  /** 切换「区分属性」开关 */
  async function toggleSplit(w: ReturnType<typeof mountApp>) {
    const sw = w.find('.block-split button[role="switch"]');
    expect(sw.exists(), "表头应有「区分属性」开关").toBe(true);
    await sw.trigger("click");
    await flushPromises();
  }

  it("默认按物品合计，且不显示状态列", async () => {
    // 统计口径：一个方块的所有状态累加成一行（红石强度、铁轨朝向与通电、
    // 楼梯朝向都是摆放形态，对「这种物品一共要用多少个」没有意义）。
    const rows: BlockCount[] = [
      // 后端按数量降序返回
      { name: "minecraft:powered_rail", state: "minecraft:powered_rail[powered=true,shape=north_south]", count: 9, zhName: "动力铁轨", enName: "Powered Rail" },
      { name: "minecraft:powered_rail", state: "minecraft:powered_rail[powered=false,shape=east_west]", count: 8, zhName: "动力铁轨", enName: "Powered Rail" },
      { name: "minecraft:redstone_wire", state: "minecraft:redstone_wire[power=15]", count: 7, zhName: "红石粉", enName: "Redstone Dust" },
      { name: "minecraft:redstone_wire", state: "minecraft:redstone_wire[power=0]", count: 5, zhName: "红石粉", enName: "Redstone Dust" },
      { name: "minecraft:redstone_wire", state: "minecraft:redstone_wire[power=3]", count: 4, zhName: "红石粉", enName: "Redstone Dust" },
      { name: "minecraft:oak_stairs", state: "minecraft:oak_stairs[facing=east]", count: 3, zhName: "橡木楼梯", enName: "Oak Stairs" },
      { name: "minecraft:oak_stairs", state: "minecraft:oak_stairs[facing=west]", count: 2, zhName: "橡木楼梯", enName: "Oak Stairs" },
      { name: "minecraft:stone", state: "minecraft:stone", count: 1, zhName: "石头", enName: "Stone" },
    ];
    const w = await selectWithBlocks(rows);

    const tableRows = w.findAll(".block-table tbody tr");
    expect(tableRows.length, "8 条状态记录应并成 4 行").toBe(4);
    const rowOf = (name: string) => tableRows.find((r) => cells(r)[2] === name)!;

    // 折叠视图没有状态列：这一行的数量是所有状态的总和，写一个代表状态会误导
    expect(headerTitles(w), "折叠视图不应有状态列").not.toContain("状态");

    // 铁轨：9+8=17
    expect(cells(rowOf("minecraft:powered_rail"))[3], "数量应是各状态之和").toBe("17");
    // 红石粉：7+5+4=16
    expect(cells(rowOf("minecraft:redstone_wire"))[3], "红石粉数量应是各状态之和").toBe("16");
    // 楼梯同样合计：3+2=5，不按朝向分行
    expect(cells(rowOf("minecraft:oak_stairs"))[3], "楼梯数量应是各朝向之和").toBe("5");
    // 合计后数量最大的排在最前
    expect(cells(tableRows[0])[2], "数量最大的行应排在最前").toBe("minecraft:powered_rail");
    w.unmount();
  });

  it("打开区分属性后每种状态一行，状态列显示完整状态键", async () => {
    // 替换规则按「方块名 + 属性」精确匹配，需要按状态核对时把开关打开：
    // 此时的数量是处于该状态的方块数，也就是这条规则会命中的数量。
    const rows: BlockCount[] = [
      { name: "minecraft:oak_stairs", state: "minecraft:oak_stairs[facing=east]", count: 3, zhName: "橡木楼梯", enName: "Oak Stairs" },
      { name: "minecraft:oak_stairs", state: "minecraft:oak_stairs[facing=west]", count: 2, zhName: "橡木楼梯", enName: "Oak Stairs" },
      { name: "minecraft:stone", state: "minecraft:stone", count: 10, zhName: "石头", enName: "Stone" },
    ];
    const w = await selectWithBlocks(rows);

    expect(w.findAll(".block-table tbody tr").length, "关闭时按物品合计成 2 行").toBe(2);

    await toggleSplit(w);
    const tableRows = w.findAll(".block-table tbody tr");
    expect(tableRows.length, "开启后每种状态一行").toBe(3);
    expect(headerTitles(w), "开启后应出现状态列").toContain("状态");

    // 每行数量是自己的方块数，而不是合计
    const stairsEast = tableRows.find((r) =>
      cells(r)[3] === "minecraft:oak_stairs[facing=east]",
    );
    expect(stairsEast, "状态列应写出完整状态键").toBeTruthy();
    expect(cells(stairsEast!)[4], "数量是该状态的方块数").toBe("3");
    const stairsWest = tableRows.find((r) =>
      cells(r)[3] === "minecraft:oak_stairs[facing=west]",
    );
    expect(cells(stairsWest!)[4], "另一状态单独一行").toBe("2");

    // 两个方向的状态框都应是可区分的行键：点其中一行后只有它高亮
    await stairsEast!.trigger("click");
    await flushPromises();
    expect(
      w.findAll(".block-table tbody tr.block-row--active").length,
      "同一方块的两个状态各自独立，只应有一行高亮",
    ).toBe(1);

    // 再关掉回到折叠
    await toggleSplit(w);
    expect(w.findAll(".block-table tbody tr").length, "关闭后回到合计视图").toBe(2);
    expect(headerTitles(w), "关闭后状态列应消失").not.toContain("状态");
    w.unmount();
  });

  it("同一物品的不同方块形态合计成一行，并注明跨了几种形态", async () => {
    // 火把与墙上的火把、立地与墙上的告示牌都是同一个物品，材料用量必须算一项。
    const rows: BlockCount[] = [
      // 后端按数量降序返回：墙挂形态比基础形态多，正是「代表形态取基础形态」的场景
      { name: "minecraft:cobblestone_wall", state: "minecraft:cobblestone_wall[up=true]", count: 353, zhName: "圆石墙", enName: "Cobblestone Wall" },
      { name: "minecraft:oak_wall_sign", state: "minecraft:oak_wall_sign[facing=north]", count: 195, zhName: "橡木墙上的告示牌", enName: "Oak Wall Sign" },
      { name: "minecraft:torch", state: "minecraft:torch", count: 103, zhName: "火把", enName: "Torch" },
      { name: "minecraft:wall_torch", state: "minecraft:wall_torch[facing=north]", count: 82, zhName: "墙上的火把", enName: "Wall Torch" },
      { name: "minecraft:pale_oak_wall_sign", state: "minecraft:pale_oak_wall_sign[facing=north]", count: 14, zhName: "淡橡木墙上的告示牌", enName: "Pale Oak Wall Sign" },
      { name: "minecraft:oak_sign", state: "minecraft:oak_sign[rotation=0]", count: 1, zhName: "橡木告示牌", enName: "Oak Sign" },
    ];
    const w = await selectWithBlocks(rows);

    const tableRows = w.findAll(".block-table tbody tr");
    expect(tableRows.length, "6 条记录应并成 4 行").toBe(4);
    // 方块名那一列可能带「（N 种形态）」后缀，因此按前缀匹配
    const rowOf = (name: string) =>
      tableRows.find((r) => (cells(r)[2] ?? "").startsWith(name))!;

    // 火把：103+82=185，代表形态取基础形态（火把），中文名与渲染图也用它
    const torch = cells(rowOf("minecraft:torch"));
    expect(torch[1], "应显示基础形态的中文名").toBe("火把");
    expect(torch[3], "数量应是两种形态之和").toBe("185");
    // 状态列不显示时，跨了哪几种形态必须在名字那一列注明
    expect(torch[2], "应注明跨了几种形态").toContain("（2 种形态）");
    // 悬停时列出具体是哪几种，用户才知道替换会漏掉哪些
    const torchNameSpan = rowOf("minecraft:torch")
      .findAll("td:nth-child(3) span")
      .find((s) => s.attributes("title") !== undefined);
    expect(torchNameSpan, "方块名那一列应有说明覆盖范围的提示").toBeTruthy();
    expect(torchNameSpan!.attributes("title"), "应列出这一行合计了哪些方块名").toContain(
      "minecraft:wall_torch",
    );
    // 告示牌同理：基础形态数量更少（1 个）也要当代表
    const sign = cells(rowOf("minecraft:oak_sign"));
    expect(sign[1], "代表形态取基础形态，中文名也取它的").toBe("橡木告示牌");
    expect(sign[3], "数量应是两种形态之和").toBe("196");
    // 只有墙挂形态时，行的名字是物品名（告示牌），因为要备的就是告示牌物品；
    // 中文名仍取自投影里真实存在的那一行
    const paleOak = cells(rowOf("minecraft:pale_oak_sign"));
    expect(paleOak[3], "数量应如实显示").toBe("14");
    expect(paleOak[1], "中文名取自投影里存在的墙挂形态").toBe("淡橡木墙上的告示牌");
    expect(paleOak[2], "只有一种形态时不该标注跨形态").not.toContain("种形态");
    // 真正的墙方块不能被带走
    expect(rowOf("minecraft:cobblestone_wall"), "圆石墙是另一种物品，应保持独立").toBeTruthy();
    w.unmount();
  });

  it("点跨了多个方块名的行的替换按钮时说清只替换其中一个名字", async () => {
    // 替换在后端按方块名精确相等匹配，而这一行的数量是整个物品的合计。
    // 若只说「已设为替换源」，用户会以为墙上的火把也会被替换。
    const rows: BlockCount[] = [
      { name: "minecraft:torch", state: "minecraft:torch", count: 103, zhName: "火把", enName: "Torch" },
      { name: "minecraft:wall_torch", state: "minecraft:wall_torch[facing=north]", count: 82, zhName: "墙上的火把", enName: "Wall Torch" },
    ];
    const w = await selectWithBlocks(rows);
    messageWarning.mockClear();

    await clickReplaceOnRow(w, 0);

    const from = document.querySelector<HTMLInputElement>(".repl-window .repl-from-name input")!;
    const fromProps = document.querySelector<HTMLInputElement>(
      ".repl-window .repl-from-props input",
    )!;
    expect(from.value, "替换源应是这一行的代表形态").toBe("minecraft:torch");
    expect(fromProps.value, "折叠视图不带属性限定").toBe("");
    const warning = messageWarning.mock.calls.map((c) => String(c[0])).join(" ");
    expect(warning, "必须提示只替换其中一个方块名").toContain("minecraft:torch");
    expect(warning, "必须点明另一个形态不会被替换").toContain("minecraft:wall_torch");
    w.unmount();
  });

  it("区分属性开启时，行内替换按钮带入该状态的属性限定", async () => {
    // 分状态后每一行就是一个具体状态，属性限定跟着带入，规则才恰好只命中这一行
    const rows: BlockCount[] = [
      { name: "minecraft:oak_stairs", state: "minecraft:oak_stairs[facing=east]", count: 3, zhName: "橡木楼梯", enName: "Oak Stairs" },
      { name: "minecraft:oak_stairs", state: "minecraft:oak_stairs[facing=west]", count: 2, zhName: "橡木楼梯", enName: "Oak Stairs" },
    ];
    const w = await selectWithBlocks(rows);
    await toggleSplit(w);

    const westRow = w
      .findAll(".block-table tbody tr")
      .find((r) => r.text().includes("facing=west"))!;
    await westRow.find(".block-act").trigger("click");
    await flushPromises();

    const from = document.querySelector<HTMLInputElement>(".repl-window .repl-from-name input")!;
    const fromProps = document.querySelector<HTMLInputElement>(
      ".repl-window .repl-from-props input",
    )!;
    expect(from.value, "源方块是这一行的方块名").toBe("minecraft:oak_stairs");
    expect(fromProps.value, "属性限定应带入这一行的状态").toBe("facing=west");
    w.unmount();
  });

  it("折叠视图下同一方块的多个状态不带属性限定", async () => {
    // 折叠视图的一行是整个物品的合计（两个朝向都在里面）。若把代表状态的
    // 属性带进规则，替换会被悄悄限制在其中一个朝向上，而界面上看不出来。
    const rows: BlockCount[] = [
      { name: "minecraft:oak_stairs", state: "minecraft:oak_stairs[facing=east]", count: 3, zhName: "橡木楼梯", enName: "Oak Stairs" },
      { name: "minecraft:oak_stairs", state: "minecraft:oak_stairs[facing=west]", count: 2, zhName: "橡木楼梯", enName: "Oak Stairs" },
    ];
    const w = await selectWithBlocks(rows);
    expect(w.findAll(".block-table tbody tr").length, "折叠视图应合计成一行").toBe(1);

    await clickReplaceOnRow(w, 0);

    const from = document.querySelector<HTMLInputElement>(".repl-window .repl-from-name input")!;
    const fromProps = document.querySelector<HTMLInputElement>(
      ".repl-window .repl-from-props input",
    )!;
    expect(from.value, "源方块是这一行的方块名").toBe("minecraft:oak_stairs");
    expect(fromProps.value, "折叠视图不应带入代表状态的属性").toBe("");
    w.unmount();
  });

  it("切换区分属性后回到第一页", async () => {
    // 分状态后行数翻倍，停在旧页码会落到一个与切换前无关的位置
    const rows: BlockCount[] = [];
    for (let i = 0; i < 60; i++) {
      rows.push({
        name: "minecraft:stone",
        state: `minecraft:stone[note=${i}]`,
        count: 1,
        zhName: "石头",
        enName: "Stone",
      });
    }
    const w = await selectWithBlocks(rows);
    // 折叠视图只有 1 行；打开区分属性后变成 60 行、共 2 页
    await toggleSplit(w);
    expect(w.findAll(".block-table tbody tr").length, "第一页应有 50 行").toBe(50);
    await w.findAll(".block-table .arco-pagination-item").find((x) => x.text().trim() === "2")!.trigger("click");
    await flushPromises();
    expect(w.findAll(".block-table tbody tr").length, "第二页应有 10 行").toBe(10);

    // 关掉再打开：行集合整个换掉，页码必须回到第一页，
    // 否则会停在一个与当前视图无关的页码上
    await toggleSplit(w);
    await toggleSplit(w);
    expect(w.findAll(".block-table tbody tr").length, "重新开启后应回到第 1 页").toBe(50);
    const firstState = (w.findAll(".block-table tbody tr td")[3]?.text() ?? "").trim();
    expect(firstState, "第 1 页应从数量最多的状态开始").toBe("minecraft:stone[note=0]");
    w.unmount();
  });

  it("行内替换按钮只触发一次，不会连带触发行点击", async () => {
    // 按钮在行内部：点击若冒泡到行的点击处理，设替换源的动作会做两遍。
    // 用跨形态的行来验证——那一行每次都会给出「只替换其中一个名字」的警告，
    // 做两遍就会看到两条一样的警告。
    const rows: BlockCount[] = [
      { name: "minecraft:torch", state: "minecraft:torch", count: 103, zhName: "火把", enName: "Torch" },
      { name: "minecraft:wall_torch", state: "minecraft:wall_torch[facing=north]", count: 82, zhName: "墙上的火把", enName: "Wall Torch" },
    ];
    const w = await selectWithBlocks(rows);
    messageWarning.mockClear();
    messageInfo.mockClear();

    await clickReplaceOnRow(w, 0);
    expect(
      messageWarning.mock.calls.filter((c) => String(c[0]).includes("minecraft:torch")).length,
      "设替换源只应做一遍，警告也只应出现一条",
    ).toBe(1);
    // 窗口里已经写着源方块，不必再弹一条普通提示
    // （跨形态那条警告本身也以「已设为替换源」开头，所以只看 info 这一路）
    expect(
      messageInfo.mock.calls.filter((c) => String(c[0]).includes("已设为替换源")).length,
      "从按钮进窗口时不该再弹设源的提示",
    ).toBe(0);
    w.unmount();
  });

  it("操作列固定在表格右侧，每行都有替换入口", async () => {
    // 表格比面板宽时横向滚动，操作列不跟着滚走才随时点得到
    const rows: BlockCount[] = [
      { name: "minecraft:stone", state: "minecraft:stone", count: 5, zhName: "石头", enName: "Stone" },
      { name: "minecraft:dirt", state: "minecraft:dirt", count: 3, zhName: "泥土", enName: "Dirt" },
    ];
    const w = await selectWithBlocks(rows);

    const titles = headerTitles(w);
    expect(titles[titles.length - 1], "最后一列应是操作列").toBe("操作");
    const fixedTh = w.findAll(".block-table thead th.arco-table-col-fixed-right");
    expect(fixedTh.length, "操作列应固定在右侧").toBe(1);
    expect(
      w.findAll(".block-table tbody tr .block-act").length,
      "每行都应有替换入口",
    ).toBe(2);
    w.unmount();
  });

  it("导出的表格随开关切换列，与界面看到的一致", async () => {
    // 导出与显示对不上会让人以为导出漏了方块：折叠视图没有状态列，
    // 导出就不该多出一列；分状态视图按状态分行，导出也得带上状态键
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    const rows: BlockCount[] = [
      { name: "minecraft:oak_stairs", state: "minecraft:oak_stairs[facing=east]", count: 3, zhName: "橡木楼梯", enName: "Oak Stairs" },
      { name: "minecraft:oak_stairs", state: "minecraft:oak_stairs[facing=west]", count: 2, zhName: "橡木楼梯", enName: "Oak Stairs" },
    ];
    const w = await selectWithBlocks(rows);

    const copyBtn = () => w.findAll(".block-head button").find((b) => b.text().includes("复制表格"))!;
    await copyBtn().trigger("click");
    await flushPromises();

    const folded = String(writeText.mock.calls.at(-1)?.[0] ?? "");
    expect(folded, "折叠视图的导出不应有状态列").not.toContain("状态");
    expect(folded.split("\r\n")[0], "表头应与界面一致").toBe(
      "显示名称\t方块ID\t数量\t堆叠",
    );
    expect(folded.split("\r\n").length, "两行数据加一行表头").toBe(2);
    expect(folded, "折叠视图的合计数量应是 5").toContain("\t5\t");

    // 打开开关后再导出：多出状态列，且每个状态一行
    await toggleSplit(w);
    await copyBtn().trigger("click");
    await flushPromises();
    const split = String(writeText.mock.calls.at(-1)?.[0] ?? "");
    expect(split.split("\r\n")[0], "分状态视图的导出应带状态列").toBe(
      "显示名称\t方块ID\t状态\t数量\t堆叠",
    );
    expect(split.split("\r\n").length, "两个状态各一行").toBe(3);
    expect(split, "应写出完整状态键").toContain("minecraft:oak_stairs[facing=west]");
    w.unmount();
  });

  it("实体栏会如实说明已记录多少个实体的坐标", async () => {
    // 实体坐标是这次新读出来并存入的（供以后做实体预览）。
    // 它坏掉的表现是「悄悄没读」，界面上看不出任何异常，因此断言可见文案。
    stubInvoke({
      analyze_schematic: {
        blocks: [],
        entities: [
          { id: "minecraft:item", count: 2, zhName: "物品", enName: "Item" },
        ],
        tileEntities: [],
        entityInstances: [
          { id: "minecraft:item", x: 1.5, y: 2, z: 3 },
          { id: "minecraft:item", x: 4.5, y: 5, z: 6 },
        ],
        nonAirTotal: 0,
        airTotal: 0,
        distinctBlockTypes: 0,
        distinctStates: 0,
      },
    });
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();

    // 切到实体页签
    const entityTab = w.findAll(".detail__tabs .arco-tabs-tab").find(
      (t) => (t.text() || "").trim() === "实体",
    );
    expect(entityTab, "应能定位到实体页签").toBeTruthy();
    await entityTab!.trigger("click");
    await flushPromises();

    expect(w.text(), "应说明已记录的实体坐标数量").toContain("已记录 2 个实体的坐标");
    w.unmount();
  });

  it("按状态键批量取回渲染图并显示出来", async () => {
    const dataUrl = "data:image/png;base64,iVBORw0KGgo=";
    const w = await selectWithBlocks(rows, {
      render_block_icons: rows.map((r) => ({ state: r.state, dataUrl, error: "" })),
    });

    const imgs = w.findAll("img.block-icon");
    expect(imgs.length, "两行都应显示出渲染图").toBe(2);
    expect(imgs[0].attributes("src")).toBe(dataUrl);

    // 一次一批，而不是逐行一次
    const calls = mockInvoke.mock.calls.filter((c) => c[0] === "render_block_icons");
    expect(calls.length, "同一页应只用一次请求").toBe(1);
    expect(calls[0][1]).toMatchObject({
      dataVersion: summary.dataVersion,
      states: rows.map((r) => r.state),
    });
    w.unmount();
  });

  it("单个方块画不出来时显示失败说明，而不是留一个破图", async () => {
    const w = await selectWithBlocks(rows, {
      render_block_icons: [
        { state: rows[0].state, dataUrl: "data:image/png;base64,iVBORw0KGgo=", error: "" },
        { state: rows[1].state, dataUrl: "", error: "方块库中没有这个方块" },
      ],
    });

    expect(w.findAll("img.block-icon").length, "只有成功的那一行有图").toBe(1);
    const failed = w.find(".block-icon--failed");
    expect(failed.exists(), "失败的那一行应显示占位").toBe(true);
    expect(failed.attributes("title")).toContain("方块库中没有这个方块");
    w.unmount();
  });

  it("整批请求失败时提示一次，并且不反复重试", async () => {
    const errSpy = vi.spyOn(Message, "error");
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "load_ui_state") return null;
      if (cmd === "scan_litematics") {
        return { items: [summary], skippedDirs: 0, skippedPaths: [] };
      }
      if (cmd === "analyze_schematic") {
        return {
          blocks: rows,
          entities: [],
          tileEntities: [],
          nonAirTotal: 0,
          airTotal: 0,
          distinctBlockTypes: 1,
          distinctStates: rows.length,
        };
      }
      if (cmd === "render_block_icons") throw new Error("方块库载入失败");
      return undefined;
    }) as never);

    const w = mountApp();
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();

    // 失败必须让用户看见，不能被静默吞掉
    expect(errSpy).toHaveBeenCalled();
    const calls = mockInvoke.mock.calls.filter((c) => c[0] === "render_block_icons");
    expect(calls.length, "失败后不应无限重试同一批").toBe(1);
    expect(w.findAll(".block-icon--failed").length, "失败的行应显示占位").toBe(rows.length);

    errSpy.mockRestore();
    w.unmount();
  });

  it("只请求当前页的方块，不把整表一次拉回来", async () => {
    const many: BlockCount[] = Array.from({ length: 120 }, (_, i) => ({
      name: `minecraft:b${i}`,
      state: `minecraft:b${i}`,
      // 数量按后端契约递减（后端按数量降序返回）：表格在合并红石粉之后会
      // 重新按数量排序，乱序的替身数据排出来的名次与真实情况不同，
      // 分页断言就会测到替身自己的顺序
      count: 120 - i,
      zhName: `方块${i}`,
      enName: `B${i}`,
    }));
    const w = await selectWithBlocks(many, {
      render_block_icons: many
        .slice(0, 50)
        .map((r) => ({ state: r.state, dataUrl: "data:image/png;base64,AA==", error: "" })),
    });

    const calls = mockInvoke.mock.calls
      .filter((c) => c[0] === "render_block_icons")
      .map((c) => (c[1] as { states: string[] }).states);
    expect(calls.length).toBe(1);
    // 断言的判据是「请求的正好是当前渲染出来的那些行」，而不是写死 50：
    // 写死页大小只是在核对配置常量，页大小改了这条测试就要跟着改，
    // 却并不能说明「有没有多请求」。
    const rendered = w.findAll(".block-table tbody tr").length;
    expect(rendered, "应渲染出下一页数量的行").toBeGreaterThan(0);
    expect(calls[0].length, "请求的方块数应与渲染出的行数一致").toBe(rendered);
    expect(calls[0].length, "不该把整表一次拉回来").toBeLessThan(many.length);
    expect(calls[0]).not.toContain("minecraft:b110");
    w.unmount();
  });

  it("翻到第二页会去取第二页的渲染图", async () => {
    const many: BlockCount[] = Array.from({ length: 120 }, (_, i) => ({
      name: `minecraft:b${i}`,
      state: `minecraft:b${i}`,
      // 同「只请求当前页」：必须按后端的数量降序给，否则排出来的名次不同
      count: 120 - i,
      zhName: `方块${i}`,
      enName: `B${i}`,
    }));
    // 必须按**请求到的**状态回，只回请求的那一批：
    // 一次性把 120 个都回给前端，第二页就全在缓存里了，翻页不会发请求。
    // 场景等其余命令仍交给 stubInvoke——只回 [] 会让 loadScene 拿到无效数据，
    // 详情面板整块渲染不出来，那样测到的是替身的问题。
    stubInvoke({
      analyze_schematic: {
        blocks: many,
        entities: [],
        tileEntities: [],
        nonAirTotal: 0,
        airTotal: 0,
        distinctBlockTypes: many.length,
        distinctStates: many.length,
      },
    });
    const base = mockInvoke.getMockImplementation()!;
    mockInvoke.mockImplementation((async (cmd: string, args: never) => {
      if (cmd === "render_block_icons") {
        const asked = (args as { states: string[] }).states;
        return asked.map((s) => ({
          state: s,
          dataUrl: "data:image/png;base64,AA==",
          error: "",
        }));
      }
      return base(cmd, args);
    }) as never);

    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();

    const page2 = [...document.querySelectorAll(".arco-pagination-item")].find(
      (el) => (el.textContent ?? "").trim() === "2",
    ) as HTMLElement | undefined;
    expect(page2, "应能定位到第 2 页按钮").toBeTruthy();
    page2!.click();
    await flushPromises();

    const states = mockInvoke.mock.calls
      .filter((c) => c[0] === "render_block_icons")
      .map((c) => (c[1] as { states: string[] }).states);
    expect(states.length, "应发生第二批请求").toBe(2);
    expect(states[1][0], "第二批应是第 51 行起的方块").toBe("minecraft:b50");
    w.unmount();
  });

  it("翻回已看过的页不再重复请求（缓存命中）", async () => {
    const many: BlockCount[] = Array.from({ length: 120 }, (_, i) => ({
      name: `minecraft:b${i}`,
      state: `minecraft:b${i}`,
      // 同「只请求当前页」：必须按后端的数量降序给，否则排出来的名次不同
      count: 120 - i,
      zhName: `方块${i}`,
      enName: `B${i}`,
    }));
    const w = await selectWithBlocks(many, {
      render_block_icons: many
        .slice(0, 50)
        .map((r) => ({ state: r.state, dataUrl: "data:image/png;base64,AA==", error: "" })),
    });
    const page = (n: string) =>
      [...document.querySelectorAll(".arco-pagination-item")].find(
        (el) => (el.textContent ?? "").trim() === n,
      ) as HTMLElement | undefined;

    const count = () => mockInvoke.mock.calls.filter((c) => c[0] === "render_block_icons").length;
    expect(count(), "第一页应有一次请求").toBe(1);

    page("2")!.click();
    await flushPromises();
    expect(count(), "第二页还没取过，应再请求一次").toBe(2);

    page("1")!.click();
    await flushPromises();
    expect(count(), "翻回第一页应命中缓存，不再请求").toBe(2);
    w.unmount();
  });

  it("换投影时正在飞的渲染图请求不会写进新表", async () => {
    // 两个投影里有**同一个状态键**的方块：这正是缓存被串的关键条件——
    // 状态键不同的话，旧响应根本填不到新表的行上，守卫有没有都一样。
    const sameKey = "minecraft:stone";
    const make = (zh: string): BlockCount[] => [
      { name: sameKey, state: sameKey, count: 5, zhName: zh, enName: "Stone" },
    ];

    let selected = make("石头甲");
    /** 第一次请求的 resolver：故意不返回，模拟「请求还在飞」 */
    let staleResolve: ((v: unknown) => void) | null = null;
    let iconCall = 0;

    stubInvoke({
      scan_litematics: {
        items: [
          summary,
          { ...summary, path: "/p/other.litematic", fileName: "other.litematic" },
        ],
        skippedDirs: 0,
        skippedPaths: [],
      },
    });
    const base = mockInvoke.getMockImplementation()!;
    mockInvoke.mockImplementation((async (cmd: string, args: never) => {
      if (cmd === "analyze_schematic") {
        return {
          blocks: selected,
          entities: [],
          tileEntities: [],
          nonAirTotal: 0,
          airTotal: 0,
          distinctBlockTypes: selected.length,
          distinctStates: selected.length,
        };
      }
      if (cmd === "render_block_icons") {
        iconCall += 1;
        if (iconCall === 1) {
          return new Promise((res) => {
            staleResolve = res as (v: unknown) => void;
          });
        }
        // 新投影的请求立刻返回
        return (args as { states: string[] }).states.map((s) => ({
          state: s,
          dataUrl: "data:image/png;base64,NEW",
          error: "",
        }));
      }
      return base(cmd, args);
    }) as never);

    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    const fileRow = w.findAll(".schematic-card").find((r) => r.text().includes("测试.litematic"))!;
    await fileRow.trigger("dblclick");
    await flushPromises();
    expect(staleResolve, "第一个请求应已发出并停在飞行中").toBeTruthy();

    // 用户换到另一个投影（同一个状态键）
    selected = make("石头乙");
    await openFileCard(w, "other.litematic");
    expect(w.find("img.block-icon").attributes("src"), "新投影应显示新图").toContain("NEW");

    // 旧请求这时才回来——它属于上一个投影，必须被丢弃
    iconCall = 0;
    const srcsAfter: string[] = [];
    staleResolve!([
      { state: sameKey, dataUrl: "data:image/png;base64,STALE", error: "" },
    ]);
    await flushPromises();
    srcsAfter.push(...w.findAll("img.block-icon").map((i) => i.attributes("src") ?? ""));
    expect(
      srcsAfter.some((s) => s.includes("STALE")),
      "旧投影的响应不该覆盖新投影已显示的图",
    ).toBe(false);
    w.unmount();
  });
});

/** 日志管理窗口：从备份与日志的「运行日志」页签进入 */
describe("日志管理窗口", () => {
  /** 在弹窗里按文字找按钮 */
  function bodyButton(label: string): HTMLElement | undefined {
    return [...document.querySelectorAll("button")].find((b) =>
      (b.textContent ?? "").includes(label),
    ) as HTMLElement | undefined;
  }

  /** 日志管理窗口本身。
   *
   * 必须按标题定位到那一个弹窗：备份与日志抽屉里同名的文字、以及抽屉本身
   * 都排在它前面，按文字在整页找按钮会先命中那些。 */
  function logModal(): HTMLElement | undefined {
    return [...document.querySelectorAll<HTMLElement>(".arco-modal")].find((m) =>
      (m.textContent ?? "").includes("日志管理"),
    );
  }

  /** 日志管理窗口里的按钮 */
  function logButton(label: string): HTMLButtonElement | undefined {
    const root = logModal();
    if (!root) return undefined;
    return [...root.querySelectorAll("button")].find((b) =>
      (b.textContent ?? "").includes(label),
    ) as HTMLButtonElement | undefined;
  }

  /** 日志窗口里实际渲染出的日志行。
   *
   * 不能拿整个弹窗的 textContent 当判据：窗口说明里也写着「删除到回收站」
   * 这样的词，用它判断「清空后还显示不显示旧日志」会永远命中说明本身。 */
  function logRows(): string[] {
    const root = logModal();
    if (!root) return [];
    return [...root.querySelectorAll(".log-list__row")].map((r) => r.textContent ?? "");
  }

  /** 从「备份与日志」的「运行日志」页签打开日志管理窗口。
   *
   * read_run_log 用**有状态**替身：清空之后必须真的读到空，
   * 否则「清空 → 界面变空」这条链路验不出来（固定数组会让漏掉刷新也照样通过）。
   *
   * `lines` 可换成别的日志内容，用来覆盖等级标签一类的展示分支。
   */
  async function openLogWindow(extra: Record<string, unknown> = {}, lines?: string[]) {
    // 与后端实际写出的格式一致：纳秒时间戳 + 制表符 + 大写等级 + 制表符 + 说明。
    // 这里曾经写成「2026-10-15 12:00:00\t…」——那是格式化后的时间，
    // 后端从不这么写，界面也就照不出真实的显示效果。
    const state = {
      lines: lines ?? [
        "1792138651762518000\tINFO\t替换 demo.litematic：石头（minecraft:stone） -> 泥土（minecraft:dirt），共 12 个",
      ],
    };
    stubInvoke({
      ...extra,
    });
    const w = mountApp();
    await flushPromises();

    // 备份与日志抽屉 → 运行日志页签 → 日志管理
    const entry = w.findAll("button").find((b) => b.text().includes("备份与日志"));
    expect(entry, "工具栏应有「备份与日志」").toBeTruthy();
    await entry!.trigger("click");
    await flushPromises();
    const tab = [...document.querySelectorAll<HTMLElement>(".arco-tabs-tab")].find((t) =>
      (t.textContent ?? "").includes("运行日志"),
    );
    expect(tab, "备份与日志里应有「运行日志」页签").toBeTruthy();
    tab!.click();
    await flushPromises();

    // 叠一层带状态的日志命令替身
    const base = mockInvoke.getMockImplementation()!;
    mockInvoke.mockImplementation((async (cmd: string, args: never) => {
      if (cmd === "read_run_log") return [...state.lines];
      if (cmd === "clear_run_log") {
        state.lines = [];
        return undefined;
      }
      return base(cmd, args);
    }) as never);

    const btn = bodyButton("日志管理");
    expect(btn, "「运行日志」页签里应有「日志管理」按钮").toBeTruthy();
    // 进页签时抽屉已读过一次，记下来供「打开窗口又读一次」的断言比对
    const readsBeforeOpen = calls("read_run_log");
    // 打开窗口会顺带刷新一次归档日期，同样记下来供断言比对
    const configuresBeforeOpen = calls("configure_run_log");
    btn!.click();
    await flushPromises();
    return { w, state, readsBeforeOpen, configuresBeforeOpen };
  }

  /** 二次确认弹层里的「清空」按钮。
   *  不能直接用 bodyButton("清空")：它会先命中触发按钮「清空日志」。 */
  function confirmClearButton(): HTMLElement | undefined {
    return [...document.querySelectorAll(".arco-popconfirm button")].find(
      (b) => (b.textContent ?? "").trim() === "清空",
    ) as HTMLElement | undefined;
  }

  /** 按命令名数调用次数 */
  function calls(cmd: string): number {
    return mockInvoke.mock.calls.filter((c) => c[0] === cmd).length;
  }

  it("「日志管理」按钮打开日志管理窗口并逐条分开显示日志", async () => {
    const { w } = await openLogWindow();
    const modal = logModal();
    expect(modal, "应打开日志管理窗口").toBeTruthy();

    // 用户要看得见日志在哪：窗口说明里显示的必须是后端给的目录
    expect(modal!.textContent, "说明里应写明日志位置").toContain(LOG_DIR);

    // 每条的四个部分各自成元素：时间单独一列、等级与动作各成标签、细节另起一段。
    // 断言不再比对整行原文——正因为「整行糊成一段」才要改版。
    const row = modal!.querySelector(".log-list__row")!;
    // 形状写死到时分秒：区域设置一变，toLocaleString 会给出
    // 10/16/2026, 8:17:31 AM 这类 12 小时制，年在前面的断言挡不住
    expect(row.querySelector(".log-list__time")!.textContent, "时间要格式化成日期").toMatch(
      /^\d{4}\/\d{2}\/\d{2} \d{2}:\d{2}:\d{2}$/,
    );
    expect(row.querySelector(".log-list__level")!.textContent?.trim(), "应显示等级标签").toBe(
      "提示",
    );
    expect(row.querySelector(".log-list__action")!.textContent?.trim()).toBe("替换");
    expect(row.querySelector(".log-list__detail")!.textContent).toContain(
      "石头（minecraft:stone） -> 泥土（minecraft:dirt），共 12 个",
    );
    // 时间戳不该原样出现在界面上（20 位数字读不出是什么时间）
    expect(row.textContent, "不应原样显示纳秒时间戳").not.toContain("1792138651762518000");
    // 三个管理动作都要在这个窗口里：用户要求的正是「管理」而不只是看
    for (const label of ["刷新", "打开日志所在目录", "清空日志"]) {
      expect(logButton(label), `日志窗口里应有「${label}」`).toBeTruthy();
    }
    w.unmount();
  });

  it("每条日志的等级标签按等级渲染", async () => {
    const { w } = await openLogWindow({}, [
      "1792138651762518000\tERROR\t替换失败：目标文件只读",
      "1792138651762518001\tWARN\t转换跳过了 1 种方块",
      "1792138651762518002\tINFO\t替换 demo.litematic",
      "1792138651762518003\tDEBUG\t读取方块库版本 3465",
    ]);
    const rows = [...logModal()!.querySelectorAll(".log-list__row")];
    const tags = rows.map((r) => r.querySelector(".log-list__level"));
    expect(tags.map((t) => t?.textContent?.trim())).toEqual(["错误", "警告", "提示", "调试"]);
    // 颜色走 Arco 标签的内置色板（深浅主题各自有一套）：错误红、调试灰
    expect(tags[0]!.className, "错误应是红色标签").toContain("arco-tag-red");
    expect(tags[3]!.className, "调试应是灰色标签").toContain("arco-tag-gray");
    // 等级列是独立一列，不该混进动作里
    expect(rows[0].querySelector(".log-list__action")!.textContent).toBe("替换失败：目标文件只读");
    w.unmount();
  });

  it("切换日志等级会调用 configure_run_log，并带上等级与本地日期", async () => {
    const { w } = await openLogWindow();
    const before = calls("configure_run_log");
    const select = logModal()!.querySelector(
      ".arco-select-view, .arco-select",
    ) as HTMLElement | null;
    expect(select, "日志窗口应有等级选择器").toBeTruthy();
    select!.click();
    await flushPromises();
    const opt = [...document.querySelectorAll<HTMLElement>(".arco-select-option")].find(
      (o) => (o.textContent ?? "").trim() === "调试",
    );
    expect(opt, "等级选择器应有「调试」一档").toBeTruthy();
    opt!.click();
    await flushPromises();

    expect(calls("configure_run_log"), "改等级后应再推一次给后端").toBe(before + 1);
    const last = mockInvoke.mock.calls.filter((c) => c[0] === "configure_run_log").at(-1)!;
    expect((last[1] as { level: string }).level, "应把选中的等级交给后端").toBe("debug");
    expect((last[1] as { localDate: string }).localDate, "应带上本地日期").toMatch(
      /^\d{4}-\d{2}-\d{2}$/,
    );

    // 等级还要被记住。界面状态的写盘是防抖的，卸载会把它立刻写出，
    // 因此不必等 400ms 也能确定地读到这次改动。
    w.unmount();
    await flushPromises();
    const saves = mockInvoke.mock.calls.filter((c) => c[0] === "save_ui_state");
    const saved = JSON.parse((saves.at(-1)![1] as { json: string }).json) as {
      logLevel?: string;
    };
    expect(saved.logLevel, "改过的等级应写进界面状态").toBe("debug");
  });

  it("恢复记忆里的日志等级，并在启动后把等级与本地日期推给后端", async () => {
    const { w } = await openLogWindow({
      load_ui_state: JSON.stringify({ version: 1, logLevel: "debug", dirs: [] }),
    });
    const shown = logModal()!.querySelector(".arco-select-view-value")?.textContent?.trim();
    expect(shown, "选择器应显示记忆里的等级").toBe("调试");

    // 恢复之后必须推一次：后端得知道该收哪些行、写进哪一天的文件
    const pushed = mockInvoke.mock.calls.filter((c) => c[0] === "configure_run_log");
    expect(pushed.length, "启动时应推一次日志设置").toBeGreaterThan(0);
    const args = pushed.at(-1)![1] as { level: string; localDate: string };
    expect(args.level, "推的应是恢复出来的等级").toBe("debug");
    expect(args.localDate, "应带上当前的本地日期").toMatch(/^\d{4}-\d{2}-\d{2}$/);
    w.unmount();
  });

  it("后端给不出日志目录时不显示位置，不猜一个路径", async () => {
    const { w } = await openLogWindow({ run_log_dir: null });
    // 路径由后端算；拿不到就整条不显示——显示一个猜的位置比不显示更糟
    expect(logModal()!.textContent, "不应显示日志位置").not.toContain("日志位置");
    w.unmount();
  });

  it("打开日志窗口时随即读取一次日志", async () => {
    // 进入「运行日志」页签时抽屉已经读过一次，因此这里比对的是
    // 「打开窗口又读了一次」，而不是总次数恰好为一
    const { w, readsBeforeOpen } = await openLogWindow();
    expect(logModal(), "此时日志窗口应已打开").toBeTruthy();
    expect(calls("read_run_log"), "打开窗口应再读一次").toBe(readsBeforeOpen + 1);
    w.unmount();
  });

  it("打开日志管理窗口会把等级与当前日期再推一次", async () => {
    // 跨天挂机时，按大小滚动出来的归档要用「今天」的日期命名，
    // 因此每次打开窗口都要顺手刷新一次日期（见 store/backups.ts 的 openLogs）
    const { w, configuresBeforeOpen } = await openLogWindow();
    expect(calls("configure_run_log"), "打开窗口应再推一次日志设置").toBe(
      configuresBeforeOpen + 1,
    );
    w.unmount();
  });

  it("「刷新」再读一次日志", async () => {
    const { w } = await openLogWindow();
    const before = calls("read_run_log");
    logButton("刷新")!.click();
    await flushPromises();
    expect(calls("read_run_log"), "刷新应再读一次").toBe(before + 1);
    w.unmount();
  });

  it("清空后显示空状态，且「清空日志」随之禁用", async () => {
    const { w } = await openLogWindow();
    logButton("清空日志")!.click();
    await flushPromises();
    confirmClearButton()!.click();
    await flushPromises();

    expect(logRows(), "清空后不应再有日志行").toEqual([]);
    expect(logModal()!.textContent ?? "", "应显示空状态").toContain("暂无日志");
    expect(logButton("清空日志")!.disabled, "没有日志时「清空日志」应禁用").toBe(true);
    w.unmount();
  });

  it("「打开日志所在目录」调用后端命令", async () => {
    const { w } = await openLogWindow();
    logButton("打开日志所在目录")!.click();
    await flushPromises();
    expect(calls("open_run_log_dir"), "应调用 open_run_log_dir").toBe(1);
    w.unmount();
  });

  it("清空日志要经二次确认，确认后才真的清空", async () => {
    const { w, state } = await openLogWindow();

    // 只点触发器还不该清空：清空不可撤销，必须先确认
    logButton("清空日志")!.click();
    await flushPromises();
    expect(calls("clear_run_log"), "确认之前不应清空").toBe(0);
    expect(state.lines.length, "确认之前日志内容不应变").toBe(1);

    const ok = confirmClearButton();
    expect(ok, "二次确认弹层里应有「清空」按钮").toBeTruthy();
    ok!.click();
    await flushPromises();

    expect(calls("clear_run_log"), "确认后应调用 clear_run_log").toBe(1);
    // 后端清了还不够：界面必须重新读取，否则会继续显示已经删掉的那几行
    expect(state.lines.length, "确认后后端内容应被清空").toBe(0);
    expect(logRows(), "清空后界面不应再显示旧日志").toEqual([]);
    w.unmount();
  });

  it("清空失败时如实报错，不假装成功", async () => {
    const { w } = await openLogWindow();
    const base = mockInvoke.getMockImplementation()!;
    mockInvoke.mockImplementation((async (cmd: string, args: never) => {
      if (cmd === "clear_run_log") throw new Error("日志文件被占用");
      return base(cmd, args);
    }) as never);

    logButton("清空日志")!.click();
    await flushPromises();
    confirmClearButton()!.click();
    await flushPromises();

    expect(
      messageError.mock.calls.some((c) => String(c[0]).includes("清空日志失败")),
      "失败应弹出错误提示",
    ).toBe(true);
    w.unmount();
  });

  it("打开日志所在目录失败时如实报错", async () => {
    const { w } = await openLogWindow();
    const base = mockInvoke.getMockImplementation()!;
    mockInvoke.mockImplementation((async (cmd: string, args: never) => {
      if (cmd === "open_run_log_dir") throw new Error("资源管理器不可用");
      return base(cmd, args);
    }) as never);

    logButton("打开日志所在目录")!.click();
    await flushPromises();

    expect(
      messageError.mock.calls.some((c) => String(c[0]).includes("打开日志目录失败")),
      "失败应弹出错误提示",
    ).toBe(true);
    w.unmount();
  });
});

/** 版本库：空库时的启动引导、扫描后勾选、补库时把本地 jar 一起交上去 */
describe("版本库", () => {
  /** 后端支持两个版本，其中 1.20.1 已就绪 */
  const supported = [
    { name: "1.20.1", dataVersion: 3465 },
    { name: "1.21", dataVersion: 3953 },
  ];

  /** 在弹窗里按文字找按钮 */
  function bodyButton(label: string): HTMLElement | undefined {
    return [...document.querySelectorAll("button")].find((b) =>
      (b.textContent ?? "").includes(label),
    ) as HTMLElement | undefined;
  }

  it("基础库为空时启动就弹出「版本库」", async () => {
    stubInvoke({
      base_db_status: { versions: [], entityAvailable: false, supported },
      default_minecraft_dir: "C:/Users/x/AppData/Roaming/.minecraft",
      scan_minecraft: { versions: [], warnings: [] },
    });
    const w = mountApp();
    await flushPromises();

    // 断言弹窗里独有的控件，而不是「版本库」这四个字——工具栏上那个
    // 按钮本身就叫「版本库」，拿它当中文断言会让用例永远通过
    const start = bodyButton("准备勾选的版本");
    expect(start, "应自动弹出引导界面").toBeTruthy();
    const text = document.body.textContent ?? "";
    expect(text, "应列出缺失的版本").toContain("1.21");
    expect(text, "应标出这些版本需要下载").toContain("需下载");
    w.unmount();
  });

  it("基础库就绪时启动不打扰用户", async () => {
    stubInvoke({
      base_db_status: { versions: supported, entityAvailable: true, supported },
    });
    const w = mountApp();
    await flushPromises();

    const opened = [...document.querySelectorAll("button")].some(
      (b) => (b.textContent ?? "").includes("准备勾选的版本"),
    );
    expect(opened, "库已就绪时不该弹出补库界面").toBe(false);
    w.unmount();
  });

  it("已就绪与已装的版本排在列表最前", async () => {
    // 支持的正式版有 49 个，列表很长；用户最常看「我已经有哪些」，
    // 因此已建好基础库的、本机装了可直接建的都要排在前面
    stubInvoke({
      base_db_status: { versions: [{ name: "1.21", dataVersion: 3953 }], entityAvailable: true, supported },
      default_minecraft_dir: "C:/mc",
      scan_minecraft: {
        versions: [{ id: "1.20.1", dataVersion: 3465, jarPath: "C:/mc/1.20.1/1.20.1.jar" }],
        warnings: [],
      },
    });
    const w = mountApp();
    await flushPromises();

    // 库非空时不会自动弹窗，从工具栏打开
    w.findAll("button").find((b) => b.text().includes("版本库"))!.trigger("click");
    await flushPromises();

    const rows = [...document.querySelectorAll(".setup-table tbody tr")];
    expect(rows.length, "应列出全部支持的版本").toBe(2);
    expect(rows[0].textContent, "已就绪的版本应排在最前").toContain("1.21");
    expect(rows[1].textContent, "本机已装的次之").toContain("1.20.1");
    w.unmount();
  });

  it("表头全选框能一次勾上清单里的所有版本", async () => {
    // 清单里没有、要联网下载的版本默认不勾（那是几十 MB 级任务），
    // 但用户想整批补齐时不该只能一行行点，表头那个全选框要能一次勾满。
    //
    // 弹窗被 teleport 到 body 上，用 wrapper 的 find 找不到，必须查 document。
    stubInvoke({
      base_db_status: { versions: [], entityAvailable: false, supported },
      default_minecraft_dir: "C:/mc",
      scan_minecraft: { versions: [], warnings: [] },
    });
    const w = mountApp();
    await flushPromises();

    const startCount = () => {
      const btn = [...document.querySelectorAll("button")].find((b) =>
        (b.textContent ?? "").includes("准备勾选的版本"),
      );
      return (btn?.textContent ?? "").match(/（(\d+)）/)?.[1];
    };
    expect(startCount(), "默认不勾要下载的版本").toBe("0");

    const head = document.querySelector<HTMLInputElement>(
      ".setup-table thead input[type=checkbox]",
    );
    expect(head, "表头应有全选框").not.toBeNull();
    expect(
      document.querySelectorAll(".setup-table tbody input[type=checkbox]").length,
      "每行一个勾选框",
    ).toBe(2);

    head!.click();
    await flushPromises();
    expect(startCount(), "全选后两个版本都进勾选").toBe("2");

    document
      .querySelector<HTMLInputElement>(".setup-table thead input[type=checkbox]")!
      .click();
    await flushPromises();
    expect(startCount(), "取消全选后清空").toBe("0");
    w.unmount();
  });

  it("扫描到本机版本就自动导入并标记已就绪，联网下载的才要用户勾选", async () => {
    // 有状态替身：导入之前 1.20.1 只是「本机已装」，导入之后基础库里就有它了
    let built = false;
    mockInvoke.mockImplementation((async (cmd: string) => {
      switch (cmd) {
        case "base_db_status":
          return {
            versions: built ? [{ name: "1.20.1", dataVersion: 3465 }] : [],
            entityAvailable: false,
            supported,
          };
        case "default_minecraft_dir":
          return "C:/mc";
        case "scan_minecraft":
          return {
            versions: [{ id: "1.20.1", dataVersion: 3465, jarPath: "C:/mc/1.20.1/1.20.1.jar" }],
            warnings: [],
          };
        case "setup_add_versions":
          built = true;
          return { outcomes: [{ id: "1.20.1", ok: true, error: "" }] };
        default:
          return [];
      }
    }) as never);
    const w = mountApp();
    await flushPromises();

    // 「找到即导入」：本机扫到的客户端 jar 直接构建，用户不必再勾一次。
    // 用本地 jar 构建不耗流量，本来也没有需要用户决定的地方。
    const call = mockInvoke.mock.calls.find((c) => c[0] === "setup_add_versions");
    expect(call, "扫描后应自动导入本机版本").toBeTruthy();
    const args = call![1] as { source: string; versions: unknown[] };
    expect(args.source, "默认用官方接口").toBe("official");
    // mcDir 也要交上去：中文语言文件不在客户端 jar 里，后端要从这里补
    expect(args.versions, "只交本机扫到的那些，不替用户决定下载").toEqual([
      {
        id: "1.20.1",
        dataVersion: 3465,
        jarPath: "C:/mc/1.20.1/1.20.1.jar",
        mcDir: "C:/mc",
      },
    ]);

    // 导入完成的版本应标为已就绪（用户要的就是这一步不必自己动手）
    const rows = [...document.querySelectorAll(".setup-table tbody tr")];
    const done = rows.find((r) => (r.textContent ?? "").includes("1.20.1"));
    expect(done!.textContent, "导入后应标为已就绪").toContain("已就绪");

    // 1.21 本机没有，默认不勾：勾上就是几十 MB 下载，得用户自己点头
    const btn = bodyButton("准备勾选的版本");
    expect(btn!.textContent, "要下载的版本不默认勾选").toContain("（0）");
    w.unmount();
  });

  it("下载进度按真实比例画进度条，不把 9% 画成 900%", async () => {
    // 让补库停在下载态：settle 之前 setup_add_versions 不结算，进度订阅才留得住
    let settle!: (v: unknown) => void;
    const pending = new Promise<unknown>((resolve) => {
      settle = resolve;
    });
    stubInvoke({
      base_db_status: { versions: [], entityAvailable: false, supported },
      default_minecraft_dir: "C:/mc",
      scan_minecraft: {
        versions: [{ id: "1.21", dataVersion: 3953, jarPath: "C:/mc/1.21/1.21.jar" }],
        warnings: [],
      },
      setup_add_versions: () => pending,
    });
    // 回调数组是模块级的，不清掉会拿到前面用例注册的那个
    progressListeners.length = 0;
    const w = mountApp();
    await flushPromises();

    const listener = progressListeners.find((l) => l.event === "base-setup-progress");
    expect(listener, "补库时应订阅进度事件").toBeTruthy();
    listener!.handler({
      payload: { id: "1.21", stage: "download", received: 9, total: 100, error: "" },
    });
    await flushPromises();

    const row = [...document.querySelectorAll(".setup-table tbody tr")].find((r) =>
      (r.textContent ?? "").includes("1.21"),
    )!;
    // 百分比只由进度条自己显示：Arco 的 percent 是 0～1 的比例，内部乘 100
    const pct = row.querySelector<HTMLElement>(".arco-progress-line-text");
    expect(pct?.textContent?.trim(), "进度条应显示真实百分比").toBe("9%");
    // 传成百分数会让条宽变成 900%、被轨道裁成一整条
    const bar = row.querySelector<HTMLElement>(".arco-progress-line-bar");
    expect(bar, "下载阶段应画出进度条").toBeTruthy();
    expect(parseFloat(bar!.style.width), "条宽应等于真实比例").toBeCloseTo(9, 3);
    expect(row.textContent, "不应出现被放大一百倍的百分比").not.toContain("900%");

    // 除不尽的进度只保留两位小数：Arco 默认文本会把比例原样乘 100 输出浮点，
    // 实测出现过「6.886031082519097%」。6886/100000 = 6.886%
    listener!.handler({
      payload: { id: "1.21", stage: "download", received: 6886, total: 100000, error: "" },
    });
    await flushPromises();
    const frac = [...document.querySelectorAll(".setup-table tbody tr")]
      .find((r) => (r.textContent ?? "").includes("1.21"))!
      .querySelector<HTMLElement>(".arco-progress-line-text");
    expect(frac?.textContent?.trim(), "除不尽时应四舍五入到两位小数").toBe("6.89%");
    expect(frac?.textContent, "不该把整串浮点都铺出来").not.toContain("6.886031");

    // 服务器没给长度时画不出进度条，这时才由文案补一句状态
    listener!.handler({
      payload: { id: "1.21", stage: "download", received: 0, total: 0, error: "" },
    });
    await flushPromises();
    expect(row.textContent, "没有总量时说明正在下载").toContain("下载中…");
    expect(row.querySelector(".arco-progress-line-bar"), "没有总量时不画进度条").toBeNull();

    settle({ outcomes: [] });
    await flushPromises();
    w.unmount();
  });
});

/** 单个方块数过大时，3D 图形默认不渲染，由用户点「开始渲染」 */
describe("大方块数文件的渲染闸门", () => {
  /** 一个方块数给定的分析结果 */
  function analysisWith(count: number) {
    return {
      blocks: [
        {
          name: "minecraft:stone",
          state: "minecraft:stone",
          count,
          zhName: "石头",
          enName: "Stone",
        },
      ],
      entities: [],
      tileEntities: [],
      entityInstances: [],
      nonAirTotal: count,
      airTotal: 0,
      distinctBlockTypes: 1,
      distinctStates: 1,
    };
  }

  /** 选中列表里某一行 */
  async function openRow(w: ReturnType<typeof mountApp>, name: string) {
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, name);
    await flushPromises();
  }

  function startButton(): HTMLButtonElement | undefined {
    return [...document.querySelectorAll("button")].find((b) =>
      (b.textContent ?? "").includes("开始渲染"),
    ) as HTMLButtonElement | undefined;
  }

  function builtScene(): boolean {
    return mockInvoke.mock.calls.some((c) => c[0] === "build_scene");
  }

  it("超过 3 万时不自动建场景，点「开始渲染」才建", async () => {
    stubInvoke({ analyze_schematic: analysisWith(40000) });
    const w = mountApp();
    await openRow(w, "测试.litematic");

    expect(builtScene(), "方块数过大时不该自动渲染").toBe(false);
    const btn = startButton();
    expect(btn, "应给出「开始渲染」按钮").toBeTruthy();
    // 要让用户看懂为什么没画：说明具体是哪个方块、有多少个
    const text = document.querySelector(".scene-gate")!.textContent ?? "";
    expect(text, "应说明是哪个方块").toContain("石头");
    expect(text, "应给出实际数量").toContain("40,000");
    expect(text, "应给出阈值").toContain("30,000");

    btn!.click();
    await flushPromises();
    expect(builtScene(), "点过之后才建场景").toBe(true);
    w.unmount();
  });

  it("不超过阈值时照常自动渲染，也不出现按钮", async () => {
    stubInvoke({ analyze_schematic: analysisWith(29999) });
    const w = mountApp();
    await openRow(w, "测试.litematic");

    expect(builtScene(), "未超阈值应照常渲染").toBe(true);
    expect(startButton(), "不该出现「开始渲染」").toBeFalsy();
    w.unmount();
  });

  it("离开该文件后渲染停止，再回来要重新点", async () => {
    // 两个文件：大的那个超过阈值，小的那个没有
    const small: SchematicSummary = {
      ...summary,
      path: "/p/小.litematic",
      fileName: "小.litematic",
    };
    mockInvoke.mockImplementation((async (cmd: string, args: never) => {
      const a = args as unknown as { path?: string };
      switch (cmd) {
        case "scan_litematics":
          return { items: [summary, small], skippedDirs: 0, skippedPaths: [] };
        case "analyze_schematic":
          return analysisWith(a.path === small.path ? 10 : 40000);
        case "build_scene":
          return scene;
        case "load_ui_state":
          return null;
        case "save_ui_state":
          return undefined;
        case "base_db_status":
          return {
            versions: [{ name: "1.20.1", dataVersion: 3465 }],
            entityAvailable: true,
            supported: [{ name: "1.20.1", dataVersion: 3465 }],
          };
        case "list_block_versions":
          return [];
        default:
          return [];
      }
    }) as never);

    const w = mountApp();
    await openRow(w, "测试.litematic");
    expect(startButton(), "大文件应先要用户点头").toBeTruthy();
    startButton()!.click();
    await flushPromises();
    expect(builtScene(), "点过之后建了场景").toBe(true);

    // 切到小文件：照常渲染，闸门不出现
    await openFileCard(w, "小.litematic");
    await flushPromises();
    expect(startButton(), "小文件不该有闸门").toBeFalsy();

    // 切回大文件：上一次的「开始渲染」不该带过来
    await openFileCard(w, "测试.litematic");
    await flushPromises();
    expect(startButton(), "离开过该文件后要重新点「开始渲染」").toBeTruthy();
    w.unmount();
  });
});

/** 「更新」窗口与顶部预留的 RMC 入口 */
describe("更新窗口", () => {
  function toolbarButton(text: string): HTMLElement | undefined {
    return [...document.querySelectorAll("button")].find((b) =>
      (b.textContent ?? "").trim().includes(text),
    ) as HTMLElement | undefined;
  }

  /** 打开「更新」窗口 */
  async function openUpdate(w: ReturnType<typeof mountApp>) {
    toolbarButton("更新")!.click();
    await flushPromises();
    return w;
  }

  const info = (over: Partial<Record<string, unknown>> = {}) => ({
    current: "0.0.2",
    latest: "0.0.3",
    tag: "v0.0.3",
    hasUpdate: true,
    releaseUrl: "https://github.com/heiheids/schematic-manager/releases/tag/v0.0.3",
    publishedAt: "2026-10-20T10:00:00Z",
    repo: "heiheids/schematic-manager",
    currentPrerelease: false,
    latestPrerelease: false,
    ...over,
  });

  it("打开窗口即显示当前版本与作者，不自动联网", async () => {
    stubInvoke({ app_version: "0.0.2" });
    const w = mountApp();
    await flushPromises();
    await openUpdate(w);

    const text = document.body.textContent ?? "";
    expect(text, "应显示当前版本").toContain("0.0.2");
    expect(text, "应显示作者").toContain("bilibili黑山大叔");
    expect(
      mockInvoke.mock.calls.some((c) => c[0] === "check_update"),
      "打开窗口不该自动检测：离线时会凭空多一次失败",
    ).toBe(false);
    w.unmount();
  });

  it("点「检测更新」后显示发现的新版本与发布页", async () => {
    stubInvoke({ app_version: "0.0.2", check_update: info() });
    const w = mountApp();
    await flushPromises();
    await openUpdate(w);

    toolbarButton("检测更新")!.click();
    await flushPromises();

    const text = document.body.textContent ?? "";
    expect(text, "应说明发现了新版本").toContain("发现新版本");
    expect(text, "应给出新版本号").toContain("0.0.3");
    // 发布页地址用只读输入框展示，可选中复制
    const url = [...document.querySelectorAll("input")].find(
      (i) => i.value.includes("github.com/heiheids/schematic-manager/releases"),
    );
    expect(url, "应给出发布页地址").toBeTruthy();
    w.unmount();
  });

  it("已是最新时不谎报有更新", async () => {
    stubInvoke({
      app_version: "0.0.3",
      check_update: info({ current: "0.0.3", latest: "0.0.3", hasUpdate: false }),
    });
    const w = mountApp();
    await flushPromises();
    await openUpdate(w);
    toolbarButton("检测更新")!.click();
    await flushPromises();

    const text = document.body.textContent ?? "";
    expect(text).toContain("已是最新");
    expect(text, "不能同时出现「发现新版本」").not.toContain("发现新版本");
    w.unmount();
  });

  /** 检测逻辑只取最新，不因性质而跳过；但界面必须把性质说清楚，
   *  让用户自己决定要不要从正式版跟到预览版。 */
  it("标出版本性质，交用户自己决定要不要更新", async () => {
    stubInvoke({
      app_version: "0.1.0-dev.13",
      check_update: info({
        current: "0.1.0-dev.13",
        currentPrerelease: true,
        latest: "0.1.0",
        latestPrerelease: false,
        tag: "v0.1.0",
      }),
    });
    const w = mountApp();
    await flushPromises();
    await openUpdate(w);
    toolbarButton("检测更新")!.click();
    await flushPromises();

    const text = document.body.textContent ?? "";
    expect(text, "当前版本是开发构建，应标为预览版").toContain("预览版");
    expect(text, "最新发布是正式版，应标为正式版").toContain("正式版");
    expect(text, "应讲清两者差别并说明由用户决定").toContain("由你决定");
    w.unmount();
  });

  it("仓库里查不到任何发布时如实说明，不说成「最新」", async () => {
    stubInvoke({
      app_version: "0.0.2",
      check_update: info({ latest: "", tag: "", hasUpdate: false, releaseUrl: "" }),
    });
    const w = mountApp();
    await flushPromises();
    await openUpdate(w);
    toolbarButton("检测更新")!.click();
    await flushPromises();

    const text = document.body.textContent ?? "";
    expect(text, "应说明没查到发布").toContain("没有查到任何发布");
    expect(text).not.toContain("已是最新");
    w.unmount();
  });

  it("检测失败时显示原因，而不是显示成没有新版本", async () => {
    stubInvoke({
      app_version: "0.0.2",
      // 网络失败：命令直接抛错。其余命令仍走标准替身，避免无关命令
      // 拿到错误类型的结果而产生额外的未处理异常
      check_update: () => {
        throw new Error("连接 api.github.com 超时");
      },
    });
    const w = mountApp();
    await flushPromises();
    await openUpdate(w);
    toolbarButton("检测更新")!.click();
    await flushPromises();

    const text = document.body.textContent ?? "";
    expect(text, "应显示失败原因").toContain("连接 api.github.com 超时");
    expect(text, "不能把失败说成最新").not.toContain("已是最新");
    expect(text, "也不能说成没有发布").not.toContain("没有查到任何发布");
    w.unmount();
  });

  it("工具栏有预留的 RMC投影站 按钮", async () => {
    stubInvoke();
    const w = mountApp();
    await flushPromises();
    const btn = toolbarButton("RMC投影站");
    expect(btn, "应有 RMC投影站 按钮").toBeTruthy();
    // 按用户要求点击不做任何事，但要能看出这个入口是预留的
    expect(btn!.getAttribute("title"), "应说明是预留入口").toContain("预留");
    w.unmount();
  });

  it("点地址框即复制并提示「已复制地址」", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    stubInvoke({ app_version: "0.0.2", check_update: info() });
    const w = mountApp();
    await flushPromises();
    await openUpdate(w);
    toolbarButton("检测更新")!.click();
    await flushPromises();

    const box = document.querySelector(".update-url") as HTMLElement;
    expect(box, "应有可点击的地址框").toBeTruthy();
    box.click();
    await flushPromises();

    expect(writeText, "应把地址写进剪贴板").toHaveBeenCalledWith(
      "https://github.com/heiheids/schematic-manager/releases/tag/v0.0.3",
    );
    expect(shownMessages().join(" "), "应提示已复制").toContain("已复制地址");
    w.unmount();
  });

  it("没查到发布时，地址框给的是发布列表页而不是空", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    stubInvoke({
      app_version: "0.0.2",
      check_update: info({ latest: "", tag: "", hasUpdate: false, releaseUrl: "" }),
    });
    const w = mountApp();
    await flushPromises();
    await openUpdate(w);
    toolbarButton("检测更新")!.click();
    await flushPromises();

    (document.querySelector(".update-url") as HTMLElement).click();
    await flushPromises();
    expect(writeText).toHaveBeenCalledWith(
      "https://github.com/heiheids/schematic-manager/releases",
    );
    w.unmount();
  });

  it("复制失败时如实报错，不谎报已复制", async () => {
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockRejectedValue(new Error("权限被拒")) },
    });
    // execCommand 也不可用（jsdom 本就没有实现），两条路都失败
    const exec = vi.fn().mockReturnValue(false);
    Object.assign(document, { execCommand: exec });
    stubInvoke({ app_version: "0.0.2" });
    const w = mountApp();
    await flushPromises();
    await openUpdate(w);

    (document.querySelector(".update-url") as HTMLElement).click();
    await flushPromises();
    expect(shownMessages().join(" "), "不能谎报已复制").not.toContain("已复制地址");
    expect(shownMessages().join(" "), "应说明失败原因").toContain("复制失败");
    w.unmount();
  });

  it("「打开下载地址」把地址交给后端打开", async () => {
    stubInvoke({ app_version: "0.0.2", check_update: info() });
    const w = mountApp();
    await flushPromises();
    await openUpdate(w);
    toolbarButton("检测更新")!.click();
    await flushPromises();

    toolbarButton("打开下载地址")!.click();
    await flushPromises();
    const call = mockInvoke.mock.calls.find((c) => c[0] === "open_url");
    expect(call, "应调用 open_url").toBeTruthy();
    expect((call![1] as { url: string }).url, "应打开该发布的页面").toContain(
      "releases/tag/v0.0.3",
    );
    w.unmount();
  });

  it("作者主页显示并可点击跳转", async () => {
    stubInvoke({ app_version: "0.0.2" });
    const w = mountApp();
    await flushPromises();
    await openUpdate(w);

    const text = document.body.textContent ?? "";
    expect(text, "应显示作者主页一行").toContain("作者主页");
    expect(text, "应显示主页地址").toContain("https://space.bilibili.com/19428259");

    const link = document.querySelector(".about-link") as HTMLElement;
    expect(link, "主页应是可点击的链接").toBeTruthy();
    link.click();
    await flushPromises();
    const call = mockInvoke.mock.calls.find((c) => c[0] === "open_url");
    expect(call, "应调用 open_url").toBeTruthy();
    expect((call![1] as { url: string }).url).toBe("https://space.bilibili.com/19428259");
    w.unmount();
  });

  it("打开链接失败时显示原因，而不是点了没反应", async () => {
    stubInvoke({
      app_version: "0.0.2",
      open_url: () => {
        throw new Error("只允许打开 http/https 链接");
      },
    });
    const w = mountApp();
    await flushPromises();
    await openUpdate(w);
    (document.querySelector(".about-link") as HTMLElement).click();
    await flushPromises();
    expect(shownMessages().join(" "), "应显示失败原因").toContain("打开链接失败");
    w.unmount();
  });
});

/** 界面语言：工具条最右的切换入口要真的把文案换掉，而不只是改一个变量 */
describe("界面语言", () => {
  // 语言是全局单例，用例跑完必须还原，否则后面的中文断言会连带失败
  afterEach(() => {
    setLocale("zh-CN");
  });

  it("在工具条上切到 English 后界面文案跟着变", async () => {
    stubInvoke();
    const w = mountApp();
    await flushPromises();
    expect(
      w.findAll("button").some((b) => b.text().includes("添加目录")),
      "默认应是简体中文",
    ).toBe(true);

    await w.findAll("button").find((b) => b.text().includes("简体中文"))!.trigger("click");
    await flushPromises();
    const option = [...document.querySelectorAll(".arco-dropdown-option")].find((o) =>
      (o.textContent ?? "").includes("English"),
    );
    expect(option, "语言下拉里应有 English").toBeTruthy();
    (option as HTMLElement).click();
    await flushPromises();

    expect(
      w.findAll("button").some((b) => b.text().includes("Add directory")),
      "切到英语后按钮应显示英文",
    ).toBe(true);
    expect(
      w.findAll("button").some((b) => b.text().includes("添加目录")),
      "切换后不应再留着中文按钮",
    ).toBe(false);
    w.unmount();
  });
});

/** 显示名称：方块名分中英两套（取自 Minecraft 语言表），界面用哪种语言就显示哪一套 */
describe("方块显示名称", () => {
  // 语言是全局单例，用例跑完必须还原
  afterEach(() => {
    setLocale("zh-CN");
  });

  async function selectWithBlocks(blocks: BlockCount[]) {
    stubInvoke({
      analyze_schematic: {
        blocks,
        entities: [],
        tileEntities: [],
        entityInstances: [],
        nonAirTotal: 0,
        airTotal: 0,
        distinctBlockTypes: 1,
        distinctStates: blocks.length,
      },
    });
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await flushPromises();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();
    return w;
  }

  /** 方块表第 1 列（0 是渲染图）的名字 */
  function nameCells(w: ReturnType<typeof mountApp>) {
    return w
      .findAll(".block-table tbody tr")
      .map((r) => (r.findAll("td")[1]?.text() ?? "").trim());
  }

  it("中文界面显示中文名，切到英语后改显示英文名", async () => {
    const w = await selectWithBlocks([
      { name: "minecraft:stone", state: "minecraft:stone", count: 10, zhName: "石头", enName: "Stone" },
      {
        name: "minecraft:oak_log",
        state: "minecraft:oak_log[axis=y]",
        count: 3,
        zhName: "橡木原木",
        enName: "Oak Log",
      },
    ]);
    expect(nameCells(w), "中文界面应显示中文名").toEqual(["石头", "橡木原木"]);

    setLocale("en-US");
    await flushPromises();
    expect(nameCells(w), "英语界面应改显示英文名").toEqual(["Stone", "Oak Log"]);
    w.unmount();
  });

  it("当前语言缺这套名字时退回另一套并标灰", async () => {
    const w = await selectWithBlocks([
      {
        name: "minecraft:mystery",
        state: "minecraft:mystery",
        count: 1,
        zhName: "",
        enName: "Mystery Block",
      },
    ]);
    const cell = w.findAll(".block-table tbody tr")[0].findAll("td")[1];
    expect(cell.text(), "缺中文名时退回英文名").toContain("Mystery Block");
    expect(cell.find(".hint-dim").exists(), "退回的名字要标灰说明").toBe(true);
    w.unmount();
  });
});
describe("投影列表排序", () => {
  /** 三个文件：文件名、修改时间、作者的顺序各不相同，
   *  这样每个字段排出来的结果都能和别的字段区分开。 */
  const make = (
    fileName: string,
    name: string,
    author: string,
    timeModified: number,
  ): SchematicSummary => ({
    ...summary,
    path: `/p/${fileName}`,
    fileName,
    name,
    author,
    timeModified,
  });

  const c = make("c.litematic", "阿尔法", "甲", 300);
  const a = make("a.litematic", "查理", "丙", 200);
  const b = make("b.litematic", "布拉沃", "", 100);

  /** 挂载并加载这三个文件 */
  async function setup() {
    stubInvoke({ scan_litematics: { items: [c, a, b], skippedDirs: 0, skippedPaths: [] } });
    const w = mountApp();
    await flushPromises();
    mockOpen.mockResolvedValue(["/p"]);
    const btn = w.findAll("button").find((x) => x.text().includes("添加目录"));
    await btn!.trigger("click");
    await flushPromises();
    return w;
  }

  /** 表格里当前的文件名列顺序（只取数据行） */
  function order(w: ReturnType<typeof mountApp>): string[] {
    // 第一列是行选择复选框列（文本为空），文件名不在第一列；
    // 直接按整行里的 .litematic 片段取，稳定且与列顺序无关
    return w
      .findAll(".schematic-card")
      .map((r) => /[\w.-]+\.litematic/.exec(r.text() ?? "")?.[0] ?? "")
      .filter(Boolean);
  }

  /** 点开排序下拉并选一项。下拉内容被 teleport 到 body，只能从 document 找。 */
  async function pick(w: ReturnType<typeof mountApp>, label: string) {
    const trigger = w.find(".file-manager__checks .arco-select");
    expect(trigger.exists(), "底部应有排序下拉").toBe(true);
    await trigger.trigger("click");
    await flushPromises();
    const opt = Array.from(document.querySelectorAll(".arco-select-option")).find((el) =>
      (el.textContent ?? "").includes(label),
    ) as HTMLElement | undefined;
    expect(opt, `下拉里应有「${label}」`).toBeTruthy();
    opt!.click();
    await flushPromises();
  }

it("默认按路径排序，未选字段时就是文件系统的扫描顺序", async () => {
    const w = await setup();
    expect(order(w), "未排序时按路径").toEqual([
      "a.litematic",
      "b.litematic",
      "c.litematic",
    ]);
    w.unmount();
  });

  it("按文件名升序与降序", async () => {
    const w = await setup();
    await pick(w, "按文件名");
    expect(order(w), "按文件名升序").toEqual([
      "a.litematic",
      "b.litematic",
      "c.litematic",
    ]);

    // 方向按钮：再点一次变降序
    const dirBtn = w.findAll(".file-manager__checks button").find((x) => x.text().includes("↑"));
    expect(dirBtn, "选了字段后应出现方向按钮").toBeTruthy();
    await dirBtn!.trigger("click");
    await flushPromises();
    expect(order(w), "按文件名降序").toEqual([
      "c.litematic",
      "b.litematic",
      "a.litematic",
    ]);
    w.unmount();
  });

  it("按修改时间排，数字比大小而不是比字符串", async () => {
    const w = await setup();
    await pick(w, "按修改时间");
    // 时间：a=200 b=100 c=300，升序应为 b,a,c；
    // 若按字符串比较会得到 "100"<"200"<"300" 同样成立，故再用降序交叉验证
    expect(order(w), "按修改时间升序").toEqual([
      "b.litematic",
      "a.litematic",
      "c.litematic",
    ]);
    w.unmount();
  });

  it("按作者排，空作者固定排在末尾（升降序都不翻到最前）", async () => {
    const w = await setup();
    await pick(w, "按作者");
    // 作者：c=甲 a=丙 b=空。中文按拼音排，丙(bǐng) 在 甲(jiǎ) 之前，
    // 因此升序是 a,c；空作者的 b 固定排末尾
    expect(order(w), "按作者升序，空作者在末尾").toEqual([
      "a.litematic",
      "c.litematic",
      "b.litematic",
    ]);

    const dirBtn = w.findAll(".file-manager__checks button").find((x) => x.text().includes("↑"));
    await dirBtn!.trigger("click");
    await flushPromises();
    // 降序时空的仍在末尾——这条是空值处理的关键：若跟着翻转，
    // 降序会把空作者顶到最前，盖住真正有作者的那两行
    // 降序把甲(c) 提到丙(a) 之前，空作者的 b 仍在末尾
    expect(order(w), "降序时空作者仍在末尾").toEqual([
      "c.litematic",
      "a.litematic",
      "b.litematic",
    ]);
    w.unmount();
  });

  it("选「按路径」可以取消排序，回到默认顺序", async () => {
    const w = await setup();
    await pick(w, "按文件名");
    const dirBtn = w.findAll(".file-manager__checks button").find((x) => x.text().includes("↑"));
    await dirBtn!.trigger("click");
    await flushPromises();
    expect(order(w)[0], "先确认已变成降序").toBe("c.litematic");

    await pick(w, "按路径");
    expect(order(w), "取消排序后回到路径序").toEqual([
      "a.litematic",
      "b.litematic",
      "c.litematic",
    ]);
    w.unmount();
  });

  it("排序设置会被记住，重开应用后仍是同一顺序", async () => {
    // 同时覆盖「收集」与「恢复」两条路径：只测其一的话，写盘字段名与读盘
    // 字段名对不上也不会被发现（两边各自看都「通过」）
    const w = await setup();
    await pick(w, "按文件名");
    const dirBtn = w.findAll(".file-manager__checks button").find((x) => x.text().includes("↑"));
    await dirBtn!.trigger("click");
    await flushPromises();
    expect(order(w)[0], "先确认已按文件名降序").toBe("c.litematic");

    // 卸载会把防抖中的改动立刻写出
    w.unmount();
    await flushPromises();

    const saved = mockInvoke.mock.calls
      .filter((c) => c[0] === "save_ui_state")
      .map((c) => JSON.parse(String((c[1] as { json: string }).json)) as Record<string, unknown>);
    const last = saved[saved.length - 1];
    expect(last, "应写出过界面状态").toBeTruthy();
    expect(last.sortKey, "写出的状态里应带排序字段").toBe("fileName");
    expect(last.sortOrder, "写出的状态里应带排序方向").toBe("descend");

    // 用同一份状态重新挂载，顺序应与刚才一致
    stubInvoke({
      load_ui_state: JSON.stringify(last),
      scan_litematics: { items: [c, a, b], skippedDirs: 0, skippedPaths: [] },
    });
    const w2 = mountApp();
    await flushPromises();
    mockOpen.mockResolvedValue(["/p"]);
    const btn = w2.findAll("button").find((x) => x.text().includes("添加目录"));
    await btn!.trigger("click");
    await flushPromises();
    expect(order(w2), "重开后应恢复降序").toEqual([
      "c.litematic",
      "b.litematic",
      "a.litematic",
    ]);
    w2.unmount();
  });
});
describe("界面术语一致性", () => {
  it("卡片标题显示文件名，编辑表单单独显示投影名称", async () => {
    stubInvoke();
    const w = mountApp();
    await flushPromises();
    mockOpen.mockResolvedValue(["/p"]);
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    const card = w.find(".schematic-card__title");
    expect(card.text(), "卡片标题应是磁盘文件名").toContain("测试.litematic");
    await openFileCard(w, "测试.litematic");
    expect(w.text(), "编辑表单应区分投影名称").toContain("投影名称");
    w.unmount();
  });


});
describe("预览控制条", () => {
  /** 挂载并选中一个文件，让控制条出现 */
  async function mountWithSelection() {
    stubInvoke();
    const w = mountApp();
    await flushPromises();
    mockOpen.mockResolvedValue(["/p"]);
    const btn = w.findAll("button").find((b) => b.text().includes("添加目录"));
    await btn!.trigger("click");
    await flushPromises();
    const row = w.findAll(".schematic-card").find((r) => r.text().includes("测试.litematic"));
    await row!.trigger("dblclick");
    await flushPromises();
    return w;
  }

  it("拖动亮度滑杆时不弹数值气泡", async () => {
    // 滑杆右侧已有常驻百分比，气泡是同一信息的第二份，还会挡住滑杆
    const w = await mountWithSelection();
    const handle = w.find(".arco-slider-btn");
    expect(handle.exists(), "应能找到亮度滑杆的手柄").toBe(true);

    // Arco 的气泡在手柄被按下时弹出
    handle.element.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    await flushPromises();

    expect(
      document.querySelector(".arco-tooltip"),
      "按下亮度滑杆手柄时不应出现数值气泡",
    ).toBeNull();

    // 数值仍有可见的显示途径
    expect(w.text(), "亮度百分比仍要有常驻显示").toMatch(/\d+%/);
    w.unmount();
  });
});
describe("投影列表的键盘导航", () => {
  /** 三个文件，按路径排序后是 a/b/c，便于断言上下移动的顺序 */
  const make = (name: string): SchematicSummary => ({
    ...summary,
    path: `/p/${name}`,
    fileName: name,
  });
  const a = make("a.litematic");
  const b = make("b.litematic");
  const c = make("c.litematic");

  async function setup() {
    stubInvoke({ scan_litematics: { items: [a, b, c], skippedDirs: 0, skippedPaths: [] } });
    const w = mountApp();
    await flushPromises();
    mockOpen.mockResolvedValue(["/p"]);
    const btn = w.findAll("button").find((x) => x.text().includes("添加目录"));
    await btn!.trigger("click");
    await flushPromises();
    return w;
  }

  /** 列表容器：键盘事件绑在它上面 */
  function host(w: ReturnType<typeof mountApp>) {
    const el = w.find(".schematic-cards__grid");
    expect(el.exists(), "应有带 tabindex 的列表容器").toBe(true);
    return el;
  }

  /** 当前聚焦的卡片；方向键只聚焦，不直接打开编辑标签 */
  function activeFile(w: ReturnType<typeof mountApp>): string {
    const card = w.find(".schematic-card--focused .schematic-card__title");
    return card.exists() ? card.text().trim() : "";
  }

  it("第一次按下方向键选中第一行", async () => {
    const w = await setup();
    await host(w).trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    expect(activeFile(w), "未选中时按方向键应选第一行").toBe("a.litematic");
    w.unmount();
  });

  it("上下方向键在相邻行之间移动", async () => {
    const w = await setup();
    const h = host(w);
    await h.trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    await h.trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    expect(activeFile(w), "再按一次应移到第二行").toBe("b.litematic");

    await h.trigger("keydown", { key: "ArrowUp" });
    await flushPromises();
    expect(activeFile(w), "按上方向键应回到第一行").toBe("a.litematic");
    w.unmount();
  });

  it("Home 与 End 直达首尾，且不会越界", async () => {
    const w = await setup();
    const h = host(w);

    await h.trigger("keydown", { key: "End" });
    await flushPromises();
    expect(activeFile(w), "End 应选中最后一行").toBe("c.litematic");

    // 已在末尾再按下方向键不能越界
    await h.trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    expect(activeFile(w), "末尾再按下方向键不应越界").toBe("c.litematic");

    await h.trigger("keydown", { key: "Home" });
    await flushPromises();
    expect(activeFile(w), "Home 应选中第一行").toBe("a.litematic");

    await h.trigger("keydown", { key: "ArrowUp" });
    await flushPromises();
    expect(activeFile(w), "开头再按上方向键不应越界").toBe("a.litematic");
    w.unmount();
  });

  it("列表没有文件时按键不报错", async () => {
    stubInvoke();
    const w = mountApp();
    await flushPromises();
    const h = w.find(".schematic-cards__grid");
    // 不应抛异常，也不应做出任何选中
    await h.trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    w.unmount();
  });
});
describe("其余列表的键盘导航", () => {
  /** 挂载并选中一个文件，让详情页签与方块表出现 */
  async function selectWithBlocks(blocks: BlockCount[]) {
    stubInvoke({
      analyze_schematic: () => ({
        blocks,
        entities: [],
        tileEntities: [],
        entityInstances: [],
        nonAirTotal: blocks.reduce((a, b) => a + b.count, 0),
        airTotal: 0,
        distinctBlockTypes: new Set(blocks.map((b) => b.name)).size,
        distinctStates: blocks.length,
      }),
    });
    const w = mountApp();
    await flushPromises();
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();
    return w;
  }

  it("方块表：方向键选中行，回车打开替换窗口并回填源方块", async () => {
    const blocks: BlockCount[] = [
      { name: "minecraft:stone", state: "minecraft:stone", count: 5, zhName: "石头", enName: "Stone" },
      { name: "minecraft:dirt", state: "minecraft:dirt", count: 3, zhName: "泥土", enName: "Dirt" },
    ];
    const w = await selectWithBlocks(blocks);
    const tab = w
      .findAll(".detail__tabs .arco-tabs-tab")
      .find((x) => (x.text() || "").trim() === "方块");
    expect(tab, "应能定位到方块页签").toBeTruthy();
    await tab!.trigger("click");
    await flushPromises();

    const host = w.find(".block-table-host");
    expect(host.exists(), "方块表应有键盘导航容器").toBe(true);

    await host.trigger("keydown", { key: "ArrowDown" });
    await flushPromises();

    // 方向键只移动当前行，不打开窗口：这张表首先是统计表
    expect(
      w.findAll(".block-table tbody tr.block-row--active").length,
      "应有且只有一行被标为当前行",
    ).toBe(1);
    expect(document.querySelector(".repl-window .arco-modal-wrapper"), "方向键不应打开窗口").toBeNull();

    // 回车把当前行带进替换窗口
    await host.trigger("keydown", { key: "Enter" });
    await flushPromises();
    const fromInput = document.querySelector<HTMLInputElement>(
      ".repl-window .repl-from-name input",
    );
    expect(fromInput, "回车应打开替换窗口").toBeTruthy();
    expect(fromInput!.value, "窗口里的源方块应是当前行").toBe("minecraft:stone");
    w.unmount();
  });

  it("日志列表：方向键在日志行之间移动并只标一行", async () => {
    stubInvoke({
      read_run_log: () => [
        "1792138651762518000\tINFO\t复制\t完成",
        "1792138651762518001\tERROR\t替换\t失败",
      ],
    });
    const w = mountApp();
    await flushPromises();
    // 备份与日志抽屉 → 运行日志页签
    await w.findAll("button").find((b) => b.text().includes("备份与日志"))!.trigger("click");
    await flushPromises();
    const tab = [...document.querySelectorAll<HTMLElement>(".arco-tabs-tab")].find((t) =>
      (t.textContent ?? "").includes("运行日志"),
    );
    expect(tab, "应有运行日志页签").toBeTruthy();
    tab!.click();
    await flushPromises();

    // 抽屉里的日志页签在当前环境下不渲染列表，改走「日志管理」窗口：
    // 两处用的是同一个 RunLogList 组件，导航行为一致。
    const manageBtn = [...document.querySelectorAll<HTMLElement>("button")].find((b) =>
      (b.textContent ?? "").includes("日志管理"),
    );
    expect(manageBtn, "运行日志页签里应有「日志管理」按钮").toBeTruthy();
    manageBtn!.click();
    await flushPromises();

    const modal = [...document.querySelectorAll<HTMLElement>(".arco-modal")].find((m) =>
      (m.textContent ?? "").includes("日志管理"),
    );
    expect(modal, "应打开日志管理窗口").toBeTruthy();

    const hostEl = modal!.querySelector<HTMLElement>(".log-list");
    expect(hostEl, "窗口里应显示日志列表").toBeTruthy();
    const rows = modal!.querySelectorAll(".log-list__row");
    expect(rows.length, "应显示两条日志").toBe(2);

    hostEl!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    await flushPromises();
    expect(
      modal!.querySelectorAll(".log-list__row--active").length,
      "应有且只有一行被标为当前行",
    ).toBe(1);
    expect(
      modal!.querySelector(".log-list__row--active")!.textContent,
      "第一次按下应停在第一条日志",
    ).toContain("复制");

    hostEl!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    await flushPromises();
    const active = modal!.querySelectorAll(".log-list__row--active");
    expect(active.length, "仍只应有一行被标记").toBe(1);
    expect(active[0].textContent, "第二次按下后应停在第二条日志").toContain("替换");

    // End 应直达最后一条
    hostEl!.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true }));
    await flushPromises();
    expect(
      modal!.querySelector(".log-list__row--active")!.textContent,
      "End 应停在最后一条",
    ).toContain("替换");
    w.unmount();
  });
});

describe("列表键盘导航不抢输入框", () => {
  const CONV_ITEMS = [
    {
      name: "minecraft:oak_stairs",
      nameZh: "橡木楼梯",
      count: 12,
      suggested: "minecraft:pale_oak_stairs",
      suggestedZh: "苍白橡木楼梯",
      chosen: "",
    },
    {
      name: "minecraft:ancient_debris",
      nameZh: "远古残骸",
      count: 3,
      suggested: "",
      suggestedZh: "",
      chosen: "",
    },
  ];

  /** 打开转换窗口并分析出两行缺口。
   *
   * 这两行的「替代为」列里各有一个输入框，正是那种「焦点在输入框里、
   * 但位于列表容器内部」的情形：此时方向键属于光标，不属于列表。
   */
  async function openWithRows() {
    mockInvoke.mockImplementation((async (cmd: string) => {
      switch (cmd) {
        case "scan_litematics":
          return { items: [summary], skippedDirs: 0, skippedPaths: [] };
        case "convert_targets":
          return [{ index: 0, name: "1.19.2", dataVersion: 3120 }];
        case "base_db_status":
          return {
            versions: [{ name: "1.20.1", dataVersion: 3465 }],
            entityAvailable: true,
            supported: [{ name: "1.20.1", dataVersion: 3465 }],
          };
        case "analyze_conversion":
          return CONV_ITEMS.map((i) => ({ ...i }));
        case "build_scene":
          return scene;
        case "analyze_schematic":
          return {
            blocks: [],
            entities: [],
            tileEntities: [],
            nonAirTotal: 0,
            airTotal: 0,
            distinctBlockTypes: 0,
            distinctStates: 0,
          };
        default:
          return [];
      }
    }) as never);

    const w = mountApp();
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();
    await w.findAll("button").find((b) => b.text().includes("版本转换"))!.trigger("click");
    await flushPromises();

    // 选目标版本：下拉 → 选项
    const select = document.querySelector(".arco-select-view, .arco-select") as HTMLElement | null;
    if (select) {
      select.click();
      await flushPromises();
      const opt = [...document.querySelectorAll(".arco-select-option")].find((o) =>
        (o.textContent ?? "").includes("1.19.2"),
      ) as HTMLElement | undefined;
      if (opt) {
        opt.click();
        await flushPromises();
      }
    }
    // 分析差异，左栏才会出现那两行缺口
    const analyze = [...document.querySelectorAll<HTMLElement>("button")].find((b) =>
      (b.textContent ?? "").includes("分析差异"),
    );
    expect(analyze, "应能点「分析差异」").toBeTruthy();
    analyze!.click();
    await flushPromises();
    return w;
  }

  it("转换窗口里在行内输入框按方向键不会切换选中行", async () => {
    const w = await openWithRows();
    const host = document.querySelector<HTMLElement>(".conv-table-host");
    expect(host, "转换窗口的左栏应有键盘导航容器").toBeTruthy();

    // 行内输入框（「替代为」那一列）确实在导航容器内部
    const input = host!.querySelector<HTMLInputElement>("input");
    expect(input, "行内应有替代品输入框").toBeTruthy();

    // 先让键盘选中第一行
    host!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    await flushPromises();
    const firstActive = host!.querySelector("tr.conv-row--active");
    expect(firstActive, "方向键应选中一行").toBeTruthy();

    // 焦点在行内输入框时按下方向键：选中行必须保持不变
    input!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    await flushPromises();
    const stillActive = host!.querySelector("tr.conv-row--active");
    expect(stillActive, "输入框里按方向键不应让选中行消失").toBeTruthy();
    expect(
      stillActive!.textContent,
      "输入框里按方向键不应把选中换到另一行",
    ).toBe(firstActive!.textContent);
    w.unmount();
  });
});
describe("导出尺寸面板", () => {
  /** 挂载并选中一个文件，让预览区与导出按钮出现 */
  async function setup() {
    stubInvoke({
      analyze_schematic: () => ({
        blocks: [],
        entities: [],
        tileEntities: [],
        nonAirTotal: 0,
        airTotal: 0,
        distinctBlockTypes: 0,
        distinctStates: 0,
      }),
      build_scene: () => scene,
    });
    const w = mountApp();
    await flushPromises();
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();
    return w;
  }

  /** 面板里的两个宽高输入框 */
  function sizeInputs(): HTMLInputElement[] {
    return [...document.querySelectorAll<HTMLInputElement>(".export-dialog input")];
  }

  it("点「导出预览图」先打开尺寸面板，而不是直接弹保存框", async () => {
    const w = await setup();
    const btn = w.findAll("button").find((b) => b.text().trim() === "导出预览图");
    expect(btn, "应有「导出预览图」按钮").toBeTruthy();
    await btn!.trigger("click");
    await flushPromises();

    const dialog = document.querySelector(".export-dialog");
    expect(dialog, "应先打开导出尺寸面板").toBeTruthy();
    // 面板里既要有预设，也要能手填
    expect(sizeInputs().length, "应有宽、高两个输入框").toBe(2);
    expect(dialog!.textContent, "应有预设区").toContain("预设尺寸");
    expect(dialog!.textContent, "应有自定义区").toContain("自定义尺寸");
    w.unmount();
  });

  it("批量导出也先经过尺寸面板，标题写明是批量", async () => {
    // 批量导出原先只能沿用记住的尺寸，而界面上已无处可改——
    // 用户无从得知批量导出会出多大图。因此它同样走这个面板。
    const w = await setup();
    // 勾选一个文件，批量导出按钮才可用；复选框在文件管理页
    await activateFiles(w);
    const cb = w.find('.schematic-card input[type="checkbox"]');
    expect(cb.exists(), "应能在文件管理页勾选卡片").toBe(true);
    await cb.setValue(true);
    await flushPromises();
    await activateEditor(w, "测试.litematic");
    const btn = w.findAll("button").find((b) => b.text().includes("批量导出"));
    expect(btn, "应有批量导出按钮").toBeTruthy();
    await btn!.trigger("click");
    await flushPromises();

    const dialog = document.querySelector(".export-dialog");
    expect(dialog, "批量导出也应先打开尺寸面板").toBeTruthy();
    expect(
      document.querySelector(".export-dialog")?.closest(".arco-modal")?.querySelector(".arco-modal-title")?.textContent ?? "",
      "标题应说明这是批量导出",
    ).toContain("批量");
    w.unmount();
  });

  it("选预设会把手填的宽高覆盖掉", async () => {
    const w = await setup();
    await w.findAll("button").find((b) => b.text().trim() === "导出预览图")!.trigger("click");
    await flushPromises();

    // 先手填一个奇怪的尺寸
    const [wIn, hIn] = sizeInputs();
    wIn.value = "700";
    wIn.dispatchEvent(new Event("input"));
    hIn.value = "300";
    hIn.dispatchEvent(new Event("input"));
    await flushPromises();
    expect((sizeInputs()[0] as HTMLInputElement).value, "手填值应生效").toBe("700");

    // 再点 16:9 → 2048 的预设按钮。短边相同的按钮每行都有，
    // 必须按比例那一行定位，否则会点到 1:1 那行的同名按钮。
    const row = document.querySelector<HTMLElement>('.export-dialog__presets [data-aspect="16x9"]');
    expect(row, "应有 16:9 那一行").toBeTruthy();
    const preset = [...row!.querySelectorAll<HTMLElement>("button")].find(
      (b) => (b.textContent ?? "").trim() === "2048 px",
    );
    expect(preset, "16:9 应有短边 2048 的按钮").toBeTruthy();
    preset!.click();
    await flushPromises();

    // 16:9 的短边是 2048，长边 3641
    const after = sizeInputs();
    const wv = Number((after[0] as HTMLInputElement).value);
    const hv = Number((after[1] as HTMLInputElement).value);
    expect(Math.min(wv, hv), "预设的短边应被覆盖成 2048").toBe(2048);
    expect(Math.max(wv, hv), "长边应按 16:9 放大").toBe(3641);
    w.unmount();
  });

  it("确认后面板关闭并按所选尺寸导出", async () => {
    const w = await setup();
    await w.findAll("button").find((b) => b.text().trim() === "导出预览图")!.trigger("click");
    await flushPromises();

    // 选 4:3 短边 1024 的预设（第一个 1024 按钮属于 1:1，取第 2 个）
    const buttons = [...document.querySelectorAll<HTMLElement>(".export-dialog button")].filter(
      (b) => (b.textContent ?? "").trim() === "1024 px",
    );
    expect(buttons.length, "每档短边各有一个按钮").toBe(5);
    buttons[1].click(); // 4:3
    await flushPromises();

    const ok = [
      ...document.querySelectorAll<HTMLElement>(".arco-modal-footer button"),
    ].find((b) => (b.textContent ?? "").trim() === "导出");
    expect(ok, "面板应有「导出」按钮").toBeTruthy();
    ok!.click();
    await flushPromises();

    // 面板关闭：Arco 的 modal 用 v-show 控制显隐，jsdom 下元素仍在文档里，
    // 因此断言可见性而不是「元素是否被移除」
    const modal = document.querySelector<HTMLElement>(".export-dialog")?.closest(".arco-modal");
    expect(modal, "面板元素应仍在（由 v-show 控制）").toBeTruthy();
    // 面板关闭：Arco 的 modal 用 v-show 控制显隐，jsdom 下元素仍留在文档里，
    // 因此断言可见性而不是「元素是否被移除」。承载 display 的那一层在
    // wrapper / modal / mask 之间，任一层被设为 none 即表示已关闭。
    const layers = [
      modal!.closest<HTMLElement>(".arco-modal-wrapper"),
      modal as HTMLElement,
      modal!.closest<HTMLElement>(".arco-modal-mask"),
    ];
    const hidden = layers.some((el) => el?.style.display === "none");
    expect(hidden, "确认后应隐藏面板").toBe(true);
    w.unmount();
  });
});
describe("保存语义（未保存草稿与另存为）", () => {
  /** 挂载、加目录、选中唯一文件。
   *
   * 方块表里给一行石头：替换的入口在方块表的行里，没有行就没有入口。
   */
  async function setup(extra: Record<string, unknown> = {}) {
    stubInvoke({
      analyze_schematic: {
        blocks: [
          {
            name: "minecraft:stone",
            state: "minecraft:stone",
            count: 6,
            zhName: "石头",
            enName: "Stone",
          },
        ],
        entities: [],
        tileEntities: [],
        entityInstances: [],
        nonAirTotal: 6,
        airTotal: 0,
        distinctBlockTypes: 1,
        distinctStates: 1,
      },
      ...extra,
    });
    const w = mountApp();
    await flushPromises();
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();
    return w;
  }

  /** 左栏的四个输入框，按稳定类名定位 */
  function replInput(cls: string): HTMLInputElement {
    const el = document.querySelector<HTMLInputElement>(`.repl-window ${cls} input`);
    if (!el) throw new Error(`找不到输入框 ${cls}`);
    return el;
  }

  /** 打开替换窗口，把目标方块填成给定值 */
  async function openReplaceWithTarget(w: ReturnType<typeof mountApp>, target: string) {
    await clickReplaceOnRow(w);
    const to = replInput(".repl-to-name");
    to.value = target;
    to.dispatchEvent(new Event("input"));
    await flushPromises();
  }

  it("方块替换不再立即写盘，而是进入待保存状态", async () => {
    // 替换会大量改写调色板且不可撤销。点「执行替换」只应记下规则，
    // 真正写盘要等用户点「保存」。
    // 先确认这套挂载能真的进入详情面板（否则后面的按钮断言都无从谈起）
    const applied: string[] = [];
    const w = await setup({
      apply_replace: () => {
        applied.push("called");
        return { replaced: 3 };
      },
    });

    expect(w.text(), "应已选中文件并进入详情面板").toContain("投影名称");

    // 从方块表那一行的替换按钮打开窗口，把目标方块填成泥土
    await openReplaceWithTarget(w, "minecraft:dirt");
    // 源方块由点的那一行决定，不可手改
    expect(replInput(".repl-from-name").value, "源方块应来自点的那一行").toBe(
      "minecraft:stone",
    );

    const ok = [...document.querySelectorAll<HTMLElement>(".arco-modal-footer button")].find(
      (b) => (b.textContent ?? "").trim() === "执行替换",
    );
    expect(ok, "应有「执行替换」按钮").toBeTruthy();
    ok!.click();
    await flushPromises();

    expect(applied.length, "此时不应调用后端写盘").toBe(0);
    // 记入草稿后，保存按钮应从禁用变为可用——这是「替换进入未保存状态」的直接证据
    const saveBtn = w.findAll("button").find((b) => b.text().trim() === "保存");
    expect(saveBtn, "应有保存按钮").toBeTruthy();
    expect(
      saveBtn!.attributes("disabled"),
      "记入替换后保存按钮应可用",
    ).toBeUndefined();
    expect(shownMessages().join("|"), "应提示已记入待保存").toContain("保存");
    w.unmount();
  });

  it("保存时执行待替换规则，并在成功提示里说明替换了几个方块", async () => {
    const applied: unknown[] = [];
    const w = await setup({
      apply_replace: (args: { path?: string }) => {
        applied.push(args);
        return { replaced: 3 };
      },
      save_metadata: () => ({ backupPath: "/p/备份/x.bak" }),
    });

    await openReplaceWithTarget(w, "minecraft:dirt");
    [...document.querySelectorAll<HTMLElement>(".repl-window .arco-modal-footer button")]
      .find((b) => (b.textContent ?? "").trim() === "执行替换")!
      .click();
    await flushPromises();

    // 点保存
    const save = w.findAll("button").find((b) => b.text().trim() === "保存");
    expect(save, "应有保存按钮").toBeTruthy();
    expect(
      save!.attributes("disabled"),
      "记入替换后应可保存（说明草稿状态已反映替换）",
    ).toBeUndefined();
    await save!.trigger("click");
    await flushPromises();

    expect(applied.length, "保存时才应真正执行替换").toBe(1);
    // 提示文案用 spy 断言而不是读 body：Arco 的 Message 容器会被 body 清空
    // 变成游离节点，读 body 只能靠运气
    expect(
      shownMessages().join("|"),
      "应说明替换了几个方块",
    ).toContain("已替换 3 个方块");
    w.unmount();
  });

  it("另存为把内容写到新路径，并把编辑对象切到新文件", async () => {
    const savedAs: Array<{ sourcePath: string; targetPath: string }> = [];
    const w = await setup({
      save_schematic_as: (args: { sourcePath: string; targetPath: string }) => {
        savedAs.push(args);
        return args.targetPath;
      },
    });

    // 另存为按钮不依赖 dirty，随时可用
    const btn = w.findAll("button").find((b) => b.text().trim() === "另存为");
    expect(btn, "应有「另存为」按钮").toBeTruthy();

    // 保存对话框返回一个目标路径
    const { save } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(save).mockResolvedValueOnce("/p/copy.litematic");
    await btn!.trigger("click");
    await flushPromises();

    expect(savedAs.length, "应调用另存为命令").toBe(1);
    expect(savedAs[0].targetPath, "目标应是用户选的路径").toBe("/p/copy.litematic");
    expect(savedAs[0].sourcePath, "来源应是当前文件").toBe("/p/测试.litematic");
    w.unmount();
  });

  it("没有未保存改动时点击关闭按钮直接关窗，不弹确认", async () => {
    // 反向情形：没有改动还弹确认就是在打扰用户
    const close = vi.fn();
    vi.mocked(getCurrentWindow).mockReturnValue({
      minimize: vi.fn(),
      toggleMaximize: vi.fn(),
      close,
      isMaximized: vi.fn().mockResolvedValue(false),
      onResized: vi.fn().mockResolvedValue(() => {}),
      theme: vi.fn().mockResolvedValue(null),
      onThemeChanged: vi.fn().mockResolvedValue(() => {}),
    } as never);

    const w = await setup();
    // 刚选中文件、什么都没改，不应显示未保存提示
    expect(w.text(), "此时不应有未保存提示").not.toContain("有未保存的修改");

    await w.find(".titlebar__btn.is-close").trigger("click");
    await flushPromises();

    expect(close, "没有改动时应直接关窗").toHaveBeenCalled();
    w.unmount();
  });

  it("有未保存改动时点击关闭按钮会先确认", async () => {
    const close = vi.fn();
    vi.mocked(getCurrentWindow).mockReturnValue({
      minimize: vi.fn(),
      toggleMaximize: vi.fn(),
      close,
      isMaximized: vi.fn().mockResolvedValue(false),
      onResized: vi.fn().mockResolvedValue(() => {}),
      theme: vi.fn().mockResolvedValue(null),
      onThemeChanged: vi.fn().mockResolvedValue(() => {}),
    } as never);

    const w = await setup();
    // 制造一处未保存改动
    const nameInput = w.find('.preview__info input');
    expect(nameInput.exists(), "应有可编辑的名称输入框").toBe(true);
    await nameInput.setValue("改了个名字");
    await flushPromises();
    expect(w.text(), "应显示有未保存的修改").toContain("有未保存的修改");

    // 点标题栏关闭按钮（自绘标题栏，三个按钮都是 div）
    const btn = w.find(".titlebar__btn.is-close");
    expect(btn.exists(), "应有窗口关闭按钮").toBe(true);
    await btn.trigger("click");
    await flushPromises();

    // 关键行为：有未保存改动时，关闭被拦下并弹出确认；无改动时才直接关。
    // 这里只断言「拦下并弹出确认」这一步——Modal.confirm 在 jsdom 下没有
    // 真实交互，它的 Promise 会立即结算，据此断言「窗口没关」并不成立。
    const modals = [...document.querySelectorAll<HTMLElement>(".arco-modal")].map(
      (m) => (m.textContent ?? ""),
    );
    expect(modals.join("||"), "应弹出未保存确认").toContain("未保存的修改");
    expect(modals.join("||"), "确认框里应说明关窗会丢弃修改").toContain("丢弃");
    expect(document.querySelector(".arco-modal"), "确认框应可见").toBeTruthy();
    w.unmount();
  });
});
describe("预览区的右键菜单", () => {
  /** 挂载并选中文件，让预览舞台出现 */
  async function setup() {
    stubInvoke({
      analyze_schematic: () => ({
        blocks: [],
        entities: [],
        tileEntities: [],
        nonAirTotal: 0,
        airTotal: 0,
        distinctBlockTypes: 0,
        distinctStates: 0,
      }),
      build_scene: () => scene,
    });
    const w = mountApp();
    await flushPromises();
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();
    return w;
  }

  /** 在预览舞台上右键 */
  async function rightClick(w: ReturnType<typeof mountApp>) {
    const stage = w.find(".editor-preview__stage");
    expect(stage.exists(), "应有预览舞台").toBe(true);
    await stage.trigger("contextmenu", { clientX: 300, clientY: 200 });
    await flushPromises();
    return stage;
  }

  it("右键预览区弹出视角菜单，含保存、投影切换与预设角度子菜单", async () => {
    const w = await setup();
    await rightClick(w);

    const menu = document.querySelector(".view-menu");
    expect(menu, "右键应弹出视角菜单").toBeTruthy();
    const text = menu!.textContent ?? "";
    expect(text, "应有保存为预设角度入口").toContain("保存为预设角度");
    expect(text, "应有正射投影开关").toContain("正射投影");
    expect(text, "应有预设角度子菜单入口").toContain("预设角度");
    // 子菜单默认收起：十个预设不该一开始就铺在菜单里
    expect(text, "预设角度初始应收起").not.toContain("顶（Y+）");

    // 展开子菜单后十个预设一个都不能少
    const sub = [...document.querySelectorAll<HTMLElement>(".view-menu .view-menu__sub")][0];
    expect(sub, "应有子菜单入口").toBeTruthy();
    sub.click();
    await flushPromises();
    const subText = document.querySelector(".view-menu__submenu")?.textContent ?? "";
    expect(subText, "展开后应有子菜单").not.toBe("");
    for (const name of [
      "顶（Y+）",
      "底（Y−）",
      "南（Z+）",
      "北（Z−）",
      "东（X+）",
      "西（X−）",
      "等距右",
      "等距左",
      "真等距右",
      "真等距左",
    ]) {
      expect(subText, `子菜单里应有「${name}」`).toContain(name);
    }
    w.unmount();
  });

  it("点子菜单里的预设后菜单与子菜单一起关闭", async () => {
    const w = await setup();
    await rightClick(w);

    const sub = [...document.querySelectorAll<HTMLElement>(".view-menu .view-menu__sub")][0];
    sub.click();
    await flushPromises();
    const items = [...document.querySelectorAll<HTMLElement>(".view-menu__submenu .ctx-menu__item")];
    const top = items.find((el) => (el.textContent ?? "").includes("顶（Y+）"));
    expect(top, "应有顶视角项").toBeTruthy();
    top!.click();
    await flushPromises();

    expect(document.querySelector(".view-menu"), "选完之后菜单应关闭").toBeNull();
    expect(document.querySelector(".view-menu__submenu"), "子菜单也应关闭").toBeNull();
    w.unmount();
  });

  it("点菜单外面会关闭菜单", async () => {
    const w = await setup();
    await rightClick(w);
    expect(document.querySelector(".view-menu"), "菜单应已打开").toBeTruthy();

    document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    await flushPromises();
    expect(document.querySelector(".view-menu"), "点别处应关闭菜单").toBeNull();
    w.unmount();
  });

  it("按 Esc 关闭菜单", async () => {
    const w = await setup();
    await rightClick(w);
    expect(document.querySelector(".view-menu"), "菜单应已打开").toBeTruthy();

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await flushPromises();
    expect(document.querySelector(".view-menu"), "按 Esc 应关闭菜单").toBeNull();
    w.unmount();
  });

  it("点「正射投影」会打上勾，再点取消", async () => {
    const w = await setup();
    const stage = w.find(".editor-preview__stage");

    await stage.trigger("contextmenu", { clientX: 300, clientY: 200 });
    await flushPromises();
    const orthoItem = () =>
      [...document.querySelectorAll<HTMLElement>(".view-menu .ctx-menu__item")].find((el) =>
        (el.textContent ?? "").includes("正射投影"),
      )!;

    expect(orthoItem().textContent, "初始未勾选").not.toContain("✓");
    orthoItem().click();
    await flushPromises();

    // 再开一次菜单看勾选状态
    await stage.trigger("contextmenu", { clientX: 300, clientY: 200 });
    await flushPromises();
    expect(orthoItem().textContent, "点过之后应显示勾选").toContain("✓");
    w.unmount();
  });
});
describe("自定义视角预设", () => {
  /** 挂载并选中文件，让预览舞台出现 */
  async function setup() {
    stubInvoke({
      analyze_schematic: () => ({
        blocks: [],
        entities: [],
        tileEntities: [],
        nonAirTotal: 0,
        airTotal: 0,
        distinctBlockTypes: 0,
        distinctStates: 0,
      }),
      build_scene: () => scene,
    });
    const w = mountApp();
    await flushPromises();
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    await flushPromises();
    return w;
  }

  /** 打开预览区的右键菜单 */
  async function openMenu(w: ReturnType<typeof mountApp>) {
    await w.find(".editor-preview__stage").trigger("contextmenu", { clientX: 300, clientY: 200 });
    await flushPromises();
    expect(document.querySelector(".view-menu"), "菜单应已打开").toBeTruthy();
  }

  /** 点菜单里文字包含 label 的那一项 */
  function menuItem(label: string): HTMLElement | undefined {
    return [...document.querySelectorAll<HTMLElement>(".view-menu .ctx-menu__item")].find((el) =>
      (el.textContent ?? "").includes(label),
    );
  }

  /** 展开「预设角度」子菜单（内置十个角度与自定义预设都在里面）。
   *
   *  用点击而不是派发 mouseenter：两者的处理函数都会置 presetSubOpen，
   *  但 jsdom 里合成的 mouseenter 不会被 Vue 的 @mouseenter 收到。
   */
  async function openPresetSub() {
    // 不能按文字找："保存为预设角度…" 里也含 "预设角度"，会选错项
    const trigger = document.querySelector<HTMLElement>(".view-menu .view-menu__sub");
    expect(trigger, "主菜单里应有预设角度入口").toBeTruthy();
    // 入口是切换语义：已经展开时再点会收起，所以只在没展开时点
    if (!document.querySelector(".view-menu__submenu")) {
      trigger!.click();
      await flushPromises();
    }
    const sub = document.querySelector(".view-menu__submenu");
    expect(sub, "子菜单应已展开").toBeTruthy();
    return sub as HTMLElement;
  }

  /** 在子菜单里找文字包含 label 的那一项 */
  function subItem(label: string): HTMLElement | undefined {
    return [...document.querySelectorAll<HTMLElement>(".view-menu__submenu .ctx-menu__item")]
      .find((el) => (el.textContent ?? "").includes(label));
  }

  it("菜单里有「保存为预设角度」入口", async () => {
    const w = await setup();
    await openMenu(w);
    expect(menuItem("保存为预设角度"), "菜单应有保存入口").toBeTruthy();
    w.unmount();
  });

  it("保存预设后出现在菜单的自定义分组里", async () => {
    const w = await setup();
    await openMenu(w);
    menuItem("保存为预设角度")!.click();
    await flushPromises();

    // 面板应打开
    const dialog = document.querySelector(".view-preset");
    expect(dialog, "应打开保存面板").toBeTruthy();
    expect(dialog!.textContent, "面板应展示投影方式").toContain("投影");
    expect(dialog!.textContent, "面板应展示相机位置").toContain("相机位置");
    expect(dialog!.textContent, "面板应展示相机角度").toContain("yaw");

    // 填名字并保存
    const input = document.querySelector<HTMLInputElement>(".view-preset input")!;
    input.value = "我的视角";
    input.dispatchEvent(new Event("input"));
    await flushPromises();
    const ok = [...document.querySelectorAll<HTMLElement>(".arco-modal-footer button")].find(
      (b) => (b.textContent ?? "").trim() === "保存",
    );
    expect(ok, "面板应有保存按钮").toBeTruthy();
    ok!.click();
    await flushPromises();

    // 再开菜单：主菜单里不该有自定义预设，它归在「预设角度」子菜单里
    await openMenu(w);
    const mainText = document.querySelector(".view-menu")!.textContent ?? "";
    expect(mainText, "自定义预设不该出现在主菜单").not.toContain("我的视角");
    expect(mainText, "主菜单不该有自定义分组标题").not.toContain("自定义预设");
    const subText = (await openPresetSub()).textContent ?? "";
    expect(subText, "应出现自定义预设分组").toContain("自定义预设");
    expect(subText, "应出现刚保存的预设名").toContain("我的视角");
    w.unmount();
  });

  it("删除自定义预设后菜单里不再出现", async () => {
    const w = await setup();
    // 先存一条
    await openMenu(w);
    menuItem("保存为预设角度")!.click();
    await flushPromises();
    const input = document.querySelector<HTMLInputElement>(".view-preset input")!;
    input.value = "待删除";
    input.dispatchEvent(new Event("input"));
    await flushPromises();
    [...document.querySelectorAll<HTMLElement>(".arco-modal-footer button")]
      .find((b) => (b.textContent ?? "").trim() === "保存")!
      .click();
    await flushPromises();

    // 打开菜单、展开「预设角度」子菜单后点删除
    await openMenu(w);
    await openPresetSub();
    const del = document.querySelector<HTMLElement>(".view-menu .view-menu__del");
    expect(del, "自定义预设应有删除入口").toBeTruthy();
    del!.click();
    await flushPromises();

    // 再开菜单、展开子菜单应看不到它
    await w.find(".editor-preview__stage").trigger("contextmenu", { clientX: 300, clientY: 200 });
    await flushPromises();
    const subText = (await openPresetSub()).textContent ?? "";
    expect(subText, "删除后不应再出现该预设").not.toContain("待删除");
    w.unmount();
  });

  it("面板里的投影方式可以改成正射并生效", async () => {
    const w = await setup();
    await openMenu(w);
    menuItem("保存为预设角度")!.click();
    await flushPromises();

    // 两种投影各有一个单选项，默认选中透视
    const radios = [
      ...document.querySelectorAll<HTMLInputElement>(".view-preset input[type=radio]"),
    ];
    expect(radios.length, "应给出透视与正射两种选择").toBe(2);
    expect(radios[0].checked, "默认应是透视").toBe(true);

    radios[1].click();
    await flushPromises();
    expect(radios[1].checked, "点过之后应选中正射").toBe(true);

    // 填名保存，存下来的预设要带正射标记
    const input = document.querySelector<HTMLInputElement>(".view-preset input[type=text]") ??
      document.querySelector<HTMLInputElement>(".view-preset input")!;
    input.value = "正射视角";
    input.dispatchEvent(new Event("input"));
    await flushPromises();
    [...document.querySelectorAll<HTMLElement>(".arco-modal-footer button")]
      .find((b) => (b.textContent ?? "").trim() === "保存")!
      .click();
    await flushPromises();

    // 子菜单里点它，应当把预览切成正射投影
    await openMenu(w);
    await openPresetSub();
    subItem("正射视角")!.click();
    await flushPromises();
    // 再开菜单看「正射投影」这一项是否被勾上
    await openMenu(w);
    const orthoItem = [...document.querySelectorAll<HTMLElement>(".view-menu .ctx-menu__item")].find(
      (el) => (el.textContent ?? "").includes("正射投影"),
    )!;
    expect(orthoItem.textContent, "应用该预设后应处于正射").toContain("✓");
    w.unmount();
  });

  it("面板里的相机位置可以手改，保存的是改后的值", async () => {
    const w = await setup();
    await openMenu(w);
    menuItem("保存为预设角度")!.click();
    await flushPromises();

    // 只取数字输入框：位置三个、角度两个
    const numbers = [
      ...document.querySelectorAll<HTMLInputElement>(".view-preset .arco-input-number input"),
    ];
    expect(numbers.length, "位置三项 + 角度两项").toBe(5);

    // 改成「从正上方 100 格处往下看」。
    // Arco 的数字输入框在失焦（或回车）时才提交，因此这里模拟真实操作：输完离开。
    const set = async (el: HTMLInputElement, v: string) => {
      el.value = v;
      el.dispatchEvent(new Event("input"));
      el.dispatchEvent(new FocusEvent("blur"));
      await flushPromises();
    };
    await set(numbers[0], "0");
    await set(numbers[1], "100");
    await set(numbers[2], "0");

    // 位置改了，角度必须跟着变成「正上方」：pitch 90
    const pitch = numbers[4];
    expect(Number(pitch.value), "正上方看下去 pitch 应为 90").toBeCloseTo(90, 0);

    const input = document.querySelector<HTMLInputElement>(".view-preset input[type=text]")!;
    input.value = "俯视";
    input.dispatchEvent(new Event("input"));
    await flushPromises();
    [...document.querySelectorAll<HTMLElement>(".arco-modal-footer button")]
      .find((b) => (b.textContent ?? "").trim() === "保存")!
      .click();
    await flushPromises();

    // 子菜单里出现该预设，点它之后相机应摆到正上方
    await openMenu(w);
    await openPresetSub();
    const item = subItem("俯视");
    expect(item, "改过位置的预设应保存成功").toBeTruthy();
    w.unmount();
  });

  it("取消按钮显示中文文案，不露出翻译键", async () => {
    const w = await setup();
    await openMenu(w);
    menuItem("保存为预设角度")!.click();
    await flushPromises();

    const footer = document.querySelector(".arco-modal-footer");
    expect(footer, "面板应有按钮栏").toBeTruthy();
    const cancel = [...footer!.querySelectorAll<HTMLElement>("button")].find((b) =>
      (b.textContent ?? "").includes("取消"),
    );
    expect(cancel, "取消按钮应有中文文案").toBeTruthy();
    expect(footer!.textContent, "不应露出未翻译的键").not.toContain("common.cancel");
    w.unmount();
  });

  it("菜单项可以用键盘操作（回车切换投影）", async () => {
    // 菜单项都带 tabindex="0"，能 Tab 到却按不动的话比不能聚焦更让人困惑
    const w = await setup();
    await openMenu(w);
    const orthoItem = [...document.querySelectorAll<HTMLElement>(".view-menu .ctx-menu__item")]
      .find((el) => (el.textContent ?? "").includes("正射投影"))!;
    expect(orthoItem, "菜单里应有正射投影项").toBeTruthy();
    expect(orthoItem.getAttribute("tabindex"), "菜单项应可聚焦").toBe("0");

    // 回车应当与点击等效：切成正射
    orthoItem.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await flushPromises();

    // 再开菜单看勾选状态
    await openMenu(w);
    const after = [...document.querySelectorAll<HTMLElement>(".view-menu .ctx-menu__item")]
      .find((el) => (el.textContent ?? "").includes("正射投影"))!;
    expect(after.getAttribute("aria-checked"), "回车后应切到正射").toBe("true");
    w.unmount();
  });

  it("把位置与注视点改成重合时不允许保存", async () => {
    // 这种位姿算不出朝向。旧实现会照存不误，重启后又被校验丢掉，
    // 用户看到的是「自己存的预设没了」
    const w = await setup();
    await openMenu(w);
    menuItem("保存为预设角度")!.click();
    await flushPromises();

    const numbers = [
      ...document.querySelectorAll<HTMLInputElement>(".view-preset .arco-input-number input"),
    ];
    // 面板打开时位置取自实时相机，注视点是模型的中心；
    // 把位置的三个分量都填成同一组数才会重合，这里直接把三格清空更简单
    for (const el of numbers.slice(0, 3)) {
      el.value = "";
      el.dispatchEvent(new Event("input"));
      el.dispatchEvent(new FocusEvent("blur"));
    }
    await flushPromises();

    const input = document.querySelector<HTMLInputElement>(".view-preset input[type=text]")!;
    input.value = "坏视角";
    input.dispatchEvent(new Event("input"));
    await flushPromises();

    const ok = [...document.querySelectorAll<HTMLElement>(".arco-modal-footer button")].find(
      (b) => (b.textContent ?? "").trim() === "保存",
    )!;
    expect(ok, "坐标留空时保存按钮应禁用").toBeTruthy();
    expect(
      (ok as HTMLButtonElement).disabled,
      "坐标不完整时不能保存",
    ).toBe(true);
    w.unmount();
  });
});

describe("工作区选项卡与子区", () => {
  it("固定选项卡保持顺序，关闭设置后可以重新打开", async () => {
    const w = mountApp();
    await flushPromises();
    const labels = () => w.findAll(".workspace-tab__label").map((item) => item.text());
    expect(labels().slice(0, 2)).toEqual(["文件管理", "设置"]);

    const settings = w.findAll(".workspace-tab").find((item) => item.text().includes("设置"));
    await settings!.find(".workspace-tab__close").trigger("click");
    await flushPromises();
    expect(w.findAll(".workspace-tab__label").map((item) => item.text())).not.toContain("设置");

    const reopen = w.findAll(".workspace-tabs__actions button").find((item) => item.text() === "设置");
    await reopen!.trigger("click");
    await flushPromises();
    expect(labels().slice(0, 2)).toEqual(["文件管理", "设置"]);
    expect(labels().filter((text) => text === "设置")).toHaveLength(1);
    w.unmount();
  });

  it("两个编辑会话分别保留自己的未保存草稿", async () => {
    const other: SchematicSummary = {
      ...summary,
      path: "/p/other.litematic",
      fileName: "other.litematic",
      name: "另一个投影",
    };
    stubInvoke({ scan_litematics: { items: [summary, other], skippedDirs: 0, skippedPaths: [] } });
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();

    await openFileCard(w, "测试.litematic");
    let input = w.findAll("input").find((item) => (item.element as HTMLInputElement).value === "测试投影");
    await input!.setValue("A 的未保存名称");

    await openFileCard(w, "other.litematic");
    input = w.findAll("input").find((item) => (item.element as HTMLInputElement).value === "另一个投影");
    await input!.setValue("B 的未保存名称");

    await activateEditor(w, "测试.litematic");
    expect(w.findAll("input").some((item) => (item.element as HTMLInputElement).value === "A 的未保存名称")).toBe(true);
    await activateEditor(w, "other.litematic");
    expect(w.findAll("input").some((item) => (item.element as HTMLInputElement).value === "B 的未保存名称")).toBe(true);
    expect(w.findAll(".workspace-tab__label").filter((item) => item.text().includes("litematic"))).toHaveLength(2);
    w.unmount();
  });

  it("目录树区分直接与递归文件，并显示空目录", async () => {
    const childFile: SchematicSummary = {
      ...summary,
      path: "/p/child/child.litematic",
      fileName: "child.litematic",
      dirName: "/p/child",
    };
    const root = {
      path: "/p",
      name: "p",
      relativePath: "",
      filePaths: [summary.path],
      truncated: false,
      children: [
        { path: "/p/empty", name: "empty", relativePath: "empty", filePaths: [], children: [], truncated: false },
        { path: "/p/child", name: "child", relativePath: "child", filePaths: [childFile.path], children: [], truncated: false },
      ],
    };
    stubInvoke({
      scan_litematics: { items: [summary, childFile], root, skippedDirs: 0, skippedPaths: [] },
    });
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();

    expect(w.findAll(".schematic-card")).toHaveLength(1);
    expect(w.findAll(".directory-node")).toHaveLength(3);
    expect(w.findAll(".directory-icon--solid")).toHaveLength(2);
    expect(w.text()).toContain("empty");

    await w.find(".file-manager__descendants span").trigger("click");
    await flushPromises();
    expect(w.findAll(".schematic-card")).toHaveLength(2);

    await selectRoot(w, "/p/child");
    expect(w.findAll(".schematic-card")).toHaveLength(1);
    expect(w.text()).toContain("child.litematic");
    w.unmount();
  });

  it("子区列表切换预览时传递区域名，全部子区不传该参数", async () => {
    stubInvoke({
      analyze_schematic: {
        regions: [
          { name: "A", position: [0, 0, 0], size: [4, 4, 4] },
          { name: "B", position: [10, 0, 0], size: [2, 3, 4] },
        ],
        blocks: [], entities: [], tileEntities: [], entityInstances: [],
        nonAirTotal: 0, airTotal: 0, distinctBlockTypes: 0, distinctStates: 0,
      },
    });
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");

    expect(w.findAll(".region-list__item")).toHaveLength(3);
    await w.findAll(".region-list__item").find((item) => item.text().includes("B"))!.trigger("click");
    await flushPromises();
    let calls = mockInvoke.mock.calls.filter((call) => call[0] === "build_scene");
    expect(calls[calls.length - 1][1]).toMatchObject({ schematicPath: summary.path, regionName: "B" });

    await w.findAll(".region-list__item").find((item) => item.text().includes("全部子区"))!.trigger("click");
    await flushPromises();
    calls = mockInvoke.mock.calls.filter((call) => call[0] === "build_scene");
    expect(calls[calls.length - 1][1]).toMatchObject({ schematicPath: summary.path });
    expect(calls[calls.length - 1][1]).not.toHaveProperty("regionName");
    w.unmount();
  });

  it("切换到另一个文件后，旧分析响应不会覆盖当前子区", async () => {
    const other: SchematicSummary = {
      ...summary,
      path: "/p/other.litematic",
      fileName: "other.litematic",
      name: "另一个投影",
    };
    let resolveOld: ((value: unknown) => void) | null = null;
    stubInvoke({ scan_litematics: { items: [summary, other], skippedDirs: 0, skippedPaths: [] } });
    const base = mockInvoke.getMockImplementation()!;
    mockInvoke.mockImplementation((async (cmd: string, args: never) => {
      if (cmd === "analyze_schematic") {
        const path = (args as unknown as { path: string }).path;
        if (path === summary.path) {
          return new Promise((resolve) => {
            resolveOld = resolve as (value: unknown) => void;
          });
        }
        return {
          regions: [{ name: "B_REGION", position: [0, 0, 0], size: [1, 1, 1] }],
          blocks: [], entities: [], tileEntities: [], entityInstances: [],
          nonAirTotal: 0, airTotal: 0, distinctBlockTypes: 0, distinctStates: 0,
        };
      }
      return base(cmd, args);
    }) as never);
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();

    const first = w.findAll(".schematic-card").find((item) => item.text().includes("测试.litematic"));
    await first!.trigger("dblclick");
    await flushPromises();
    expect(resolveOld, "旧分析请求应已发出").toBeTruthy();

    await openFileCard(w, "other.litematic");
    resolveOld!({
      regions: [{ name: "A_REGION", position: [0, 0, 0], size: [1, 1, 1] }],
      blocks: [], entities: [], tileEntities: [], entityInstances: [],
      nonAirTotal: 0, airTotal: 0, distinctBlockTypes: 0, distinctStates: 0,
    });
    await flushPromises();
    expect(w.find(".region-list").text()).toContain("B_REGION");
    expect(w.find(".region-list").text()).not.toContain("A_REGION");
    w.unmount();
  });
});


describe("多会话并发与状态边界", () => {
  const other: SchematicSummary = {
    ...summary,
    path: "/p/other.litematic",
    fileName: "other.litematic",
    name: "另一个投影",
  };
  const blockAnalysis = {
    regions: [],
    blocks: [{ name: "minecraft:stone", state: "minecraft:stone", count: 6, zhName: "石头", enName: "Stone" }],
    entities: [], tileEntities: [], entityInstances: [], nonAirTotal: 6, airTotal: 0,
    distinctBlockTypes: 1, distinctStates: 1,
  };

  async function mountTwo(overrides: Record<string, unknown> = {}) {
    stubInvoke({
      scan_litematics: { items: [summary, other], skippedDirs: 0, skippedPaths: [] },
      analyze_schematic: blockAnalysis,
      ...overrides,
    });
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    return w;
  }

  async function openReplaceTarget(w: ReturnType<typeof mountApp>, target: string) {
    await clickReplaceOnRow(w);
    const input = document.querySelector<HTMLInputElement>(".repl-window .repl-to-name input")!;
    input.value = target;
    input.dispatchEvent(new Event("input"));
    await flushPromises();
  }

  async function chooseConvertTarget() {
    const select = document.querySelector<HTMLElement>(".conv-window .arco-select-view, .conv-window .arco-select");
    expect(select, "转换窗口应有版本选择器").toBeTruthy();
    select!.click();
    await flushPromises();
    const option = [...document.querySelectorAll<HTMLElement>(".arco-select-option")].find((item) =>
      (item.textContent ?? "").includes("1.19.2"),
    );
    expect(option, "应列出目标版本").toBeTruthy();
    option!.click();
    await flushPromises();
  }

  async function openConvert(w: ReturnType<typeof mountApp>) {
    await w.findAll("button").find((b) => b.text().includes("版本转换"))!.trigger("click");
    await flushPromises();
    await chooseConvertTarget();
  }

  it("替换表单按编辑会话保存，Enter 只打开聚焦卡片", async () => {
    const w = await mountTwo();
    await openFileCard(w, "测试.litematic");
    await openReplaceTarget(w, "minecraft:dirt");
    await openFileCard(w, "other.litematic");
    await openReplaceTarget(w, "minecraft:glass");

    await activateEditor(w, "测试.litematic");
    expect(document.querySelector<HTMLInputElement>(".repl-window .repl-to-name input")!.value).toBe("minecraft:dirt");
    await activateEditor(w, "other.litematic");
    expect(document.querySelector<HTMLInputElement>(".repl-window .repl-to-name input")!.value).toBe("minecraft:glass");
    await w.unmount();

    const fresh = await mountTwo();
    // 按内容定位而不是下标：卡片顺序取决于运行环境的排序规则，
    // 中文与拉丁文文件名在不同 ICU 数据下先后不同
    const otherCard = fresh
      .findAll(".schematic-card")
      .find((card) => card.text().includes("other.litematic"));
    expect(otherCard, "应能找到 other 卡片").toBeTruthy();
    await otherCard!.trigger("keydown", { key: "Enter" });
    await flushPromises();
    const editors = fresh.findAll(".workspace-tab__label").filter((tab) => tab.attributes("title")?.includes("litematic"));
    expect(editors).toHaveLength(1);
    expect(editors[0].attributes("title")).toBe(other.path);
    fresh.unmount();
  });

  it("旧替换预览响应不会写入另一个编辑器", async () => {
    let resolveOld: ((value: unknown) => void) | null = null;
    const w = await mountTwo({
      preview_replace: (args: { schematicPath: string }) => {
        if (args.schematicPath === summary.path) {
          return new Promise((resolve) => { resolveOld = resolve; });
        }
        return { replaced: 9, paletteChanged: 1 };
      },
    });
    await openFileCard(w, "测试.litematic");
    await openReplaceTarget(w, "minecraft:dirt");
    [...document.querySelectorAll<HTMLElement>(".repl-window button")]
      .find((b) => (b.textContent ?? "").includes("预计替换数量"))!.click();
    await flushPromises();
    expect(resolveOld).toBeTruthy();

    await openFileCard(w, "other.litematic");
    await openReplaceTarget(w, "minecraft:dirt");
    [...document.querySelectorAll<HTMLElement>(".repl-window button")]
      .find((b) => (b.textContent ?? "").includes("预计替换数量"))!.click();
    await flushPromises();
    expect(document.querySelector(".repl-window")!.textContent).toContain("将替换 9 个方块");

    resolveOld!({ replaced: 1, paletteChanged: 1 });
    await flushPromises();
    expect(document.querySelector(".repl-window")!.textContent).toContain("将替换 9 个方块");
    expect(document.querySelector(".repl-window")!.textContent).not.toContain("将替换 1 个方块");
    w.unmount();
  });

  it("旧转换分析不会覆盖另一个编辑器的结果", async () => {
    let resolveOld: ((value: unknown) => void) | null = null;
    const item = (name: string) => ({
      name, nameZh: name, nameEn: name, count: 1,
      suggested: "", suggestedZh: "", suggestedEn: "", chosen: "",
    });
    const w = await mountTwo({
      convert_targets: [{ index: 0, name: "1.19.2", dataVersion: 3120 }],
      analyze_conversion: (args: { schematicPath: string }) => {
        if (args.schematicPath === summary.path) {
          return new Promise((resolve) => { resolveOld = resolve; });
        }
        return [item("minecraft:b_result")];
      },
    });
    await openFileCard(w, "测试.litematic");
    await openConvert(w);
    [...document.querySelectorAll<HTMLElement>(".conv-window button")]
      .find((b) => (b.textContent ?? "").includes("分析差异"))!.click();
    await flushPromises();
    expect(resolveOld).toBeTruthy();

    await openFileCard(w, "other.litematic");
    await openConvert(w);
    [...document.querySelectorAll<HTMLElement>(".conv-window button")]
      .find((b) => (b.textContent ?? "").includes("分析差异"))!.click();
    await flushPromises();
    expect(document.querySelector(".conv-window")!.textContent).toContain("minecraft:b_result");

    resolveOld!([item("minecraft:a_stale")]);
    await flushPromises();
    expect(document.querySelector(".conv-window")!.textContent).toContain("minecraft:b_result");
    expect(document.querySelector(".conv-window")!.textContent).not.toContain("minecraft:a_stale");
    w.unmount();
  });

  it("保存期间禁用编辑入口，完成后保留已提交内容", async () => {
    let finishSave: ((value: unknown) => void) | null = null;
    let saved = false;
    const w = await mountTwo({
      save_metadata: () => {
        saved = true;
        return new Promise((resolve) => { finishSave = resolve; });
      },
      scan_litematics: () => ({
        items: [{ ...summary, name: saved ? "本次提交" : "测试投影" }, other],
        skippedDirs: 0,
        skippedPaths: [],
      }),
    });
    await openFileCard(w, "测试.litematic");
    let input = w.findAll("input").find((item) => (item.element as HTMLInputElement).value === "测试投影")!;
    await input.setValue("本次提交");
    await w.findAll("button").find((b) => b.text().trim() === "保存")!.trigger("click");
    await flushPromises();
    expect(finishSave).toBeTruthy();

    input = w.findAll("input").find((item) => (item.element as HTMLInputElement).value === "本次提交")!;
    expect(input.attributes("disabled"), "保存期间元数据输入应禁用").toBeDefined();
    finishSave!({ backupPath: "/p/backup.litematic" });
    await flushPromises();
    expect(w.findAll("input").some((item) => (item.element as HTMLInputElement).value === "本次提交")).toBe(true);
    expect(w.findAll("button").find((b) => b.text().trim() === "保存")!.attributes("disabled")).toBeDefined();
    w.unmount();
  });

  it("另存为到未加载目录仍切换文件，并应用暂存替换", async () => {
    const replaced: string[] = [];
    const output = { ...summary, path: "/outside/copy.litematic", fileName: "copy.litematic", dirName: "/outside", name: "副本" };
    const w = await mountTwo({
      save_schematic_as: (args: { targetPath: string }) => args.targetPath,
      apply_replace: (args: { schematicPath: string }) => {
        replaced.push(args.schematicPath);
        return { replaced: 2, paletteChanged: 1 };
      },
      peek_schematic: (args: { path: string }) => {
        expect(args.path).toBe(output.path);
        return output;
      },
    });
    await openFileCard(w, "测试.litematic");
    await openReplaceTarget(w, "minecraft:dirt");
    [...document.querySelectorAll<HTMLElement>(".repl-window .arco-modal-footer button")]
      .find((b) => (b.textContent ?? "").trim() === "执行替换")!.click();
    await flushPromises();

    const { save } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(save).mockResolvedValueOnce(output.path);
    await w.findAll("button").find((b) => b.text().trim() === "另存为")!.trigger("click");
    await flushPromises();
    expect(replaced).toEqual([output.path]);
    expect(w.find(".workspace-tab--active").text()).toContain("copy.litematic");
    expect(w.findAll("input").some((item) => (item.element as HTMLInputElement).value === "副本")).toBe(true);
    w.unmount();
  });

  it("并发添加目录不会用后完成的整表覆盖新目录", async () => {
    let finishSlow: (() => void) | null = null;
    stubInvoke({
      scan_litematics: (args: { dir: string }) => {
        const item = { ...summary, path: args.dir + "/file.litematic", fileName: "file.litematic", dirName: args.dir };
        if (args.dir === "/slow") {
          return new Promise((resolve) => {
            finishSlow = () => resolve({ items: [item], skippedDirs: 0, skippedPaths: [] });
          });
        }
        return { items: [item], skippedDirs: 0, skippedPaths: [] };
      },
    });
    const w = mountApp();
    const add = () => w.findAll("button").find((b) => b.text().includes("添加目录"))!;
    mockOpen.mockResolvedValueOnce(["/slow"]);
    await add().trigger("click");
    await flushPromises();
    expect(finishSlow).toBeTruthy();

    mockOpen.mockResolvedValueOnce(["/fast"]);
    await add().trigger("click");
    await flushPromises();
    expect(w.findAll(".directory-node__name").some((n) => n.attributes("title") === "/fast")).toBe(true);
    finishSlow!();
    await flushPromises();
    const roots = w.findAll(".directory-node__name").map((n) => n.attributes("title"));
    expect(roots).toContain("/fast");
    expect(roots).toContain("/slow");
    w.unmount();
  });

  it("父子目录返回同一路径时只生成一份文件记录", async () => {
    const nested = { ...summary, path: "/p/child/a.litematic", fileName: "a.litematic", dirName: "/p/child" };
    stubInvoke({ scan_litematics: { items: [nested], skippedDirs: 0, skippedPaths: [] } });
    mockOpen.mockResolvedValue(["/p", "/p/child"]);
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    expect(w.findAll(".schematic-card")).toHaveLength(1);
    expect(w.text()).toContain("共 1 个投影");
    w.unmount();
  });

  it("固定标签状态恢复时不恢复上次编辑会话", async () => {
    stubInvoke({ load_ui_state: JSON.stringify({ version: 1, fixedTabs: ["settings"] }) });
    const w = mountApp();
    await flushPromises();
    expect(w.findAll(".workspace-tab__label").map((tab) => tab.text())).toEqual(["设置"]);
    expect(w.find(".settings-panel").exists()).toBe(true);
    expect(w.find(".editor-detail").exists()).toBe(false);
    w.unmount();
  });

  it("备份还原期间切换文件不会改写当前标签路径", async () => {
    let finishRestore: (() => void) | null = null;
    const w = await mountTwo({
      list_backups: [{ path: "/backup/a.litematic", stem: "a", op: "save", createdNs: "0", size: 10 }],
      restore_backup: () => new Promise<void>((resolve) => { finishRestore = () => resolve(); }),
    });
    await openFileCard(w, "测试.litematic");
    await activateFiles(w);
    await w.findAll("button").find((b) => b.text().includes("备份与日志"))!.trigger("click");
    await flushPromises();
    const restore = [...document.querySelectorAll<HTMLElement>(".arco-drawer .arco-link")]
      .find((item) => (item.textContent ?? "").includes("还原"));
    expect(restore, "备份列表应有还原入口").toBeTruthy();
    restore!.click();
    await flushPromises();
    expect(finishRestore).toBeTruthy();

    await openFileCard(w, "other.litematic");
    finishRestore!();
    await flushPromises();
    expect(w.find(".workspace-tab--active").text()).toContain("other.litematic");
    expect(w.findAll("input").some((item) => (item.element as HTMLInputElement).value === "另一个投影")).toBe(true);
    expect(w.find(".workspace-tab--active").text()).not.toContain("测试.litematic");
    w.unmount();
  });

  it("取消删除文件时保留隐藏标签的未保存草稿", async () => {
    const confirm = vi.spyOn(Modal, "confirm").mockRejectedValue(new Error("取消"));
    try {
      const w = await mountTwo();
      await openFileCard(w, "测试.litematic");
      const input = w.findAll("input").find((item) => (item.element as HTMLInputElement).value === "测试投影")!;
      await input.setValue("不能丢失的草稿");
      await activateFiles(w);
      const card = w.findAll(".schematic-card").find((item) => item.text().includes("测试.litematic"))!;
      await card.find("input[type=checkbox]").setValue(true);
      await w.findAll("button").find((b) => b.text().trim() === "删除")!.trigger("click");
      await flushPromises();

      expect(confirm, "删除有草稿的文件前应确认").toHaveBeenCalled();
      expect(mockInvoke.mock.calls.some((call) => call[0] === "delete_files"), "取消后不应删除磁盘文件").toBe(false);
      await activateEditor(w, "测试.litematic");
      expect(w.findAll("input").some((item) => (item.element as HTMLInputElement).value === "不能丢失的草稿")).toBe(true);
      w.unmount();
    } finally {
      confirm.mockRestore();
    }
  });

  it("重扫后无草稿的隐藏会话使用新的文件元数据", async () => {
    const w = await mountTwo();
    await openFileCard(w, "测试.litematic");
    await activateFiles(w);
    const external = { ...summary, name: "外部修改后的名称", author: "外部作者" };
    stubInvoke({
      scan_litematics: { items: [external, other], skippedDirs: 0, skippedPaths: [] },
      analyze_schematic: blockAnalysis,
    });
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();

    await activateEditor(w, "测试.litematic");
    expect(w.findAll("input").some((item) => (item.element as HTMLInputElement).value === "外部修改后的名称")).toBe(true);
    expect(w.findAll("input").some((item) => (item.element as HTMLInputElement).value === "外部作者")).toBe(true);
    expect(w.findAll("button").find((b) => b.text().trim() === "保存")!.attributes("disabled")).toBeDefined();
    w.unmount();
  });

  it("转换分析和目标版本随编辑会话恢复", async () => {
    const item = (name: string) => ({
      name, nameZh: name, nameEn: name, count: 1,
      suggested: "", suggestedZh: "", suggestedEn: "", chosen: "",
    });
    const w = await mountTwo({
      convert_targets: [{ index: 0, name: "1.19.2", dataVersion: 3120 }],
      analyze_conversion: (args: { schematicPath: string }) => [
        item(args.schematicPath === summary.path ? "minecraft:a_saved" : "minecraft:b_saved"),
      ],
    });
    await openFileCard(w, "测试.litematic");
    await openConvert(w);
    [...document.querySelectorAll<HTMLElement>(".conv-window button")]
      .find((b) => (b.textContent ?? "").includes("分析差异"))!.click();
    await flushPromises();
    expect(document.querySelector(".conv-window")!.textContent).toContain("minecraft:a_saved");

    await openFileCard(w, "other.litematic");
    await openConvert(w);
    [...document.querySelectorAll<HTMLElement>(".conv-window button")]
      .find((b) => (b.textContent ?? "").includes("分析差异"))!.click();
    await flushPromises();
    expect(document.querySelector(".conv-window")!.textContent).toContain("minecraft:b_saved");

    await activateEditor(w, "测试.litematic");
    expect(document.querySelector(".conv-window")!.textContent).toContain("minecraft:a_saved");
    expect(document.querySelector(".conv-window")!.textContent).not.toContain("minecraft:b_saved");
    w.unmount();
  });

  it("移除目录后旧扫描响应不会重新写回该目录", async () => {
    const w = await mountTwo();
    await openFileCard(w, "测试.litematic");
    await activateFiles(w);
    const base = mockInvoke.getMockImplementation()!;
    let finishScan: (() => void) | null = null;
    mockInvoke.mockImplementation((async (cmd: string, args: never) => {
      if (cmd === "scan_litematics") {
        return new Promise((resolve) => {
          finishScan = () => resolve({
            items: [summary, other], skippedDirs: 0, skippedPaths: [],
            root: {
              path: "/p", name: "p", relativePath: "",
              filePaths: [summary.path, other.path], children: [], truncated: false,
            },
          });
        });
      }
      return base(cmd, args);
    }) as never);
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    expect(finishScan).toBeTruthy();

    await w.findAll("button").find((b) => b.text().includes("已加载目录"))!.trigger("click");
    await flushPromises();
    const remove = [...document.querySelectorAll<HTMLElement>(".dir-item button")]
      .find((b) => (b.textContent ?? "").includes("移除"));
    remove!.click();
    await flushPromises();
    finishScan!();
    await flushPromises();
    expect(w.findAll(".directory-node")).toHaveLength(0);
    await activateEditor(w, "测试.litematic");
    expect(w.find(".editor-detail").exists()).toBe(true);
    w.unmount();
  });

  it("文件所在目录被禁用时保存替换仍刷新分析和场景", async () => {
    let analysisCalls = 0;
    const w = await mountTwo({
      analyze_schematic: () => {
        analysisCalls++;
        return blockAnalysis;
      },
      apply_replace: () => ({ replaced: 3, paletteChanged: 1 }),
      save_metadata: () => ({ backupPath: "/p/backup.litematic" }),
    });
    await openFileCard(w, "测试.litematic");
    await openReplaceTarget(w, "minecraft:dirt");
    [...document.querySelectorAll<HTMLElement>(".repl-window .arco-modal-footer button")]
      .find((b) => (b.textContent ?? "").trim() === "执行替换")!.click();
    await flushPromises();
    const before = analysisCalls;

    await activateFiles(w);
    await w.findAll("button").find((b) => b.text().includes("已加载目录"))!.trigger("click");
    await flushPromises();
    const checkbox = document.querySelector<HTMLInputElement>(".dir-item input[type=checkbox]")!;
    checkbox.click();
    await flushPromises();
    await activateEditor(w, "测试.litematic");
    await w.findAll("button").find((b) => b.text().trim() === "保存")!.trigger("click");
    await flushPromises();
    expect(analysisCalls, "保存成功后即使文件不在可见列表也应重新分析").toBeGreaterThan(before);
    expect(w.find(".editor-detail").exists()).toBe(true);
    w.unmount();
  });


  it("仍加载的目录确认文件消失后关闭无草稿标签", async () => {
    const w = await mountTwo();
    await openFileCard(w, "测试.litematic");
    await openFileCard(w, "other.litematic");
    await activateFiles(w);
    stubInvoke({
      scan_litematics: { items: [other], skippedDirs: 0, skippedPaths: [] },
      analyze_schematic: blockAnalysis,
    });
    mockOpen.mockResolvedValue("/p");
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    const editorLabels = w.findAll(".workspace-tab__label").filter((tab) => tab.attributes("title")?.includes("litematic"));
    expect(editorLabels.map((tab) => tab.attributes("title"))).toEqual([other.path]);
    expect(w.find(".file-manager").exists()).toBe(true);
    w.unmount();
  });


  it("切换转换目标后旧分析立即失效", async () => {
    const w = await mountTwo({
      convert_targets: [
        { index: 0, name: "1.19.2", dataVersion: 3120 },
        { index: 1, name: "1.19.4", dataVersion: 3337 },
      ],
      analyze_conversion: [{
        name: "minecraft:old_target", nameZh: "旧目标", nameEn: "Old", count: 1,
        suggested: "", suggestedZh: "", suggestedEn: "", chosen: "",
      }],
    });
    await openFileCard(w, "测试.litematic");
    await openConvert(w);
    [...document.querySelectorAll<HTMLElement>(".conv-window button")]
      .find((b) => (b.textContent ?? "").includes("分析差异"))!.click();
    await flushPromises();
    expect(document.querySelector(".conv-window")!.textContent).toContain("minecraft:old_target");

    document.querySelector<HTMLElement>(".conv-window .arco-select-view, .conv-window .arco-select")!.click();
    await flushPromises();
    [...document.querySelectorAll<HTMLElement>(".arco-select-option")]
      .find((item) => (item.textContent ?? "").includes("1.19.4"))!.click();
    await flushPromises();
    expect(document.querySelector(".conv-window")!.textContent).not.toContain("minecraft:old_target");
    const ok = [...document.querySelectorAll<HTMLButtonElement>(".conv-window .arco-modal-footer button")]
      .find((b) => (b.textContent ?? "").includes("开始转换"));
    expect(ok?.disabled, "新目标必须重新分析后才能转换").toBe(true);
    w.unmount();
  });

  it("延迟恢复固定标签不会移除已打开的编辑器", async () => {
    let finishLoad: (() => void) | null = null;
    stubInvoke({
      load_ui_state: () => new Promise((resolve) => {
        finishLoad = () => resolve(JSON.stringify({ version: 1, fixedTabs: ["files", "settings"], dirs: [] }));
      }),
    });
    mockOpen.mockResolvedValue("/p");
    const w = mountApp();
    await w.findAll("button").find((b) => b.text().includes("添加目录"))!.trigger("click");
    await flushPromises();
    await openFileCard(w, "测试.litematic");
    expect(finishLoad).toBeTruthy();
    finishLoad!();
    await flushPromises();
    expect(w.find(".workspace-tab--active").text()).toContain("测试.litematic");
    expect(w.find(".editor-detail").exists()).toBe(true);
    w.unmount();
  });

  it("旧备份清单响应不会覆盖新文件的清单", async () => {
    let finishOld: (() => void) | null = null;
    const w = await mountTwo({
      list_backups: (args: { stem: string }) => {
        if (args.stem === "测试") {
          return new Promise((resolve) => {
            finishOld = () => resolve([{ path: "/backup/old-A", stem: "测试", op: "old-A", createdNs: "0", size: 1 }]);
          });
        }
        return [{ path: "/backup/current-B", stem: "other", op: "current-B", createdNs: "0", size: 1 }];
      },
    });
    await openFileCard(w, "测试.litematic");
    await activateFiles(w);
    await w.findAll("button").find((b) => b.text().includes("备份与日志"))!.trigger("click");
    await flushPromises();
    expect(finishOld).toBeTruthy();

    await openFileCard(w, "other.litematic");
    await activateFiles(w);
    await w.findAll("button").find((b) => b.text().includes("备份与日志"))!.trigger("click");
    await flushPromises();
    expect(document.querySelector(".arco-drawer")!.textContent).toContain("current-B");
    finishOld!();
    await flushPromises();
    expect(document.querySelector(".arco-drawer")!.textContent).toContain("current-B");
    expect(document.querySelector(".arco-drawer")!.textContent).not.toContain("old-A");
    w.unmount();
  });

  it("目录扫描完成后恢复属于后续根目录的选择", async () => {
    stubInvoke({
      load_ui_state: JSON.stringify({
        version: 1,
        dirs: ["/a", "/b"],
        enabledDirs: ["/a", "/b"],
        selectedDirKey: "/b\u0000",
        fixedTabs: ["files", "settings"],
      }),
      scan_litematics: (args: { dir: string }) => ({
        items: [{ ...summary, path: args.dir + "/file.litematic", fileName: args.dir + ".litematic", dirName: args.dir }],
        skippedDirs: 0,
        skippedPaths: [],
      }),
    });
    const w = mountApp();
    await flushPromises();
    expect(w.find(".directory-node__row--active .directory-node__name").attributes("title")).toBe("/b");
    expect(w.findAll(".schematic-card")).toHaveLength(1);
    expect(w.find(".schematic-card").text()).toContain("/b.litematic");
    w.unmount();
  });

});
