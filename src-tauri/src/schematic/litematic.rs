//! .litematic 投影文件模型。
//!
//! 对照 legacy-cpp/litematic.cpp 实现，保留其已验证语义：
//!   - 完整保留解析后的 NBT 树（root），未修改的字段原样写回，零信息损失
//!   - BlockStates 采用「跨 long 连续打包、低位在前」，与 Litematica 一致
//!   - 区域 Size 各分量可能为负（表示向负方向延伸），加载时归一化
//!   - 索引顺序为 X 最快、Z 次之、Y 最慢
//!   - 兼容 v6 的 BlockStatePalette 与 v7 的 Palette
//!
//! 参考格式：
//!   root
//!     MinecraftDataVersion: Int
//!     Version: Int            投影格式版本（v5/v6/v7）
//!     SubVersion: Int
//!     Metadata: Compound { Name, Author, Description, TimeCreated, TimeModified,
//!                          EnclosingSize: {x,y,z}, RegionCount, TotalBlocks, ... }
//!     Regions: Compound { <区域名>: Region }

use crate::error::{AppError, AppResult};
use crate::format::nbt::{self, Value};
use std::collections::BTreeMap;

/// 单个区域的方块数上限。
///
/// 体积来自文件里的 Size 三个分量，是纯粹不可信的外部输入：不设上界时
/// 三个 i32::MAX 相乘会溢出（debug panic、release 回绕成任意值），
/// 回绕结果还会被用来分配 indices（每个方块 4 字节）。
///
/// 取 5000 万：已远超真实投影的量级（最大规模的工程也用不到千万级），
/// 同时把单次分配限制在约 200 MB，不会因一个畸形文件耗尽内存。
const MAX_REGION_VOLUME: usize = 50_000_000;

/// 方块状态：名称 + 属性（保持顺序，序列化时顺序稳定）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlockState {
    pub name: String,
    pub props: Vec<(String, String)>,
}

impl BlockState {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), props: Vec::new() }
    }

    /// "facing=east,half=bottom"
    pub fn prop_str(&self) -> String {
        self.props
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// "minecraft:oak_stairs[facing=east,half=bottom]"
    pub fn key(&self) -> String {
        let ps = self.prop_str();
        if ps.is_empty() {
            self.name.clone()
        } else {
            format!("{}[{}]", self.name, ps)
        }
    }

    pub fn prop(&self, k: &str) -> Option<&str> {
        self.props.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.as_str())
    }

    pub fn set_prop(&mut self, k: &str, v: &str) {
        if let Some(slot) = self.props.iter_mut().find(|(pk, _)| pk == k) {
            slot.1 = v.to_string();
            return;
        }
        // 属性按字典序排列，与游戏内的规范顺序一致
        self.props.push((k.to_string(), v.to_string()));
        self.props.sort_by(|a, b| a.0.cmp(&b.0));
    }

    /// 是否为空气，用于统计。
    ///
    /// 不含 structure_void：legacy-cpp 统计时只把 air / cave_air / void_air
    /// 计入空气（litematic.cpp:374-377），structure_void 算非空气。
    /// 而渲染时会跳过 structure_void（render.cpp:317）。
    /// 两处口径不同是有意的，因此这里不叫「渲染是否跳过」。
    pub fn is_air(&self) -> bool {
        matches!(
            self.name.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    }
}

/// 单个区域
#[derive(Debug, Clone, Default)]
pub struct Region {
    pub name: String,
    pub pos: [i32; 3],
    pub size: [i32; 3],
    pub volume: usize,
    pub bits: i32,
    pub palette: Vec<BlockState>,
    /// 每个方块的调色板索引，长度 = volume
    pub indices: Vec<i32>,
    pub entity_count: usize,
    pub tile_entity_count: usize,
    pub pending_block_ticks: usize,
    pub pending_fluid_ticks: usize,
}

impl Region {
    /// 线性索引：X 最快、Z 次之、Y 最慢
    pub fn index(&self, x: i32, y: i32, z: i32) -> usize {
        let sx = self.size[0] as usize;
        let sz = self.size[2] as usize;
        x as usize + z as usize * sx + y as usize * sx * sz
    }

    pub fn pos_from_index(&self, i: usize) -> [i32; 3] {
        let (sx, sz) = (self.size[0] as usize, self.size[2] as usize);
        if sx == 0 || sz == 0 {
            return [0, 0, 0];
        }
        let x = i % sx;
        let r = i / sx;
        let z = r % sz;
        let y = r / sz;
        [x as i32, y as i32, z as i32]
    }

