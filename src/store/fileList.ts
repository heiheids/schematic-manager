/**
 * 文件列表域：目录加载、文件清单、选中与搜索、多选与文件操作、
 * 右键菜单、重复文件筛查、元数据草稿保存。
 *
 * 这些内容都围绕「左边那列文件」运转，互相咬合（选中文件才有草稿可保存；
 * 勾选集合服务于批量操作；筛查结果给行上色并预勾选），放在一个域里
 * 而不是拆成更多碎片。
 *
 * 与其他域的协作：选中变化时经 FileListDeps 调 3D 预览域重建场景与
 * 图标缓存；列宽复位经 FileListDeps 通知界面状态域回写。依赖只声明
 * 本域用到的成员，而不是整棵 store，避免类型上的循环引用。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { TableColumnData, TableData } from "@arco-design/web-vue";
import { open } from "@tauri-apps/plugin-dialog";
import { t } from "../i18n";
import {
  analyzeSchematic,
  copyFilesToClipboard,
  copyFilesToDir,
  deleteFiles,
  renameSchematic,
  revealInFileManager,
  saveMetadata,
  scanDuplicates,
  scanLitematics,
} from "../api";
import { NAME_COL_W } from "../composables/useSidebarResize";
import type {
  AnalysisResult,
  DuplicateScan,
  SchematicSummary,
  SceneData,
} from "../types";

/** 可排序的字段，与列表列一一对应（#43 点名的五项）。
 *
 * 定义在模块级而不是 slice 内部：界面状态域要用它校验从磁盘恢复的值，
 * 写在 slice 里就没法引用。 */
export type SortKey =
  | "fileName"
  | "name"
  | "mcVersionName"
  | "timeModified"
  | "author";

/** 排序方向。Arco 的列头第三次点击是「取消」，因此调用方还会传空值，
 *  这个类型只覆盖有方向的那两种。 */
export type SortOrder = "ascend" | "descend";

/** 文件列表域用到的其他域成员。
 *
 * 各域只声明自己用到的依赖，而不是参数化整棵 store：AppStore 由所有域的
 * 返回值组合而成，若参数类型写 AppStore 会形成「接口 extends 返回值、
 * 返回值依赖参数」的循环引用。
 */
export interface FileListDeps {
  /** 3D 预览域的场景；选中变化时需要清空 */
  scene: Ref<SceneData | null>;
  /** 3D 预览域的加载错误；清空场景时一并清掉 */
  sceneError: Ref<string>;
  /** 界面状态域的回写；列宽调整后触发 */
  persistUiState: () => void;
  /** 3D 预览域的图标缓存复位；换投影时调用 */
  resetBlockIcons: () => void;
  /** 3D 预览域的场景加载；选中变化时调用 */
  loadScene: (record: SchematicSummary) => void;
}

/** 疑似重复组：名称、尺寸、方块总数都相同，但内容并不相同。
 *
 * 与「重复」分开成两种结论，是因为两者的可信度根本不同：重复组里的文件
 * 逐字节一致，删掉多余的副本没有风险；疑似组只说明属性撞上了，可能只是
 * 同一投影被重新保存过，也可能压根是两个不同的投影。
 */
export interface SuspectGroup {
  /** 投影名称，同时是分组键的一部分，也是界面上供用户核对的信息 */
  name: string;
  /** 尺寸（X×Y×Z），与方块总数一起构成属性键 */
  enclosing: [number, number, number];
  totalBlocks: number;
  /** 组内文件路径，按路径排序 */
  paths: string[];
}

/** 状态栏要显示的内容：文案键与插值参数。
 *
 * 不存译文：存下来的译文会固定成赋值时的语言，切语言后不再跟着变。 */
export interface StatusText {
  key: string;
  params?: Record<string, unknown>;
}

/** 重复组配色的种类数，与 global.scss 里生成的色相数量一致。
 *
 * 两组共用一种颜色会让人误以为它们是同一组，因此这个数一旦调整，
 * 样式表里的循环次数必须跟着改。
 */
const DUP_COLOR_COUNT = 12;

