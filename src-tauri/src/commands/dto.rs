//! 前后端交换的数据结构。
//!
//! 这里刻意与内部的 litematic 模型分开：内部模型会随解析需求演进，
//! 而这里的字段是对前端的稳定契约，字段名采用前端惯用的驼峰命名。

use serde::{Deserialize, Serialize};

/// 投影文件的摘要信息（列表展示用，不解码方块）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchematicSummary {
    pub path: String,
    pub file_name: String,
    pub dir_name: String,
    pub file_size: u64,
    /// 解析失败时的原因；为空表示有效
    pub error: String,

    pub format_version: i32,
    pub data_version: i32,
    pub mc_version_name: String,

    pub name: String,
    pub author: String,
    pub description: String,
    pub time_created: i64,
    pub time_modified: i64,

    pub enclosing: [i32; 3],
    pub total_blocks: i64,
    pub total_volume: i64,
    pub region_count: i32,
    pub entity_count: i64,
    pub tile_entity_count: i64,
    pub pending_ticks: i64,
    pub has_preview: bool,
}

/// 某个方块的统计条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockCount {
    pub name: String,
    pub state: String,
    /// 堆叠数（按 64 一组换算后的可读形式另由前端处理）
    pub count: u64,
    /// 中文名，取自方块库语言表；查不到时为空串
    pub zh_name: String,
    /// 英文名，取自方块库语言表；查不到时为空串
    pub en_name: String,
}

/// 实体统计条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityCount {
    pub id: String,
    pub count: u64,
    /// 中文名，取自方块库语言表；查不到时为空串
    pub zh_name: String,
    /// 英文名，取自方块库语言表；查不到时为空串
    pub en_name: String,
}

/// 实体贴图库的可用情况
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityTextureStatusDto {
    pub available: bool,
    pub versions: usize,
    pub textures: usize,
    pub paths: usize,
    /// 不可用时的原因；可用时为空串
    pub error: String,
}

/// 一张实体贴图的像素
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityTextureDto {
    pub path: String,
    pub width: u16,
    pub height: u16,
    pub has_alpha: bool,
    /// RGBA8888 像素，长度恒为 width * height * 4
    pub rgba: Vec<u8>,
}

/// 一个实体实例（id + 投影内坐标）。
///
/// 名字不放在这里、只在 `EntityCount` 上放一份：同一个实体可能有上千个实例，
/// 逐条重复中英文名会让结果体积成倍增长，而名字按 id 查表即可。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityInstanceDto {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// 扫描目录树中的一个节点。
///
/// 文件路径只挂在直接包含它们的目录上，前端展开子目录时不必从祖先节点
/// 重复过滤整棵树的文件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryNode {
    pub path: String,
    pub name: String,
    /// 相对于扫描根目录的路径，由 `strip_prefix` 计算；根节点为空串。
    pub relative_path: String,
    /// 当前目录直接包含的 `.litematic` 文件，不递归混入后代文件。
    pub file_paths: Vec<String>,
    /// 当前目录的直接子目录，包括没有投影文件的目录。
    pub children: Vec<DirectoryNode>,
    /// 到达递归深度上限时为真，此时未读取当前目录的内容。
    pub truncated: bool,
}

/// 目录扫描结果
///
/// 带上 skipped：不可读的子目录会被跳过，若不告知用户，
/// 结果看起来是完整的，用户会以为文件真的不存在。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub items: Vec<SchematicSummary>,
    /// 因权限或 IO 错误被跳过的目录数
    pub skipped_dirs: u32,
    /// 被跳过的目录路径（最多记录前若干个，便于用户定位）
    pub skipped_paths: Vec<String>,
    pub root: DirectoryNode,
}

/// 方块替换规则
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceRuleDto {
    pub from_name: String,
    #[serde(default)]
    pub from_props: Vec<(String, String)>,
    pub to_name: String,
    #[serde(default)]
    pub to_props: Vec<(String, String)>,
    #[serde(default = "default_true")]
    pub inherit_props: bool,
}

fn default_true() -> bool {
    true
}