    pub fn inside(&self, x: i32, y: i32, z: i32) -> bool {
        x >= 0 && y >= 0 && z >= 0 && x < self.size[0] && y < self.size[1] && z < self.size[2]
    }
}

// ---------------------------------------------------------------- 位打包

/// 读取跨 long 连续打包的第 index 个值。
///
/// Litematica 的打包方式是「整体连续」而非「按 long 对齐」：
/// 第 index 个值从比特位 index*bits 开始，可跨越 long 边界。
pub fn read_bits(arr: &[i64], index: usize, bits: i32) -> i32 {
    if bits <= 0 || bits > 32 {
        return 0;
    }
    let mask: u64 = if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 };
    let start_offset = match index.checked_mul(bits as usize) {
        Some(v) => v,
        None => return 0,
    };
    let start_arr = start_offset >> 6;
    let end_arr = ((index + 1) * bits as usize - 1) >> 6;
    let start_bit = (start_offset & 63) as u32;

    if start_arr >= arr.len() {
        return 0;
    }
    if start_arr == end_arr {
        return ((arr[start_arr] as u64) >> start_bit & mask) as i32;
    }
    if end_arr >= arr.len() {
        return 0;
    }
    let end_offset = 64 - start_bit;
    let mut v = (arr[start_arr] as u64) >> start_bit;
    v |= (arr[end_arr] as u64) << end_offset;
    (v & mask) as i32
}

/// 写入跨 long 连续打包的第 index 个值（只覆盖该值占用的比特位）
pub fn write_bits(arr: &mut [i64], index: usize, bits: i32, value: i32) {
    if bits <= 0 || bits > 32 {
        return;
    }
    let mask: u64 = if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 };
    let start_offset = index * bits as usize;
    let start_arr = start_offset >> 6;
    let end_arr = ((index + 1) * bits as usize - 1) >> 6;
    let start_bit = (start_offset & 63) as u32;
    let v = (value as u64) & mask;

    if start_arr >= arr.len() {
        return;
    }
    if start_arr == end_arr {
        arr[start_arr] = ((arr[start_arr] as u64 & !(mask << start_bit)) | (v << start_bit)) as i64;
        return;
    }
    if end_arr >= arr.len() {
        return;
    }
    let end_offset = 64 - start_bit;
    arr[start_arr] = ((arr[start_arr] as u64 & !(mask << start_bit)) | (v << start_bit)) as i64;
    arr[end_arr] = ((arr[end_arr] as u64 & !(mask >> end_offset)) | (v >> end_offset)) as i64;
}

/// 调色板大小所需的比特数。Litematica 最少用 2 位。
pub fn bits_for_palette(palette_size: usize) -> i32 {
    if palette_size <= 2 {
        return 2;
    }
    // ceil(log2(n))
    let mut bits = 2;
    while (1usize << bits) < palette_size {
        bits += 1;
    }
    bits as i32
}

/// 把索引数组编码成跨 long 打包的数组
/// 把调色板编码成 NBT 列表。
///
/// 每项为 Compound：Name 必填；Properties 为 属性名 -> 属性值，
/// 无属性时不写该字段——与 legacy-cpp 的写法一致，
/// 多写一个空 Compound 虽不影响读取，但会让文件与原版有差异。
pub fn encode_palette(palette: &[BlockState]) -> Value {
    Value::List(
        palette
            .iter()
            .map(|bs| {
                let mut fields = vec![("Name".to_string(), Value::String(bs.name.clone()))];
                if !bs.props.is_empty() {
                    fields.push((
                        "Properties".to_string(),
                        Value::Compound(
                            bs.props
                                .iter()
                                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                                .collect(),
                        ),
                    ));
                }
                // 元素类型必须是 Compound，读取端依赖它
                Value::Compound(fields)
            })
            .collect(),
        10, // TAG_Compound
    )
}

pub fn encode_indices(indices: &[i32], bits: i32) -> Vec<i64> {
    if bits <= 0 || indices.is_empty() {
        return Vec::new();
    }
    let total_bits = indices.len() * bits as usize;
    let mut arr = vec![0i64; total_bits.div_ceil(64)];
    for (i, v) in indices.iter().enumerate() {
        write_bits(&mut arr, i, bits, *v);
    }
    arr
}