export function createFileListSlice(s: FileListDeps) {
  /** 每个已加载目录 → 该目录的扫描结果。
   *
   * 用「目录 → 结果」而不是一个扁平列表：用户可一次加载多个目录，
   * 移除其中一个时只需丢掉它那一份，不必重新扫描其余目录。
   *
   * 键是用户选择的目录本身（不是文件所在的子目录）：
   * 一个目录下的文件可能分布在多层子目录里，用文件自身的 dirName 分不出
   * 「它属于哪次加载」。
   */
  const dirItems = ref<Record<string, SchematicSummary[]>>({});

  /** 已加载的目录，按加载顺序 */
  const loadedDirs = computed(() => Object.keys(dirItems.value));

  /** 在下拉中被勾选（即参与列表显示）的目录。
   *
   * 新加载的目录默认勾选：用户刚选完目录，期望立刻看到内容。
   * 用 Set 便于 O(1) 判断，且与「哪些目录被加载」分开——
   * 加载了但不勾选，就是这个目录的文件暂不显示。
   */
  const enabledDirs = ref<Set<string>>(new Set());

  /** 目录被勾选的依据：已加载且勾选，外部加进来的目录一律不显示 */
  function isDirEnabled(dir: string): boolean {
    return enabledDirs.value.has(dir);
  }

  /** 勾选/取消一个目录 */
  function toggleDir(dir: string, on: boolean) {
    const next = new Set(enabledDirs.value);
    if (on) next.add(dir);
    else next.delete(dir);
    enabledDirs.value = next;
    // 该目录的文件可能正被选中或勾选，需要一并收敛，
    // 否则详情会停留在已不显示的文件上
    syncSelection(files.value);
    pruneChecks(files.value);
  }

  /** 全部目录合并后的文件列表，只含被勾选目录的文件，按路径排序保证顺序稳定 */
  const files = computed<SchematicSummary[]>(() => {
    const all: SchematicSummary[] = [];
    for (const d of loadedDirs.value) {
      if (!enabledDirs.value.has(d)) continue;
      all.push(...(dirItems.value[d] ?? []));
    }
    all.sort((a, b) => a.path.localeCompare(b.path));
    return all;
  });

  const selected = ref<SchematicSummary | null>(null);
  const analysis = ref<AnalysisResult | null>(null);
  const analyzing = ref(false);
  const saving = ref(false);
  // 元数据编辑草稿。选中文件时用文件中的值填充
  const draft = ref({ fileName: "", name: "", author: "", description: "" });
  const dirty = ref(false);
  const searchText = ref("");

  // ---------------------------------------------------------------- 重复文件筛查
  //
  // 就地筛查地址栏里已勾选的目录，不另开页面：结果直接标在列表上。
  //
  // 「重复」与「疑似重复」必须分开对待：
  //   - 重复：后端逐字节确认内容完全相同，多出来的副本默认勾上，可以直接删；
  //   - 疑似：名称、尺寸、方块总数都一样而内容不同（同一份投影被重新保存过，
  //     也可能只是属性恰好撞上），只提示、不默认勾选。
  //   把疑似也默认勾上等于替用户认定「可以删」，那正是要避免的误识别。
  const dupBusy = ref(false);
  const dupResult = ref<DuplicateScan | null>(null);
  const dupSuspects = ref<SuspectGroup[]>([]);

  /** 每个重复文件的路径 -> 组序号（从 1 起）。
   *
   * 组序号用于给同组文件上同一种颜色：界面上一眼看得出「这几个是一组」。
   * 用 Map 而不是逐行查数组：列表可能有上千行，逐行线性查找会退化成平方复杂度。
   */
  const dupGroupNo = computed(() => {
    const m = new Map<string, number>();
    (dupResult.value?.groups ?? []).forEach((g, i) => {
      for (const p of g.paths) m.set(p, i + 1);
    });
    return m;
  });

  /** 每个疑似重复文件的路径 -> 疑似组序号（从 1 起） */
  const suspectGroupNo = computed(() => {
    const m = new Map<string, number>();
    dupSuspects.value.forEach((g, i) => {
      for (const p of g.paths) m.set(p, i + 1);
    });
    return m;
  });

  /** 重复文件总数（同一份内容重复计数） */
  const dupFileCount = computed(() => dupGroupNo.value.size);

  /** 疑似重复文件总数 */
  const suspectFileCount = computed(() => suspectGroupNo.value.size);

  /** 筛查已勾选的目录。
   *
   * 一次把全部目录交给后端，而不是逐目录调用再拼接：跨目录的同一份内容
   * 只有放在一起比才能归到同一组——逐个目录筛时它两边各只有一个成员，
   * 而只有一个成员的组根本不会返回，于是「同一份投影被复制到两个目录」
   * 这种最常见的重复反而查不出来。
   */
  async function scanDuplicatesInline() {
    const dirs = loadedDirs.value.filter((d) => enabledDirs.value.has(d));
    if (dirs.length === 0) {
      Message.warning(t("files.dupNoDir"));
      return;
    }
    dupBusy.value = true;
    // 记下发起时的世代：结果回来时若列表已经变过，这份结论对应的就不是
    // 用户眼前的文件集合了（期间仍可添加目录、勾选或移除目录）
    const epoch = dupEpoch;
    try {
      const r = await scanDuplicates(dirs);
      if (epoch !== dupEpoch) {
        Message.info(t("files.dupStale"));
        return;
      }
      dupResult.value = r;
      dupSuspects.value = findSuspects(r);
      // 默认勾选「多于重复文件」：每组保留第一份，其余勾上待删
      applyDefaultDupChecks();
      reportDupScan(r);
    } catch (e) {
      Message.error(t("files.dupFailed", { error: String(e) }));
      clearDuplicateScan();
    } finally {
      dupBusy.value = false;
    }
  }

  /** 筛查结果提示。
   *
   * 必须说出筛查了多少个文件：一个都没扫到，与「扫过了、确实没有重复」
   * 在界面上完全是两回事。只报「没有发现重复文件」会让用户以为功能坏了
   * ——它以前的毛病正是如此。
   */
  function reportDupScan(r: DuplicateScan) {
    if (r.scanned === 0) {
      Message.warning(t("files.dupNothing"));
      return;
    }
    const parts = [t("files.dupScanned", { count: r.scanned })];
    if (r.groups.length > 0) {
      parts.push(t("files.dupGroupsFound", { count: r.groups.length }));
    } else {
      parts.push(t("files.dupNoneFound"));
    }
    if (dupSuspects.value.length > 0) {
      parts.push(t("files.dupSuspectsPending", { count: dupSuspects.value.length }));
    }
    const text = parts.join(t("common.listSeparator"));
    if (r.groups.length > 0) Message.success(text);
    else Message.info(text);
  }

  /** 找出「疑似重复」：名称、尺寸、方块总数都相同，但内容并不相同。
   *
   * 用列表里已有的摘要算，不再读一遍文件：这些字段在目录扫描时就解析好了，
   * 再解析一次纯属白花时间。
   *
   * 要求名称非空：只靠尺寸与方块总数会把大量互不相干的投影凑成一组。
   */
  function findSuspects(r: DuplicateScan): SuspectGroup[] {
    const confirmed = new Set<string>();
    for (const g of r.groups) for (const p of g.paths) confirmed.add(p);

    const byKey = new Map<string, SuspectGroup>();
    const seen = new Set<string>();
    for (const f of files.value) {
      // 父目录与它自己的子目录同时被加载时，同一个文件在列表里出现两次。
      // 不去重的话它会自己和自己配对，凑出一组只有一个文件的「疑似重复」。
      if (seen.has(f.path)) continue;
      seen.add(f.path);
      if (f.error || !f.name) continue;
      // 用 \0 分隔：名称里带分隔符时也不会拼出相同的键
      const key = `${f.name}\u0000${f.enclosing.join(",")}\u0000${f.totalBlocks}`;
      const hit = byKey.get(key);
      if (hit) {
        hit.paths.push(f.path);
      } else {
        byKey.set(key, {
          name: f.name,
          enclosing: f.enclosing,
          totalBlocks: f.totalBlocks,
          paths: [f.path],
        });
      }
    }

    // 属性键覆盖全部文件，到输出时才剔掉已确认重复的那些。
    //
    // 反过来（建键时就排除）会漏报：a、b 逐字节相同、c 与它们属性相同而
    // 内容不同时，c 会孤零零地落单，被下面「至少两个成员」的条件滤掉，
    // 而 c 恰恰是用户最需要看一眼的那一份。
    return [...byKey.values()]
      .filter((g) => g.paths.length > 1)
      .map((g) => ({ ...g, paths: g.paths.filter((p) => !confirmed.has(p)).sort() }))
      .filter((g) => g.paths.length > 0)
      .sort((a, b) => a.paths[0].localeCompare(b.paths[0]));
  }

  /** 默认勾选「多于重复文件」：每组保留第一份，其余勾上待处理。
   *
   * 第一份显式取消勾选，而不是「不去动它」：它可能本来就是勾着的，
   * 那样「保留第一份」就名不副实，用户一按删除会把最后一份也删掉。
   * 疑似重复组不参与：它们内容并不相同，该不该删要用户看过再定。
   */
  function applyDefaultDupChecks() {
    const next = new Set(checkedPaths.value);
    for (const g of dupResult.value?.groups ?? []) {
      g.paths.forEach((p, i) => {
        if (i === 0) next.delete(p);
        else next.add(p);
      });
    }
    checkedPaths.value = next;
  }

  /** 这一行的重复组序号；不重复时为 0 */
  function dupNoOf(path: string): number {
    return dupGroupNo.value.get(path) ?? 0;
  }

  /** 这一行的疑似重复组序号；不属于任何疑似组时为 0 */
  function suspectNoOf(path: string): number {
    return suspectGroupNo.value.get(path) ?? 0;
  }

  /** 重复组的配色序号（从 1 起，0 表示不上色）。
   *
   * 配色只有限几种，超出后循环使用，相邻组因此始终不同色；
   * 行底色与行首色块用的是同一张表（见 global.scss 的「重复文件标记」）。
   */
  function dupColorNo(path: string): number {
    const no = dupNoOf(path);
    return no > 0 ? ((no - 1) % DUP_COLOR_COUNT) + 1 : 0;
  }

  /** 清除重复标记：恢复普通行样式；用户已有的勾选保持不变 */
  function clearDuplicateScan() {
    dupResult.value = null;
    dupSuspects.value = [];
  }

  // 列表一变，上一次的筛查结论就作废：增删目录、重新扫描、删除文件都会改变
  // 参与筛查的文件集合，而结论是按那个集合下的。留着旧标记最直接的后果是
  // 汇总条继续报「发现 3 组重复」，而副本其实已经被删掉了。
  //
  // 放在这里统一作废，而不是在增删目录的每个入口各写一遍——那些入口有四五个，
  // 漏掉任何一个都会留下同样的假结论。dupEpoch 同时让正在跑的筛查知道
  // 「你出发时的列表已经不是现在这个了」，从而丢弃过期结果。
  let dupEpoch = 0;
  watch(files, () => {
    dupEpoch++;
    clearDuplicateScan();
  });

  // ---------------------------------------------------------------- 排序
  //
  // 排序只改变**显示**顺序：files / orderedFiles 的语义（路径序、重复组置顶）
  // 仍被筛查、勾选、选中收敛等逻辑依赖，不能动它。因此这里在列表交给表格前
  // 单独排一次。
  //
  // 用工具条上的排序控件，而不是 Arco 的列头排序：要排的五个字段里，
  // 名称、时间、作者都不是列表列（列表只有文件名/版本/尺寸/方块数），
  // 为了排序把它们都加成列会把目录栏撑宽，而这一版正在做的正是布局优化。
  const sortKey = ref<SortKey | null>(null);
  const sortOrder = ref<SortOrder | null>(null);

  /** 指定排序字段与方向；两者都传 null 表示回到默认的路径序。 */
  function setSort(key: SortKey | null, order: SortOrder | null) {
    // 只给字段不给方向时按升序：调用方（下拉框）表达的是「按这个字段排」，
    // 不该要求它同时知道方向状态
    sortKey.value = key;
    sortOrder.value = key === null ? null : (order ?? "ascend");
  }

  /** 排序用的取值：字符串统一转小写比较，数字原样。
   *  作者、名称可能为空，空值统一排到末尾（不随升降序翻转），
   *  否则降序时一堆空作者会顶到最前面，盖住真正有内容的那几行。 */
  function sortValue(f: SchematicSummary, key: SortKey): string | number {
    if (key === "timeModified") return f.timeModified;
    if (key === "author") return (f.author ?? "").toLowerCase();
    if (key === "name") return (f.name ?? "").toLowerCase();
    if (key === "mcVersionName") return (f.mcVersionName ?? "").toLowerCase();
    return (f.fileName ?? "").toLowerCase();
  }

  /** 按当前排序规则排出一份新数组；未选排序时按路径（与原行为一致） */
  function sortFiles(list: SchematicSummary[]): SchematicSummary[] {
    const key = sortKey.value;
    const order = sortOrder.value;
    if (!key || !order) return [...list].sort((a, b) => a.path.localeCompare(b.path));
    const dir = order === "descend" ? -1 : 1;
    return [...list].sort((a, b) => {
      const va = sortValue(a, key);
      const vb = sortValue(b, key);
      // 空字符串排末尾，与升降序无关
      const ea = va === "";
      const eb = vb === "";
      if (ea !== eb) return ea ? 1 : -1;
      if (ea && eb) return a.path.localeCompare(b.path);
      const cmp =
        typeof va === "number" && typeof vb === "number"
          ? va - vb
          : String(va).localeCompare(String(vb), "zh-Hans-CN");
      // 值相同时按路径定序，保证顺序稳定（否则每次重排都可能换位置）
      return cmp !== 0 ? cmp * dir : a.path.localeCompare(b.path);
    });
  }

  /** 列表显示顺序：重复文件排在最前（同组相邻），其次是疑似重复组，
   *  其余按当前排序规则（未选排序时仍按路径）。
   *
   * 「靠前」是为了让用户一眼看到筛查结果，不必在几百行里自己找；
   * 同组相邻才谈得上顺着颜色一份份核对。没做过筛查时顺序与原来完全一致。
   *
   * 排序只在同一层级、同一组内部生效，不会把筛查结果排散——
   * 那些标记是用来顺着颜色核对的，一旦被排序拆开就失去了意义。
   */
  const orderedFiles = computed<SchematicSummary[]>(() => {
    if (!dupResult.value && dupSuspects.value.length === 0) {
      return sortFiles(files.value);
    }
    // 值里的两个数依次是「层级」与「组序号」：先按层级（重复在前、疑似在后），
    // 同层再按组序，同组内部按当前排序规则
    const rank = new Map<string, [number, number]>();
    (dupResult.value?.groups ?? []).forEach((g, i) => {
      for (const p of g.paths) rank.set(p, [0, i]);
    });
    dupSuspects.value.forEach((g, i) => {
      for (const p of g.paths) rank.set(p, [1, i]);
    });
    if (rank.size === 0) return sortFiles(files.value);
    return sortFiles(files.value).sort((a, b) => {
      const ra = rank.get(a.path);
      const rb = rank.get(b.path);
      if (ra && rb) return ra[0] - rb[0] || ra[1] - rb[1];
      if (ra) return -1;
      if (rb) return 1;
      return 0;
    });
  });

  /// 按名称、作者、文件名、路径过滤；同时匹配中英文输入不做转换，
  /// 保持与输入一致即可——投影命名本身就是混合的。
  const filteredFiles = computed(() => {
    const q = searchText.value.trim().toLowerCase();
    if (!q) return orderedFiles.value;
    return orderedFiles.value.filter((f) => {
      const hay = [f.fileName, f.path, f.name ?? "", f.author ?? ""]
        .join("\n")
        .toLowerCase();
      return hay.includes(q);
    });
  });

  /** 目录栏表格的列（默认宽度）。
   *
   * 用户可在表头拖动调整宽度（见 columnResizable）；调整结果存在
   * colWidths 里，由 tableColumns 套回，因此这里始终是「默认值」。
   *
   * 「尺寸」列原本只有 slotName。Arco 的列宽拖动只对有 dataIndex 的列生效
   * （table.js 里 resizable 要求 column.dataIndex），因此这里补一个 dataIndex——
   * td 渲染时 slotName 优先（实测 table-td.js:148），显示内容不受影响，
   * 只是让该列也能拖动。
   */
  const columns = computed<TableColumnData[]>(() => [
    // 名称列走插槽：重复文件要在文件名前带上组号色块，
    // 直接渲染 dataIndex 没法插入这个标记（见 DirSidebar 的 #fileName）。
    //
    // 这里关掉 Arco 自带的 tooltip：它把插槽内容整段的 textContent 当提示文本
    // （会变成「1文件名」），而且判断「是否溢出」时量的是自己的行内包裹层，
    // 盒级 flex 子项的宽度永远不超过它，于是长文件名反而不弹提示。
    // 提示改由插槽自己给出（见 .dup-name__text 的 title）。
    {
      title: t("files.colName"),
      dataIndex: "fileName",
      slotName: "fileName",
      width: NAME_COL_W,
      ellipsis: true,
      tooltip: false,
    },
    { title: t("files.colMcVersion"), dataIndex: "mcVersionName", width: 90 },
    // 110 是实测值：`16 × 12 × 16` 在 100px 下会折成两行
    { title: t("files.colDims"), dataIndex: "dims", slotName: "dims", width: 110 },
    // 方块总数收窄到参考图的 73px：这几列加起来正好等于参考里的目录栏宽度 666
    { title: t("files.colBlocks"), dataIndex: "totalBlocks", width: 73 },
    // 1px 占位列，只为让最后一列（方块总数）也能拖动。
    //
    // Arco 判定可拖动时要求 `index2 < row.length - 1`（table.js:1247）：
    // **最后一列永远不能拖**，因为它的宽度被当成「剩余空间」。
    // 而用户要求每一列都能调宽。补一个 1px 宽的占位列放在最右侧，
    // 它就不再是最后一列，从而获得拖动手柄；
    // 占位列自身没有 dataIndex，本来也不参与拖动，且被样式隐藏。
    //
    // Arco 的列 class 属性是 headerCellClass / bodyCellClass，**没有 className**
    // （见 interface.d.ts）。原先写 className 会被静默忽略，占位列因此仍带边框。
    {
      title: "",
      dataIndex: "__filler",
      width: 1,
      headerCellClass: "sidebar__filler-col",
      bodyCellClass: "sidebar__filler-col",
    },
  ]);

  const status = ref<StatusText>({ key: "files.statusReady" });

  const statusText = computed(() => {
    const n = files.value.length;
    const text = t(status.value.key, status.value.params ?? {});
    return n === 0 ? text : t("files.statusWithTotal", { status: text, count: n });
  });

  /** 状态栏内容以「文案键 + 参数」的形式存，而不是存译文。
   *
   * 译文一旦在赋值时算出来就固定成了当时的语言，切到英语后状态栏会继续
   * 显示中文。存键则每次渲染都按当前语言取。 */
  function setStatus(key: string, params?: Record<string, unknown>) {
    status.value = { key, params };
  }

  /// 列表被新的扫描结果替换后，让选中项与新列表保持一致。
  ///
  /// 选中项记录的是上一次扫描的路径。若只替换列表而不处理选中项，详情面板
  /// 会继续显示**已经不在列表中**的文件：「保存元数据」「方块替换」
  /// 「版本转换」都会作用到那个路径上，而用户在列表里选中不了它，
  /// 看不出自己的改动写到了哪里。
  ///
  /// 按路径比对而不是一律清空：在同一目录重新扫描时该文件仍在列表中，
  /// 此时应保留选中并刷新记录（元数据与解析错误都可能已经变化）。
  /// 这与 onRowClick 的行为一致——切换查看对象时草稿一律重置。
  function syncSelection(items: SchematicSummary[]) {
    const cur = selected.value;
    if (!cur) return;
    const hit = items.find((f) => f.path === cur.path);
    if (hit) {
      selected.value = hit;
      resetDraft(hit);
      return;
    }
    // 文件已不在新列表中：清空选中，以及只属于它的解析结果与预览，
    // 否则右侧面板会显示上一个目录的数据
    selected.value = null;
    resetDraft(null);
    analysis.value = null;
    s.scene.value = null;
    s.sceneError.value = "";
  }

  // ---------------------------------------------------------------- 目录栏多选与文件操作

  /** 已勾选的文件路径集合。
   *
   * 用 Set 而不是数组：勾选/取消/反选都只做 O(1) 的成员判断，
   * 而 129 个文件时每次全量比较数组会明显变慢。
   * 同时与列表数据解耦——列表重新扫描后，仍然存在的路径自然保持勾选。
   */
  const checkedPaths = ref<Set<string>>(new Set());

  /** 供 a-table 的 rowSelection 使用：Arco 要求数组形式的 keys */
  const checkedKeys = computed(() => Array.from(checkedPaths.value));

  /** 当前列表里被勾选的条数（只算可见的，避免「筛掉了却仍算选中」） */
  const checkedCount = computed(() =>
    filteredFiles.value.filter((f) => checkedPaths.value.has(f.path)).length,
  );

  /** 被勾选的文件记录（按列表顺序，保证操作顺序稳定可预期） */
  const checkedFiles = computed(() =>
    filteredFiles.value.filter((f) => checkedPaths.value.has(f.path)),
  );

  const hasChecked = computed(() => checkedCount.value > 0);

  /** 勾选变化：Arco 传回完整 keys 数组 */
  function onSelectionChange(keys: (string | number)[]) {
    checkedPaths.value = new Set(keys.map(String));
  }

  /** 全选当前列表（只选可见项：搜索过滤后「全选」应当只作用于筛选结果） */
  function checkAll() {
    const next = new Set(checkedPaths.value);
    for (const f of filteredFiles.value) next.add(f.path);
    checkedPaths.value = next;
  }

  /** 反选当前列表：可见项在「选中/未选中」之间翻转 */
  function invertChecks() {
    const next = new Set(checkedPaths.value);
    for (const f of filteredFiles.value) {
      if (next.has(f.path)) next.delete(f.path);
      else next.add(f.path);
    }
    checkedPaths.value = next;
  }

  /** 清除全部勾选（含被搜索过滤掉、当前不可见的那些） */
  function clearChecks() {
    checkedPaths.value = new Set();
  }

  /** 扫描结果更新后丢弃已不存在的勾选项。
   *
   * 必须清理：勾选集里留着已删除/已移走的路径，之后点「删除」或「复制」
   * 会因路径不存在而整批失败，用户却看不出是哪个文件的问题。
   */
  function pruneChecks(items: SchematicSummary[]) {
    const alive = new Set(items.map((f) => f.path));
    let changed = false;
    const next = new Set<string>();
    for (const p of checkedPaths.value) {
      if (alive.has(p)) next.add(p);
      else changed = true;
    }
    if (changed) checkedPaths.value = next;
  }

  // 列宽：由用户在表头拖动调整。
  const colWidths = ref<Record<string, number>>({});

  /** 表头拖动结束：记录该列的新宽度 */
  function onColumnResize(dataIndex: string, width: number) {
    colWidths.value = { ...colWidths.value, [dataIndex]: width };
    // 列宽属于「玩家对界面的修改」，同样要记住
    s.persistUiState();
  }

  /** 计算实际用于渲染的列：把用户调整过的宽度套回去。 */
  const tableColumns = computed<TableColumnData[]>(() =>
    columns.value.map((c) => {
      const key = String(c.dataIndex ?? "");
      const w = key ? colWidths.value[key] : undefined;
      return w ? { ...c, width: w } : c;
    }),
  );

  /** 表格的 key。改变它会强制重建表格组件。
   *
   * 复位列宽只能靠重建，原因在 Arco 内部：拖动时它把宽度记在自己的
   * `columnWidth` 里，渲染时按
   *   `columnWidth[dataIndex] || column.width`
   * 取值（table-col-group.js）。也就是说一旦内部记下宽度，外部怎么改
   * `columns.width` 都不会生效——实测拖到 266px 后点复位，仍停在 266px。
   * Arco 没有提供清除该状态的接口，因此改为换 key 重建。
   */
  const tableKey = ref(0);

  /** 恢复默认列宽：清掉用户改动，并重建表格以清掉 Arco 内部记录 */
  function resetColumnWidths() {
    colWidths.value = {};
    tableKey.value += 1;
    s.persistUiState();
  }

  const fileBusy = ref(false);

  /** 统一的文件操作错误提示：把后端给的原因原样带出来 */
  function reportFileOpError(action: string, e: unknown) {
    Message.error(t("files.failed", { action, error: String(e) }));
  }

  function formatBytes(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / 1024 / 1024).toFixed(2)} MB`;
  }

  /** 删除勾选的文件（移入回收站） */
  async function doDeleteFiles() {
    const list = checkedFiles.value;
    if (list.length === 0) return;
    fileBusy.value = true;
    try {
      const r = await deleteFiles(list.map((f) => f.path));
      Message.success(
        t("files.deleted", { count: r.names.length, size: formatBytes(r.totalBytes) }),
      );
      // 从各个目录的结果里移除已删除的文件。
      // 按路径而不是文件名移除：不同目录下可能有同名文件，
      // 按文件名会把另一个目录里同名的文件也一并移出列表。
      const gone = new Set(list.map((f) => f.path));
      const next: Record<string, SchematicSummary[]> = {};
      for (const d of loadedDirs.value) {
        next[d] = (dirItems.value[d] ?? []).filter((f) => !gone.has(f.path));
      }
      dirItems.value = next;
      pruneChecks(files.value);
      syncSelection(files.value);
    } catch (e) {
      reportFileOpError(t("files.delete"), e);
    } finally {
      fileBusy.value = false;
    }
  }

  /** 复制勾选的文件到剪贴板 */
  async function doCopyFiles() {
    const list = checkedFiles.value;
    if (list.length === 0) return;
    fileBusy.value = true;
    try {
      const r = await copyFilesToClipboard(list.map((f) => f.path));
      Message.success(
        t("files.copiedToClipboard", {
          count: r.names.length,
          size: formatBytes(r.totalBytes),
        }),
      );
    } catch (e) {
      reportFileOpError(t("files.copy"), e);
    } finally {
      fileBusy.value = false;
    }
  }

  /** 复制到指定目录：先让用户选目标目录 */
  async function doCopyFilesTo() {
    const list = checkedFiles.value;
    if (list.length === 0) return;
    try {
      const dest = await open({
        directory: true,
        multiple: false,
        title: t("files.chooseDestDir"),
      });
      if (typeof dest !== "string") return;
      fileBusy.value = true;
      const r = await copyFilesToDir(list.map((f) => f.path), dest);
      Message.success(
        t("files.copiedToDir", {
          count: r.names.length,
          size: formatBytes(r.totalBytes),
          dir: r.destDir,
        }),
      );
    } catch (e) {
      reportFileOpError(t("files.copyTo"), e);
    } finally {
      fileBusy.value = false;
    }
  }

  /** 打开第一个勾选文件所在的文件夹。
   *
   * 只处理第一个：资源管理器的「选中」语义只对一个目标有意义，
   * 因此界面上也如实提示用的是哪一个。
   */
  async function doOpenFolder() {
    const first = checkedFiles.value[0];
    if (!first) return;
    try {
      await revealInFileManager(first.path);
      Message.success(t("files.located", { name: first.fileName }));
    } catch (e) {
      reportFileOpError(t("files.openFolder"), e);
    }
  }

  /** 右键菜单：只保留「打开文件夹」，作用于右键点中的那一行。
   *
   * 用 a-table 的 rowContextmenu 事件而不是自定义 tr 插槽：
   * 该插槽在 Arco 里不存在（实测只在 tbody 内部渲染 tr），
   * 写上去不会报错但永远不生效。
   *
   * 菜单位置用 fixed 定位到鼠标处；点击别处、窗口失焦或按 Esc 关闭。
   */
  const ctxMenu = ref<{ open: boolean; x: number; y: number; path: string; name: string }>({
    open: false,
    x: 0,
    y: 0,
    path: "",
    name: "",
  });

  function onRowContextMenu(raw: TableData, ev: Event) {
    ev.preventDefault();
    const row = raw as unknown as SchematicSummary;
    const me = ev as MouseEvent;
    // 视口下边缘附近要向上翻，否则菜单会被窗口裁掉
    const menuH = 40;
    const y = me.clientY + menuH > window.innerHeight ? me.clientY - menuH : me.clientY;
    ctxMenu.value = { open: true, x: me.clientX, y, path: row.path, name: row.fileName };
  }

  function closeRowMenu() {
    if (ctxMenu.value.open) ctxMenu.value = { ...ctxMenu.value, open: false };
  }

  async function revealRow(row: { path: string; name: string }) {
    closeRowMenu();
    try {
      await revealInFileManager(row.path);
      Message.success(t("files.located", { name: row.name }));
    } catch (e) {
      reportFileOpError(t("files.openFolder"), e);
    }
  }

  /** 全局：点击别处或按 Esc 关闭右键菜单。
   *
   * 点在我们自己的菜单上时**不能**关：pointerdown 先于 click 触发，
   * 若在这里把菜单从 DOM 摘掉（v-if），随后的 click 永远落不到菜单项上，
   * 表现就是「右键菜单点不动、点了没反应」。这是菜单项唯一失效的原因，
   * 工具栏那个同名按钮走的是另一条路径，所以它一直是好的。
   */
  function onGlobalPointerDown(e: Event) {
    const t = e.target as Node | null;
    // blur 事件的 target 是 window，没有 closest，因此先确认是元素节点
    const el = t && t.nodeType === 1 ? (t as HTMLElement) : null;
    if (el?.closest?.(".ctx-menu")) return;
    closeRowMenu();
  }
  function onGlobalKey(e: KeyboardEvent) {
    if (e.key === "Escape") closeRowMenu();
  }

  /** 全局禁用浏览器默认右键菜单。
   *
   * 不拦截时，右键会弹出 WebView2 自带的那套
   * 「返回 / 刷新 / 另存为 / 打印 / 更多工具」——它是浏览器菜单，
   * 出现在桌面应用里既突兀又无用，还会盖住我们自己的菜单。
   *
   * 这里用前端拦截而不是调 WebView2 的
   * `SetAreDefaultContextMenusEnabled(false)`：后者需要在启动阶段拿到
   * 控制器，失败时只能打日志、用户在界面上看不到任何反馈，而这一步
   * 一旦没生效就完全退回浏览器菜单，属于「静默失效」。前端监听则
   * 不依赖平台接口，且能被自动化测试直接验证。
   *
   * 只在没有自己的菜单要展示时才 preventDefault：text/textarea 上保留
   * 原生菜单（复制/粘贴），否则用户没法在输入框里用右键粘贴。
   */
  function onDocumentContextMenu(e: MouseEvent) {
    const el = e.target as HTMLElement | null;
    // 输入类元素保留原生菜单：里面的复制/粘贴是系统能力，我们没实现
    if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) {
      return;
    }
    e.preventDefault();
  }

  onMounted(() => {
    // 右键菜单：点击别处、滚动、按 Esc 都要关闭，否则菜单会一直挂在屏幕上
    window.addEventListener("pointerdown", onGlobalPointerDown);
    window.addEventListener("keydown", onGlobalKey);
    window.addEventListener("blur", onGlobalPointerDown);
    // 拦截浏览器默认右键菜单（capture 阶段，确保在组件处理之前生效）
    document.addEventListener("contextmenu", onDocumentContextMenu, true);
  });

  // 组件卸载时摘掉 window 上的监听，避免留下悬挂的监听
  onBeforeUnmount(() => {
    window.removeEventListener("pointerdown", onGlobalPointerDown);
    window.removeEventListener("keydown", onGlobalKey);
    window.removeEventListener("blur", onGlobalPointerDown);
    document.removeEventListener("contextmenu", onDocumentContextMenu, true);
  });

  async function pickDirectory() {
    // 对话框调用本身也可能失败（插件未就绪、系统对话框不可用）。
    // 放在 try 之外会让这个 Promise 静默 reject：用户点了「添加目录」
    // 却什么都没发生，也收不到任何提示。其余对话框调用都在 try 内，
    // 这里保持一致。
    let picked: unknown;
    try {
      picked = await open({
        directory: true,
        multiple: true,
        title: t("files.chooseSourceDir"),
      });
    } catch (e) {
      Message.error(t("files.openPickerFailed", { error: String(e) }));
      return;
    }

    // multiple: true 时返回数组；某些平台/插件版本可能仍返回单个字符串，
    // 两种都接受，避免「多选后一个都不加载」。
    const dirs: string[] = (Array.isArray(picked) ? picked : [picked]).filter(
      (d): d is string => typeof d === "string" && d.length > 0,
    );
    if (dirs.length === 0) return;

    setStatus("files.statusScanning");
    const next = { ...dirItems.value };
    const parts: string[] = [];
    let bad = 0;
    let skippedDirs = 0;
    const skippedPaths: string[] = [];
    let added = 0;

    for (const dir of dirs) {
      try {
        const r = await scanLitematics(dir);
        if (!(dir in next)) added++;
        next[dir] = r.items;
        bad += r.items.filter((f) => f.error).length;
        skippedDirs += r.skippedDirs;
        skippedPaths.push(...r.skippedPaths);
      } catch (e) {
        // 一个目录失败不应让其余目录也加载不了；如实报出是哪个目录
        Message.error(t("files.scanDirFailed", { dir, error: String(e) }));
      }
    }

    if (Object.keys(next).length === Object.keys(dirItems.value).length && added === 0) {
      // 一个目录都没加进来（全部失败或被取消）
      if (skippedDirs === 0) Message.warning(t("files.noNewDirs"));
      setStatus("files.statusScanDone");
      return;
    }

    dirItems.value = next;
    // 新加载的目录默认勾选：用户刚选完目录，期望立刻看到内容。
    // 已存在的目录保持原勾选状态，不因为重新扫描而被重置。
    const checked = new Set(enabledDirs.value);
    for (const dir of dirs) {
      if (dir in next) checked.add(dir);
    }
    enabledDirs.value = checked;
    // 新的合并列表出来后收敛选中与勾选
    syncSelection(files.value);
    pruneChecks(files.value);

    if (added > 0) parts.push(t("files.addedDirs", { count: added }));
    if (skippedDirs > 0) {
      parts.push(t("files.skippedDirsPart", { count: skippedDirs }));
      // 列出被跳过的路径，否则用户无从判断少了哪些内容
      Message.warning({
        content: t("files.skippedDirsWarning", { paths: skippedPaths.join("\n") }),
        duration: 8000,
      });
    }
    if (bad > 0) parts.push(t("files.parseFailedPart", { count: bad }));
    if (parts.length > 0) {
      setStatus("files.statusScanDoneWith", {
        parts: parts.join(t("common.listSeparator")),
      });
    } else {
      setStatus("files.statusScanDone");
    }
  }

  /** 移除一个已加载目录。
   *
   * 只丢掉它那一份扫描结果，不重新扫描其余目录：重扫又慢又可能因
   * 某个目录临时不可读而连带失败，而用户只是想移掉一个目录。
   */
  function removeDir(dir: string) {
    const next = { ...dirItems.value };
    delete next[dir];
    dirItems.value = next;
    // 勾选集里也要去掉它：留着已卸载目录的条目，之后重新加载同名目录时
    // 会「自己就勾上了」，与用户预期不符
    const checked = new Set(enabledDirs.value);
    checked.delete(dir);
    enabledDirs.value = checked;
    // 该目录的文件可能正被选中或勾选，需要一并收敛，
    // 否则右侧详情会停留在已移出列表的文件上
    syncSelection(files.value);
    pruneChecks(files.value);
    Message.info(t("files.dirRemoved", { dir }));
  }

  /** 重新扫描全部已加载目录，用于文件被改动之后。
   *
   * 逐个目录独立处理：某一个目录临时读不到（被删除、权限变化）时，
   * 保留它原有结果并提示，而不是让整次刷新失败、把其它目录也清空。
   */
  /** 改名成功后把界面里所有按路径记录的状态迁到新路径。
   *
   * 改名是磁盘操作，界面若不跟着迁，就会出现「记录指向一个已不存在的路径」：
   *   - selected 与 dirItems 里的记录：再点保存会拿旧路径去重命名，必然失败；
   *   - checkedPaths：按路径勾选，不迁则该文件变成「未勾选」，批量操作漏掉它；
   *   - dupGroupNo：重复文件分组按路径标记，不迁则行的高亮/分组号凭空消失。
   *
   * 这里只做本地收敛，不重新扫描：调用方随后会 rescanAll 或在失败分支里补扫。
   */
  function syncRenamedPath(cur: SchematicSummary, newPath: string, newName: string) {
    const patch = <T extends SchematicSummary>(f: T): T =>
      f.path === cur.path ? { ...f, path: newPath, fileName: newName } : f;

    if (selected.value?.path === cur.path) {
      selected.value = { ...selected.value, path: newPath, fileName: newName };
    }

    const moved: Record<string, SchematicSummary[]> = {};
    for (const [dir, items] of Object.entries(dirItems.value)) {
      moved[dir] = items.map(patch);
    }
    dirItems.value = moved;

    if (checkedPaths.value.has(cur.path)) {
      const next = new Set(checkedPaths.value);
      next.delete(cur.path);
      next.add(newPath);
      checkedPaths.value = next;
    }
    // 改名会改动 dirItems，下面的 watcher 随即作废上一次的筛查结论，
    // 因此这里不需要（也不该）把组里的旧路径改成新路径：
    // 旁路维护一份随时会被清掉的镜像，只会让人以为它还有用。
  }

  async function rescanAll(): Promise<void> {
    const dirs = loadedDirs.value;
    if (dirs.length === 0) return;
    const next: Record<string, SchematicSummary[]> = {};
    for (const d of dirs) {
      try {
        const r = await scanLitematics(d);
        next[d] = r.items;
      } catch (e) {
        Message.error(t("files.rescanFailed", { dir: d, error: String(e) }));
        next[d] = dirItems.value[d] ?? [];
      }
    }
    dirItems.value = next;
  }

  function resetDraft(record: SchematicSummary | null) {
    draft.value = {
      fileName: record?.fileName ?? "",
      name: record?.name ?? "",
      author: record?.author ?? "",
      description: record?.description ?? "",
    };
    dirty.value = false;
  }

  /** 文件名是否被改过（忽略首尾空白，但**区分大小写**）。
   *
   * 不能忽略大小写：把 demo.litematic 改成 DEMO.litematic 是用户明确的意图，
   * 忽略它会让保存看起来成功、名字却没变（清空草稿后输入框又变回旧名），
   * 而且全程没有任何提示。后端能正确处理仅大小写的改名。
   */
  function fileNameChanged(cur: SchematicSummary): boolean {
    return draft.value.fileName.trim() !== cur.fileName;
  }

  async function saveDraft() {
    const cur = selected.value;
    if (!cur) return;
    saving.value = true;
    let renameNote = "";
    let renamedOk = false;
    try {
      // 改了文件名就先改名，再把元数据写到**新路径**上。
      // 顺序不能反：先写元数据再改名，会先给旧名字留一份备份，
      // 而改名又要重新链接备份，同一件事被做两遍且中间态对不上。
      let targetPath = cur.path;
      if (fileNameChanged(cur)) {
        const renamed = await renameSchematic(cur.path, draft.value.fileName.trim());
        targetPath = renamed.path;
        renameNote = t("files.renamedTo", { name: renamed.fileName });
        renamedOk = true;
        if (renamed.relinkError) {
          // 文件已改名成功，只是备份没接上：如实告知，不能静默
          Message.warning(t("files.backupRelinkFailed", { error: renamed.relinkError }));
        } else if (renamed.relinked > 0) {
          renameNote += t("files.backupsFollowed", { count: renamed.relinked });
        }
        // 改名成功后必须立刻把界面里的路径收敛到新路径。
        //
        // 若只等下面的 saveMetadata 成功才更新，一旦写元数据失败，selected 还指向
        // 旧路径、dirty 仍为 true；用户再点一次「保存」就会拿一个已不存在的路径去
        // 重命名，必然失败——从此再也存不下去，而磁盘上文件名其实早就改了。
        syncRenamedPath(cur, renamed.path, renamed.fileName);
      }
      const out = await saveMetadata(targetPath, {
        name: draft.value.name,
        author: draft.value.author,
        description: draft.value.description,
      });
      dirty.value = false;
      // 重新读取，确保界面显示的是文件中的真实内容而不是内存里的草稿
      await rescanAll();
      const hit = files.value.find((f) => f.path === targetPath);
      if (hit) {
        selected.value = hit;
        resetDraft(hit);
        Message.success(t("files.saved", { note: renameNote, path: out.backupPath }));
      } else {
        // 文件被移动或重命名时，界面若继续显示旧记录会误导用户
        Message.warning(
          t("files.savedMissing", { note: renameNote, path: out.backupPath }),
        );
      }
    } catch (e) {
      // 改名与写元数据是两步，失败原因必须分清：只报「保存失败」会让用户
      // 以为文件名也没变，而磁盘上它其实已经改了
      if (renamedOk) {
        Message.error(
          t("files.renamedButMetaFailed", { note: renameNote, error: String(e) }),
        );
      } else {
        Message.error(t("files.saveFailed", { error: String(e) }));
      }
      // 重新扫描，让界面回到与磁盘一致的状态（改名已生效的那部分也要体现出来）。
      // 重扫本身失败时 rescanAll 已逐目录提示过，这里只是兜底不让它打断 finally。
      await rescanAll().catch(() => {});
    } finally {
      saving.value = false;
    }
  }

  // Arco 的 row-click 回调给出的是通用的 TableData，
  // 这里收窄回业务类型；行数据本就来自 files，字段是一致的。
  async function onRowClick(raw: TableData) {
    const record = raw as unknown as SchematicSummary;
    selected.value = record;
    s.resetBlockIcons();
    analysis.value = null;
    resetDraft(record);
    if (record.error) return;
    // 先解析再建 3D 场景：方块数过大时预览区不自动渲染，而「过大」要看
    // 统计结果才能判断。两个命令各自都要读一遍文件，串行并不比并行慢多少。
    analyzing.value = true;
    try {
      analysis.value = await analyzeSchematic(record.path);
    } catch (e) {
      Message.error(t("files.analyzeFailed", { error: String(e) }));
    } finally {
      analyzing.value = false;
    }
    void s.loadScene(record);
  }

  /** 重新解析当前选中项。
   *
   * 方块名与渲染图都来自基础库，库一变（版本库补/重建了版本、语言表补齐）
   * 先前那份分析结果里存的就是旧名字。不重解析的话，用户会看到「库已经更新了、
   * 界面还是英文名」，只能靠再点一次那一行才会变——那不该由用户去猜。
   */
  async function refreshSelectedAnalysis() {
    const cur = selected.value;
    if (!cur) return;
    await onRowClick(cur as unknown as TableData);
  }

  /** 列表行的样式类：当前选中项高亮，重复文件按组上色，疑似重复另行标出。
   *
   * 用 a-table 的 rowClass 而不是自己拼 tr：行的 DOM 由 Arco 生成，
   * 手改会在排序/筛选重新渲染后丢失。
   *
   * 重复组与疑似组的样式不同：重复用实心底色的组色，疑似用统一的警示色。
   * 两者不能共用一套配色——那样用户分不清「这几个内容一样」与
   * 「这几个只是看起来一样」。
   */
  function rowClass(raw: TableData): string {
    const rec = raw as unknown as SchematicSummary;
    const cls: string[] = [];
    if (selected.value?.path === rec.path) cls.push("row-selected");
    const no = dupNoOf(rec.path);
    if (no > 0) {
      cls.push(`row-dup-${dupColorNo(rec.path)}`);
    } else if (suspectNoOf(rec.path) > 0) {
      cls.push("row-dup-suspect");
    }
    return cls.join(" ");
  }

  /** 该行是否属于某个重复组，用于在文件名旁显示标记 */
  function isDup(path: string): boolean {
    return dupNoOf(path) > 0;
  }

  /** 该行是否属于某个疑似重复组 */
  function isSuspect(path: string): boolean {
    return suspectNoOf(path) > 0;
  }

  return {
    dirItems,
    loadedDirs,
    enabledDirs,
    isDirEnabled,
    toggleDir,
    files,
    filteredFiles,
    searchText,
    sortKey,
    sortOrder,
    setSort,
    selected,
    analysis,
    analyzing,
    status,
    statusText,
    draft,
    dirty,
    saving,
    resetDraft,
    fileNameChanged,
    saveDraft,
    pickDirectory,
    removeDir,
    rescanAll,
    syncSelection,
    syncRenamedPath,
    onRowClick,
    refreshSelectedAnalysis,
    columns,
    colWidths,
    onColumnResize,
    tableColumns,
    tableKey,
    resetColumnWidths,
    checkedPaths,
    checkedKeys,
    checkedCount,
    checkedFiles,
    hasChecked,
    onSelectionChange,
    checkAll,
    invertChecks,
    clearChecks,
    pruneChecks,
    fileBusy,
    reportFileOpError,
    formatBytes,
    doDeleteFiles,
    doCopyFiles,
    doCopyFilesTo,
    doOpenFolder,
    ctxMenu,
    onRowContextMenu,
    closeRowMenu,
    revealRow,
    dupBusy,
    dupResult,
    dupSuspects,
    dupGroupNo,
    dupFileCount,
    suspectFileCount,
    scanDuplicatesInline,
    clearDuplicateScan,
    rowClass,
    dupNoOf,
    dupColorNo,
    isDup,
    isSuspect,
  };
}

export type FileListSlice = ReturnType<typeof createFileListSlice>;