impl From<ReplaceRuleDto> for crate::schematic::replace::ReplaceRule {
    fn from(d: ReplaceRuleDto) -> Self {
        Self {
            from_name: d.from_name,
            from_props: d.from_props,
            to_name: d.to_name,
            to_props: d.to_props,
            inherit_props: d.inherit_props,
        }
    }
}

/// 方块替换结果
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceOutcomeDto {
    pub replaced: u64,
    pub palette_changed: usize,
}

impl From<crate::schematic::replace::ReplaceOutcome> for ReplaceOutcomeDto {
    fn from(o: crate::schematic::replace::ReplaceOutcome) -> Self {
        Self { replaced: o.replaced, palette_changed: o.palette_changed }
    }
}

/// 方块检索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockHitDto {
    pub name: String,
    pub zh: String,
    pub en: String,
    pub versions: Vec<BlockVersionDto>,
}

/// 方块所在的版本
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockVersionDto {
    pub index: usize,
    pub data_version: i32,
}

/// 版本选项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionOptionDto {
    pub index: usize,
    pub name: String,
    pub data_version: i32,
    pub is_overlay: bool,
    pub block_count: usize,
}

/// 转换中需要处理的一项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertItemDto {
    pub name: String,
    /// 源方块的中英两个名字。界面上凡是出现方块 ID 的地方都要一并给出名字，
    /// 只给 ID 时用户无法确认自己要替换的到底是哪一个方块；而界面语言可以切换，
    /// 所以两套都给，由界面按当前语言取一套、缺哪套退另一套。
    ///
    /// 由后端给出而不是前端逐个回查：方块库就在手里，一次会话里几十条
    /// 逐个走 IPC 查询既慢又没必要。
    pub name_zh: String,
    pub name_en: String,
    pub count: u64,
    pub suggested: String,
    /// 推荐替代品的中文名；没有推荐时为空
    pub suggested_zh: String,
    /// 推荐替代品的英文名；没有推荐时为空
    pub suggested_en: String,
    pub chosen: String,
}

/// 可作为转换目标的版本
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertTargetDto {
    pub index: usize,
    pub name: String,
    pub data_version: i32,
}

/// 转换结果
// 不能派生 Copy：该结构体含 String 字段，Copy 无法实现。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertOutcomeDto {
    pub replaced_kinds: usize,
    pub replaced_blocks: u64,
    pub skipped_kinds: usize,
    /// 新文件的路径
    pub output_path: String,
}

/// 一组内容相同的投影文件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroupDto {
    pub size: u64,
    pub paths: Vec<String>,
}

/// 重复文件筛查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateScanDto {
    /// 本次实际参与判定的文件数（已按路径去重）。
    ///
    /// 必须给出：结果是「筛查了 108 个文件，没有重复」还是「一个文件都没扫到」
    /// 对用户含义完全不同，只报 groups 为空时两者在界面上无法区分。
    /// 读取失败、没能参与判定的文件不计入这里，它们在 unreadable 里单独上报。
    pub scanned: usize,
    pub groups: Vec<DuplicateGroupDto>,
    /// 读取失败、未能参与判定的文件及原因
    pub unreadable: Vec<UnreadableFileDto>,
    /// 递归过程中读不进去的子目录个数。
    ///
    /// 必须上报：这些目录里的文件没有参与筛查，据结果删副本就可能漏掉
    /// 真正的重复项。只给个数，路径在目录加载时已经逐条列给用户了。
    pub skipped_dirs: u32,
}

/// 未能读取的文件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadableFileDto {
    pub path: String,
    pub reason: String,
}

/// 一条备份记录
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntryDto {
    pub path: String,
    pub stem: String,
    pub op: String,
    /// 纳秒时间戳，以十进制字符串传递。
    /// 纳秒量级约 1.8e18，超出 JavaScript 安全整数范围（约 9e15），
    /// 用数值传递会丢精度，因此这里用字符串。
    pub created_ns: String,
    pub size: u64,
}