/// 把跨 long 打包的数组解码成索引数组
pub fn decode_indices(arr: &[i64], count: usize, bits: i32) -> Vec<i32> {
    (0..count).map(|i| read_bits(arr, i, bits)).collect()
}

// ---------------------------------------------------------------- 摘要

/// 只读元数据（用于列表快速扫描，不解码方块）
#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub error: String,
    pub path: String,
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
    pub file_size: u64,
}

/// 一个实体实例的位置。
///
/// 文件里的 `Pos` 是相对**所属区域原点**的偏移（与方块坐标同一套约定），
/// 因此这里存的是加上区域位置之后的投影内坐标，供预览直接使用。
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedEntity {
    pub id: String,
    pub pos: [f64; 3],
}

/// 完整分析结果
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    /// 按完整状态统计（含属性）
    pub blocks_by_state: BTreeMap<String, u64>,
    pub entities_by_id: BTreeMap<String, u64>,
    pub tile_entities_by_id: BTreeMap<String, u64>,
    /// 逐个实体的 id 与坐标。以后做实体预览要用，现在先读出来存好。
    pub entities: Vec<PlacedEntity>,
    pub non_air_total: u64,
    pub air_total: u64,
}

// ---------------------------------------------------------------- 载入

/// 从 NBT 根节点构造 Summary（不解析方块数据）
pub fn summary_from_nbt(root: &Value) -> Summary {
    let mut s = Summary::default();
    s.format_version = root.get("Version").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    s.data_version = root
        .get("MinecraftDataVersion")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    s.mc_version_name = crate::mc::ver::version_name_from_data_version(s.data_version);

    if let Some(md) = root.get("Metadata") {
        let gs = |k: &str| md.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        s.name = gs("Name");
        s.author = gs("Author");
        s.description = gs("Description");
        s.time_created = md.get("TimeCreated").and_then(|v| v.as_i64()).unwrap_or(0);
        s.time_modified = md.get("TimeModified").and_then(|v| v.as_i64()).unwrap_or(0);
        s.total_blocks = md.get("TotalBlocks").and_then(|v| v.as_i64()).unwrap_or(0);
        s.total_volume = md.get("TotalVolume").and_then(|v| v.as_i64()).unwrap_or(0);
        s.region_count = md.get("RegionCount").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        // 实体/方块实体/待处理刻**不在 Metadata 里**：实测真实投影的 Metadata
        // 只有 TimeCreated、TimeModified、EnclosingSize、Description、
        // RegionCount、TotalBlocks、TotalVolume、Author、Name 这几项。
        // 这些数量必须按区域内的列表求和（legacy-cpp 的 Peek 也是这么做的，
        // 见 legacy-cpp/src/litematic.cpp:274-284）。此前读 Metadata 的同名键，
        // 一个都取不到，界面上「实体数量 / 方块实体 / 待处理刻」恒为 0。
        if let Some(es) = md.get("EnclosingSize") {
            if let Some(xyz) = es.as_xyz() {
                s.enclosing = xyz;
            }
        }
        if md.get("PreviewImageData").is_some() {
            s.has_preview = true;
        }
    }

    // 按区域内的列表统计数量。RegionCount 与实体数量都以实际内容为准：
    // Metadata 里可能缺失这些字段，而文件本身是有内容的。
    if let Some(Value::Compound(regs)) = root.get("Regions") {
        if s.region_count == 0 {
            s.region_count = regs.len() as i32;
        }
        for (_, rv) in regs.iter() {
            let len = |k: &str| -> i64 {
                rv.get(k).and_then(|x| x.as_list()).map_or(0, |l| l.len() as i64)
            };
            s.entity_count += len("Entities");
            s.tile_entity_count += len("TileEntities");
            s.pending_ticks += len("PendingBlockTicks") + len("PendingFluidTicks");
        }
    }
    s
}

/// 解析单个区域
/// 由区域尺寸算出方块数，并校验不会溢出、不超过上限。
///
/// 单独成函数是为了能直接测试边界：区域解析到这一步已经需要构造完整的
/// NBT 与足够长的 BlockStates，而这里只需三个整数。
///
/// 调用方必须先确认三个分量均为正（负值表示向负方向延伸，会先被归一化）。
fn region_volume(size: [i32; 3], name: &str) -> AppResult<usize> {
    let mut volume: usize = 1;
    for a in 0..3 {
        volume = volume
            .checked_mul(size[a] as usize)
            .ok_or_else(|| AppError::Corrupt(format!("区域 {name} 的尺寸过大，体积计算溢出")))?;
    }
    if volume > MAX_REGION_VOLUME {
        return Err(AppError::Corrupt(format!(
            "区域 {name} 的体积为 {volume}，超过上限 {MAX_REGION_VOLUME}（约合 {} MB 的方块索引）",
            MAX_REGION_VOLUME * 4 / 1024 / 1024
        )));
    }
    Ok(volume)
}

