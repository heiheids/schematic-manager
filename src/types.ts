// 与 Rust 侧 src-tauri/src/dto.rs 一一对应。
// 字段名在两处必须保持一致，改动时请同步。

export interface SchematicSummary {
  path: string;
  fileName: string;
  dirName: string;
  fileSize: number;
  error: string;
  formatVersion: number;
  dataVersion: number;
  mcVersionName: string;
  name: string;
  author: string;
  description: string;
  timeCreated: number;
  timeModified: number;
  enclosing: [number, number, number];
  totalBlocks: number;
  totalVolume: number;
  regionCount: number;
  entityCount: number;
  tileEntityCount: number;
  pendingTicks: number;
  hasPreview: boolean;
}

/** 批量文件操作的结果（与后端 FileOpOutcome 对齐） */
export interface FileOpOutcome {
  /** 实际处理的文件名 */
  names: string[];
  /** 复制操作写入的目标路径；删除时为空 */
  written: string[];
  /** 目标目录（复制时） */
  destDir: string;
  /** 处理的文件总字节数 */
  totalBytes: number;
}

export interface BlockCount {
  name: string;
  state: string;
  count: number;
  /** 中文名，取自方块库语言表；查不到时为空串 */
  zhName: string;
  /** 英文名，取自方块库语言表；查不到时为空串 */
  enName: string;
}

/** 方块统计表的一行。
 *
 * 与 BlockCount 的区别在统计口径：BlockCount 是「某个方块状态用了多少个」，
 * 而这里按**物品**合计——忽略方块状态（朝向、强度、是否含水），也合并同一个
 * 物品的不同方块形态（火把与墙上的火把、炼药锅的内容物、铁砧的破损程度），
 * 因为用户想知道的是「这种物品一共要备多少个」。
 *
 * `name` 是**物品名**（水折成水桶、耕地折成泥土），不是方块名；代表方块的
 * 名字在 `state` 的方括号之前那一段里。`state` 还供渲染图使用。注意替换是按
 * 方块名精确匹配的，而这一行可能跨多个方块名，因此 `forms` 必须保留下来
 * （见 blockReplace.ts 的 sourceOf）。
 */
export interface BlockRow extends BlockCount {
  /**
   * 行的唯一键，供表格的 row-key 与「当前行」判定使用。
   *
   * 不能借用 `state`：按物品合计时同一个状态可能出现在两行里（带花的花盆会
   * 同时贡献「花盆」与「植物」两件物品），两张行共用同一个 state 会让表格
   * 认错行，也会让行内按钮作用到另一行上。
   */
  key: string;
  /** 这一行合计进来的方块状态条目数；1 表示只有一条 */
  stateCount: number;
  /** 这一行跨了哪几个方块名（火把与墙上的火把是两个），按首次出现顺序 */
  forms: string[];
}

export interface EntityCount {
  id: string;
  count: number;
  /** 中文名，取自方块库语言表；查不到时为空串 */
  zhName: string;
  /** 英文名，取自方块库语言表；查不到时为空串 */
  enName: string;
}

/** 一个实体实例：id 与投影内坐标（已含区域原点） */
export interface EntityInstance {
  id: string;
  x: number;
  y: number;
  z: number;
}

/** 实体贴图库的可用情况 */
export interface EntityTextureStatus {
  available: boolean;
  versions: number;
  textures: number;
  paths: number;
  /** 不可用时的原因；可用时为空串 */
  error: string;
}

/** 一张实体贴图的像素 */
export interface EntityTexture {
  path: string;
  width: number;
  height: number;
  hasAlpha: boolean;
  /** RGBA8888 像素，长度恒为 width * height * 4 */
  rgba: number[];
}

/** 目录扫描结果 */
export interface DirectoryNode {
  path: string;
  name: string;
  relativePath: string;
  /** 只包含当前目录的直接文件，不包含后代目录 */
  filePaths: string[];
  children: DirectoryNode[];
  /** 达到扫描深度，尚未读取该目录的子项 */
  truncated: boolean;
}