/// 重命名投影文件的结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameOutcomeDto {
    /// 重命名后的完整路径
    pub path: String,
    /// 重命名后的文件名
    pub file_name: String,
    /// 被重新链接到新名字的备份分组数
    pub relinked: usize,
    /// 备份重新链接失败的原因。
    ///
    /// 与 relinked 分开而不是让整个命令失败：文件本身已经改名成功，
    /// 若这时返回错误，界面会以为改名没发生，显示的状态与磁盘不符。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relink_error: Option<String>,
}

/// 场景批次：供前端一个材质使用
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneBatchDto {
    pub texture: u32,
    pub positions: Vec<f32>,
    pub uvs: Vec<f32>,
    pub colors: Vec<f32>,
    pub indices: Vec<u32>,
    pub texture_width: u16,
    pub texture_height: u16,
    /// RGBA8888 像素；前端用 DataTexture 直接建纹理
    pub texture_rgba: Vec<u8>,
    /// 贴图透明类型："opaque" / "cutout" / "translucent"
    pub alpha_mode: String,
}

/// 前端渲染所需的完整场景
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneDto {
    pub batches: Vec<SceneBatchDto>,
    pub total_vertices: usize,
    pub total_triangles: usize,
    pub enclosing: [i32; 3],
    pub origin: [i32; 3],
    pub mc_version_name: String,
    pub data_version: i32,
    pub unresolved_blocks: usize,
}

impl From<crate::render::scene::SceneData> for SceneDto {
    fn from(s: crate::render::scene::SceneData) -> Self {
        Self {
            batches: s
                .batches
                .into_iter()
                .map(|b| SceneBatchDto {
                    texture: b.texture,
                    positions: b.positions,
                    uvs: b.uvs,
                    colors: b.colors,
                    indices: b.indices,
                    texture_width: b.texture_width,
                    texture_height: b.texture_height,
                    texture_rgba: b.texture_rgba,
            alpha_mode: match b.alpha_mode {
                crate::render::scene::AlphaMode::Opaque => "opaque",
                crate::render::scene::AlphaMode::Cutout => "cutout",
                crate::render::scene::AlphaMode::Translucent => "translucent",
            }
            .to_string(),
                })
                .collect(),
            total_vertices: s.total_vertices,
            total_triangles: s.total_triangles,
            enclosing: s.enclosing,
            origin: s.origin,
            mc_version_name: s.mc_version_name,
            data_version: s.data_version,
            unresolved_blocks: s.unresolved_blocks,
        }
    }
}

/// 保存操作的结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOutcome {
    pub path: String,
    /// 保存前生成的备份文件位置
    pub backup_path: String,
}

/// 批量文件操作的结果。
///
/// 带上实际处理的文件名与（复制时的）目标路径：用户需要看到
/// 「到底动了哪些文件、复制到哪儿了」，只回一句「成功」无法核对。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOpOutcome {
    /// 实际处理的文件名
    pub names: Vec<String>,
    /// 复制操作写入的目标路径；删除时为空
    pub written: Vec<String>,
    /// 目标目录（复制时）
    pub dest_dir: String,
    /// 处理的文件总字节数。界面用它在提示里给出「共多少 MB」，
    /// 让用户能核对复制的确实是预期的那批文件。
    pub total_bytes: u64,
}

/// 投影中一个区域的名称、归一化位置和尺寸。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionSummary {
    pub name: String,
    pub position: [i32; 3],
    pub size: [i32; 3],
}

/// 完整分析结果
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    pub blocks: Vec<BlockCount>,
    pub entities: Vec<EntityCount>,
    pub tile_entities: Vec<EntityCount>,
    pub regions: Vec<RegionSummary>,
    /// 逐个实体的 id 与坐标，供以后做实体预览
    pub entity_instances: Vec<EntityInstanceDto>,
    pub non_air_total: u64,
    pub air_total: u64,
    pub distinct_block_types: usize,
    pub distinct_states: usize,
}