fn region_from_nbt(name: &str, v: &Value) -> AppResult<Region> {
    let mut r = Region { name: name.to_string(), ..Default::default() };

    let pos = v
        .get("Position")
        .and_then(|p| p.as_xyz())
        .ok_or_else(|| AppError::Parse(format!("区域 {name} 缺少 Position")))?;
    let mut size = v
        .get("Size")
        .and_then(|p| p.as_xyz())
        .ok_or_else(|| AppError::Parse(format!("区域 {name} 缺少 Size")))?;

    // Size 分量可能为负：表示向负方向延伸。
    // 此时原点要相应前移，尺寸取绝对值，否则后续索引计算全部错位。
    for a in 0..3 {
        if size[a] < 0 {
            // 取绝对值必须检查：i32::MIN 的相反数无法用 i32 表示。
            // 不检查的话，debug 构建会 panic，release 构建回绕后仍是负数，
            // 于是 r.pos 已被写成错位的值，区域却以「尺寸为 0」被放行，
            // 后续按 r.pos 计算场景原点就会把整个预览框到别处去。
            //
            // 顺序很重要：必须先用**原始负值**算出前移后的原点，再取绝对值。
            // 反过来会把正的尺寸当成偏移量，原点被推远一个尺寸（实测
            // Size.x=-16、Position.x=15 时得到 32 而非 0）。
            let new_pos = pos[a].wrapping_add(size[a]).wrapping_add(1);
            size[a] = size[a]
                .checked_neg()
                .ok_or_else(|| AppError::Corrupt(format!("区域 {name} 的 Size 分量无法取绝对值")))?;
            r.pos[a] = new_pos;
        } else {
            r.pos[a] = pos[a];
        }
    }
    r.size = size;

    // 尺寸为 0 的区域没有方块数据
    if size.iter().any(|&s| s <= 0) {
        return Ok(r);
    }

    // 体积必须用 checked 运算并设上界。
    // 三个分量各自可达 i32::MAX，直接相乘会溢出：debug 构建 panic，
    // release 构建回绕成一个任意值，随后被用来分配 indices
    // （每个方块 4 字节），既可能给出错误结果也可能耗尽内存。
    // 该值只受文件内容控制，因此必须当成不可信输入处理。
    r.volume = region_volume(size, name)?;

    // v7 用 Palette，v6 用 BlockStatePalette
    let pal_val = v
        .get("BlockStatePalette")
        .or_else(|| v.get("Palette"))
        .ok_or_else(|| AppError::Parse(format!("区域 {name} 缺少调色板")))?;
    let pal_list = pal_val
        .as_list()
        .ok_or_else(|| AppError::Parse(format!("区域 {name} 的调色板不是列表")))?;
    for item in pal_list {
        let n = item.get("Name").and_then(|x| x.as_str()).unwrap_or("minecraft:air");
        let mut bs = BlockState::new(n);
        if let Some(Value::Compound(props)) = item.get("Properties") {
            for (k, pv) in props {
                if let Some(val) = pv.as_str() {
                    bs.props.push((k.clone(), val.to_string()));
                }
            }
            bs.props.sort_by(|a, b| a.0.cmp(&b.0));
        }
        r.palette.push(bs);
    }

    r.bits = bits_for_palette(r.palette.len());

    // 解码方块索引。
    // 必须先用「文件声明的 Size」换算出所需 long 数并校验：
    // BlockStates 数组偏短时 read_bits 会一律返回 0，而索引 0 通常是空气，
    // 于是统计结果静默偏小——用户会看到一个「看起来正常但数字是错的」投影。
    if let Some(longs) = v.get("BlockStates").and_then(|x| x.as_longs()) {
        // volume 与 bits 都在上面受过约束（bits 最大 32），这里不会溢出
        let need = (r.volume * r.bits as usize).div_ceil(64);
        if longs.len() < need {
            return Err(AppError::Corrupt(format!(
                "区域 {name} 的 BlockStates 长度不足：按尺寸与位数需要 {need} 个 long，实际 {}",
                longs.len()
            )));
        }
        r.indices = decode_indices(longs, r.volume, r.bits);
        // 越界索引视为空气，避免后续取值 panic
        let pal_len = r.palette.len() as i32;
        for i in r.indices.iter_mut() {
            if *i < 0 || *i >= pal_len {
                *i = 0;
            }
        }
    } else {
        r.indices = vec![0; r.volume];
    }

    r.entity_count = v.get("Entities").and_then(|x| x.as_list()).map_or(0, |l| l.len());
    r.tile_entity_count = v.get("TileEntities").and_then(|x| x.as_list()).map_or(0, |l| l.len());
    r.pending_block_ticks =
        v.get("PendingBlockTicks").and_then(|x| x.as_list()).map_or(0, |l| l.len());
    r.pending_fluid_ticks =
        v.get("PendingFluidTicks").and_then(|x| x.as_list()).map_or(0, |l| l.len());

    Ok(r)
}