export interface ScanResult {
  items: SchematicSummary[];
  /** 目录树根，包含没有投影文件的目录 */
  root: DirectoryNode;
  /** 因权限或 IO 错误被跳过的目录数 */
  skippedDirs: number;
  /** 被跳过的目录路径（最多前若干条） */
  skippedPaths: string[];
}

/** 方块替换规则 */
export interface ReplaceRule {
  fromName: string;
  fromProps: [string, string][];
  toName: string;
  toProps: [string, string][];
  /** 是否继承源方块中目标也支持的属性 */
  inheritProps: boolean;
}

/** 方块替换结果 */
export interface ReplaceOutcome {
  replaced: number;
  paletteChanged: number;
}

/** 方块所在的版本 */
export interface BlockVersion {
  index: number;
  dataVersion: number;
}

/** 方块检索结果 */
export interface BlockHit {
  name: string;
  zh: string;
  en: string;
  versions: BlockVersion[];
}

/** 重命名投影文件的结果 */
export interface RenameOutcome {
  /** 重命名后的完整路径 */
  path: string;
  /** 重命名后的文件名 */
  fileName: string;
  /** 被重新链接到新名字的备份分组数 */
  relinked: number;
  /**
   * 备份重新链接失败的原因。
   *
   * 与 relinked 分开而不是让整个命令失败：文件本身已经改名成功，
   * 若这时当成整体失败，界面显示的状态会与磁盘不符。
   */
  relinkError?: string;
}

/** 一个方块的渲染图 */
export interface BlockIcon {
  /** 方块状态键，与请求里的一致 */
  state: string;
  /** `data:image/png;base64,...`；渲染失败时为空串 */
  dataUrl: string;
  /** 渲染失败的原因；成功时为空串 */
  error: string;
}

/** 方块库中的版本选项 */
export interface VersionOption {
  index: number;
  name: string;
  dataVersion: number;
  isOverlay: boolean;
  blockCount: number;
}

/** 转换中需要处理的一项 */
export interface ConvertItem {
  name: string;
  /** 源方块的中英两个名字；界面按当前语言取一套 */
  nameZh: string;
  nameEn: string;
  count: number;
  suggested: string;
  /** 推荐替代品的中英两个名字；没有推荐时为空 */
  suggestedZh: string;
  suggestedEn: string;
  chosen: string;
}

/** 可作为转换目标的版本 */
export interface ConvertTarget {
  index: number;
  name: string;
  dataVersion: number;
}

/** 转换结果 */
export interface ConvertOutcome {
  replacedKinds: number;
  replacedBlocks: number;
  skippedKinds: number;
  outputPath: string;
}

/** 一组内容相同的投影文件 */
export interface DuplicateGroup {
  size: number;
  paths: string[];
}

/** 未能读取的文件 */
export interface UnreadableFile {
  path: string;
  reason: string;
}

/** 重复文件筛查结果 */
export interface DuplicateScan {
  /** 本次实际参与筛查的文件数（已按路径去重） */
  scanned: number;
  groups: DuplicateGroup[];
  /** 读取失败、未能参与判定的文件 */
  unreadable: UnreadableFile[];
  /** 递归过程中读不进去的子目录个数 */
  skippedDirs: number;
}

/** 一条备份记录 */
export interface BackupEntry {
  path: string;
  stem: string;
  op: string;
  /** 纳秒时间戳的十进制字符串；超出 JS 安全整数范围，故不用 number */
  createdNs: string;
  size: number;
}

/** 场景批次：一个批次对应一张贴图 */
export interface SceneBatch {
  texture: number;
  positions: number[];
  uvs: number[];
  /** 每顶点颜色系数，RGB 三通道，已含明暗与染色 */
  colors: number[];
  /** 贴图透明类型。决定前端如何绘制该批次 */
  alphaMode: "opaque" | "cutout" | "translucent";
  indices: number[];
  textureWidth: number;
  textureHeight: number;
  /** RGBA8888 像素 */
  textureRgba: number[];
}