/// 一个方块的渲染图。
///
/// 逐项带上 `state` 而不是按数组下标对齐：调用方一次可能只请求其中一部分，
/// 只靠下标迟早会把图片贴到错误的行上。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockIconDto {
    /// 方块状态键，与请求里的一致，形如 `minecraft:oak_stairs[facing=east]`
    pub state: String,
    /// `data:image/png;base64,...`；渲染失败时为空串
    pub data_url: String,
    /// 渲染失败的原因；成功时为空串。
    /// 必须逐项回报：某个方块画不出来不该让整批失败，但用户需要知道为什么。
    pub error: String,
}

/// 基础库里一个已就绪的版本（方块库 + 该版本是否带实体贴图由整体开关看）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseVersionDto {
    pub name: String,
    pub data_version: i32,
}

/// 基础库的当前状态，前端据此决定是否弹出「版本库」界面
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseDbStatusDto {
    /// 已就绪的版本，按数据版本升序
    pub versions: Vec<BaseVersionDto>,
    /// 实体贴图库是否已生成（还没有任何版本时就是没有）
    pub entity_available: bool,
    /// 支持的全部版本。「支持哪些」只有后端这张表说了算，前端拿它
    /// 对照就绪/已装来列缺失项，不在前端另抄一份
    pub supported: Vec<BaseVersionDto>,
}

/// 扫描 .minecraft 得到的已装版本
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McVersionDto {
    pub id: String,
    pub data_version: i32,
    pub jar_path: String,
}

/// .minecraft 扫描结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McScanDto {
    pub versions: Vec<McVersionDto>,
    /// 存在客户端 jar 但读不出版本信息的目录；必须逐条带回原因，
    /// 否则「明明装了却不显示」无从解释
    pub warnings: Vec<String>,
}

/// 版本下载来源。官方 API 在国内常不通，BMCL 是镜像，因此两个都要留。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SetupSource {
    Official,
    Bmcl,
}

/// 「版本库」里要准备的一个版本。
///
/// `jar_path` 为空表示需要按 source 下载；有值表示直接用这个本地 jar。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupVersionDto {
    pub id: String,
    pub data_version: i32,
    pub jar_path: Option<String>,
    /// 本机 .minecraft 目录：构建时要从这里补语言文件（中文在资源库里，
    /// 不在客户端 jar 内）；为空则按官方默认位置找。
    #[serde(default)]
    pub mc_dir: Option<String>,
}

/// 单个版本的准备结果。逐版本回报而不是整批成败：
/// 一个版本下载失败不该让其它版本白跑一遍。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupVersionOutcomeDto {
    pub id: String,
    pub ok: bool,
    /// 失败原因；成功时为空串
    pub error: String,
    /// 成功但需要提醒用户的事，取值为稳定的提示码，由前端出对应语言的说明。
    /// 目前只有 `lang-unavailable`：没取到语言表，界面会显示英文名。
    ///
    /// 给码而不是给句子：本项目的界面支持中英切换，后端出中文句子会让英文
    /// 界面里冒出中文。
    #[serde(default)]
    pub warning: Option<String>,
}

/// 一批版本的准备结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupOutcomeDto {
    pub outcomes: Vec<SetupVersionOutcomeDto>,
}

/// 一次更新检测的结果。
///
/// `latest` 与 `tag` 在查不到任何可解析的发布时为空串，此时 `hasUpdate` 为 false：
/// 「查不到」与「有新版本」必须分清，否则网络异常会被说成有更新。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfoDto {
    /// 当前可执行文件的版本号
    pub current: String,
    /// 最新发布的版本号（去掉前导 v）
    pub latest: String,
    /// 最新发布的原始 tag
    pub tag: String,
    pub has_update: bool,
    /// 该发布的页面地址
    pub release_url: String,
    pub published_at: String,
    /// 检测目标仓库，界面直接展示
    pub repo: String,
    /// 当前版本是否为预览版（版本号带 `-` 后缀）
    pub current_prerelease: bool,
    /// 最新发布是否为预览版。界面据此标出性质，由用户自己决定要不要更新
    pub latest_prerelease: bool,
}

#[cfg(test)]
#[path = "../tests/commands/dto.rs"]
mod tests;