/// 已载入的投影文件
pub struct Schematic {
    pub root: Value,
    pub regions: Vec<Region>,
    pub path: String,
    /// 元数据（名称/作者/描述/数据版本）被改动
    pub meta_modified: bool,
    /// 方块索引被改动。只有它为真时才需要重新编码 BlockStates，
    /// 否则改一个描述也会重写全部区域的方块数据。
    pub blocks_modified: bool,
}

impl Schematic {
    /// 从文件载入
    pub fn load(path: &str) -> AppResult<Self> {
        let bytes = std::fs::read(path)?;
        let root = nbt::parse_maybe_gzip(&bytes)?;
        let mut sc = Self {
            root,
            regions: Vec::new(),
            path: path.to_string(),
            meta_modified: false,
            blocks_modified: false,
        };
        sc.rebuild_regions()?;
        Ok(sc)
    }

    /// 只读元数据扫描（不解码方块，适合批量列表）
    pub fn peek(path: &str) -> Summary {
        let mut s = Summary { path: path.to_string(), ..Default::default() };
        match std::fs::read(path) {
            Ok(bytes) => {
                s.file_size = bytes.len() as u64;
                match nbt::parse_maybe_gzip(&bytes) {
                    Ok(root) => {
                        let mut inner = summary_from_nbt(&root);
                        inner.path = path.to_string();
                        inner.file_size = s.file_size;
                        return inner;
                    }
                    Err(e) => {
                        s.error = e.to_string();
                        return s;
                    }
                }
            }
            Err(e) => {
                s.error = e.to_string();
                s
            }
        }
    }

    fn rebuild_regions(&mut self) -> AppResult<()> {
        self.regions.clear();
        if let Some(Value::Compound(regs)) = self.root.get("Regions") {
            for (name, v) in regs {
                self.regions.push(region_from_nbt(name, v)?);
            }
        }
        Ok(())
    }

    pub fn summary(&self) -> Summary {
        let mut s = summary_from_nbt(&self.root);
        s.path = self.path.clone();
        s
    }