/** 前端渲染所需的完整场景 */
export interface SceneData {
  batches: SceneBatch[];
  totalVertices: number;
  totalTriangles: number;
  enclosing: [number, number, number];
  origin: [number, number, number];
  mcVersionName: string;
  dataVersion: number;
  /** 该版本方块库中查不到定义的方块种类数 */
  unresolvedBlocks: number;
}

/** 保存操作的结果 */
export interface SaveOutcome {
  path: string;
  /** 保存前生成的备份文件位置 */
  backupPath: string;
}

export interface RegionSummary {
  name: string;
  position: [number, number, number];
  size: [number, number, number];
}

export interface AnalysisResult {
  /** 按文件中的实际 Regions 返回，供子区列表与预览选择 */
  regions: RegionSummary[];
  blocks: BlockCount[];
  entities: EntityCount[];
  tileEntities: EntityCount[];
  /**
   * 逐个实体的 id 与投影内坐标。
   *
   * 名字只在 `entities` 里放一份：同一实体可能有上千个实例，
   * 逐条重复中英文名会让结果体积成倍增长。
   */
  entityInstances: EntityInstance[];
  nonAirTotal: number;
  airTotal: number;
  distinctBlockTypes: number;
  distinctStates: number;
}

// ---------------------------------------------------------------- 版本库

/** 基础库里一个已就绪的版本 */
export interface BaseVersion {
  name: string;
  dataVersion: number;
}

/** 基础库的当前状态，前端据此决定是否弹出「版本库」界面 */
export interface BaseDbStatus {
  versions: BaseVersion[];
  /** 实体贴图库是否已生成 */
  entityAvailable: boolean;
  /** 支持补齐的全部版本（后端支持表，前端不另抄一份） */
  supported: BaseVersion[];
}

/** 扫描 .minecraft 得到的已装版本 */
export interface McVersion {
  id: string;
  dataVersion: number;
  jarPath: string;
}

/** .minecraft 扫描结果 */
export interface McScan {
  versions: McVersion[];
  /** 存在客户端 jar 但读不出版本信息的目录，逐条带原因 */
  warnings: string[];
}

/** 版本下载来源 */
export type SetupSource = "official" | "bmcl";

/** 「版本库」里要准备的一个版本；jarPath 为 null 表示按来源下载 */
export interface SetupVersion {
  id: string;
  dataVersion: number;
  jarPath: string | null;
  /** 本机 .minecraft 目录：后端从这里补语言文件（中文不在客户端 jar 里） */
  mcDir?: string | null;
}

/** 单个版本的准备结果 */
export interface SetupVersionOutcome {
  id: string;
  ok: boolean;
  error: string;
  /** 成功但需要提醒的事，取值为后端定义的提示码（如 `lang-unavailable`） */
  warning?: string | null;
}

/** 一批版本的准备结果 */
export interface SetupOutcome {
  outcomes: SetupVersionOutcome[];
}

/** 「版本库」进度事件（base-setup-progress）的有效载荷 */
export interface BaseSetupProgress {
  id: string;
  stage: "download" | "build" | "done";
  received: number;
  total: number;
  error: string;
}

/** 一次更新检测的结果。
 *
 * `latest` 与 `tag` 在仓库里查不到可解析的发布时为空串，此时 `hasUpdate` 为
 * false：界面上「没有新版本」与「没查到发布」要分开说，不能混为一谈。
 */
export interface UpdateInfo {
  /** 当前可执行文件的版本号 */
  current: string;
  /** 最新发布的版本号（去掉前导 v） */
  latest: string;
  /** 最新发布的原始 tag，如 v0.0.2-dev.6 */
  tag: string;
  hasUpdate: boolean;
  /** 该发布的页面地址；查不到发布时为空串 */
  releaseUrl: string;
  /** 发布时间（ISO 8601）；查不到时为空串 */
  publishedAt: string;
  /** 检测目标仓库，如 heiheids/schematic-manager */
  repo: string;
  /** 当前版本是否为预览版（版本号带 `-` 后缀） */
  currentPrerelease: boolean;
  /** 最新发布是否为预览版。界面据此标出性质，由用户自己决定要不要更新 */
  latestPrerelease: boolean;
}