    /// 统计方块与实体
    pub fn analyze(&self) -> Analysis {
        let mut a = Analysis::default();
        for r in &self.regions {
            for idx in &r.indices {
                let Some(b) = r.palette.get(*idx as usize) else { continue };
                if b.is_air() {
                    a.air_total += 1;
                    continue;
                }
                a.non_air_total += 1;
                *a.blocks_by_state.entry(b.key()).or_insert(0) += 1;
            }
        }
        for r in &self.regions {
            if let Some(Value::Compound(regs)) = self.root.get("Regions") {
                if let Some(rv) = regs.iter().find(|(n, _)| n == &r.name).map(|(_, v)| v) {
                    for key in ["Entities", "TileEntities"] {
                        if let Some(list) = rv.get(key).and_then(|x| x.as_list()) {
                            for e in list {
                                // NBT 的键区分大小写，实体与方块实体的标识字段
                                // 都是小写 `id`（legacy-cpp 也取 "id"）。
                                // 写成 "Id" 会一个都匹配不到，全部归入 unknown，
                                // 界面上就只剩一行 unknown。
                                let id = e.get("id").and_then(|x| x.as_str()).unwrap_or("unknown");
                                let map = if key == "Entities" {
                                    &mut a.entities_by_id
                                } else {
                                    &mut a.tile_entities_by_id
                                };
                                *map.entry(id.to_string()).or_insert(0) += 1;

                                // 实体坐标：`Pos` 是相对区域原点的偏移，
                                // 加上区域位置才是投影内坐标。
                                // 方块实体的 `Pos` 是可选且常缺省，因此只记实体。
                                if key == "Entities" {
                                    let rel = e.get("Pos").and_then(|x| x.as_list());
                                    let comp = |i: usize| -> f64 {
                                        rel.and_then(|l| l.get(i))
                                            .and_then(|v| v.as_f64())
                                            .unwrap_or(0.0)
                                    };
                                    a.entities.push(PlacedEntity {
                                        id: id.to_string(),
                                        pos: [
                                            r.pos[0] as f64 + comp(0),
                                            r.pos[1] as f64 + comp(1),
                                            r.pos[2] as f64 + comp(2),
                                        ],
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        a
    }

    // --- 元数据编辑（保持 NBT 结构，仅改对应字段） ---

    fn meta_set_str(&mut self, key: &str, v: &str) -> bool {
        let Some(md) = self.root.get("Metadata") else { return false };
        let Some(old) = md.get(key) else { return false };
        // 值未变化时不算改动：否则重复保存相同内容也会触发备份与写盘
        if old.as_str() == Some(v) {
            return false;
        }
        let mut md = md.clone();
        md.set(key, Value::String(v.to_string()));
        self.root.set("Metadata", md);
        self.meta_modified = true;
        true
    }

    pub fn set_meta_name(&mut self, v: &str) -> bool {
        self.meta_set_str("Name", v)
    }
    pub fn set_meta_author(&mut self, v: &str) -> bool {
        self.meta_set_str("Author", v)
    }
    pub fn set_meta_description(&mut self, v: &str) -> bool {
        self.meta_set_str("Description", v)
    }
    /// 把编辑后的索引重新编码回 NBT 并保存
    pub fn save_to(&mut self, path: &str) -> AppResult<()> {
        self.sync_to_nbt()?;
        let bytes = nbt::serialize_gzip(&self.root, "", 6)?;
        // 先写临时文件再改名，避免中途失败留下半个文件
        let tmp = format!("{path}.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)?;
        self.meta_modified = false;
        self.blocks_modified = false;
        Ok(())
    }

    /// 把内存中的区域索引写回 NBT 的 BlockStates 字段
    fn sync_to_nbt(&mut self) -> AppResult<()> {
        // 只有方块数据真的被改动过才重新编码 BlockStates，
        // 否则改一个描述也会把全部区域的方块数据重写一遍。
        if !self.blocks_modified {
            return Ok(());
        }
        let region_names: Vec<String> = self.regions.iter().map(|r| r.name.clone()).collect();
        let mut regs = match self.root.get("Regions").cloned() {
            Some(Value::Compound(v)) => v,
            _ => {
                return Err(AppError::Parse(
                    "投影的 Regions 字段缺失或类型不正确，无法写回方块数据".into(),
                ))
            }
        };
        for name in region_names {
            let Some(mut r) = self.regions.iter().find(|r| r.name == name).cloned() else {
                continue;
            };
            if r.volume == 0 {
                continue;
            }
            let slot = regs.iter_mut().find(|(n, _)| n == &name).ok_or_else(|| {
                AppError::Parse(format!("NBT 中找不到区域 {name}，无法写回方块数据"))
            })?;

            // 调色板必须一并写回：替换会向 palette 追加新状态，
            // 只写索引的话，盘上的调色板仍是旧长度，新索引越界，
            // 游戏与 Litematica 会读到错误方块或直接判为损坏。
            //
            // 键名沿用文件原有的那个：v6 用 BlockStatePalette、v7 用 Palette。
            // 一律写 BlockStatePalette 会让 v7 文件多出一个字段而原字段仍是旧值。
            let pal_key = if slot.1.get("Palette").is_some() {
                "Palette"
            } else {
                "BlockStatePalette"
            };
            slot.1.set(pal_key, encode_palette(&r.palette));

            // 位宽也要按新的调色板长度重算：调色板增长后仍用旧位宽，
            // write_bits 会按掩码静默截断索引，写出的就是错数据。
            r.bits = bits_for_palette(r.palette.len());
            slot.1.set(
                "BlockStates",
                Value::LongArray(encode_indices(&r.indices, r.bits)),
            );
        }
        self.root.set("Regions", Value::Compound(regs));
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/schematic/litematic.rs"]
mod tests;
