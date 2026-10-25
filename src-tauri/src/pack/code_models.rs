//! 由程序代码描述的方块几何。
//!
//! 箱子、末影箱、头颅、床、旗帜、潜影盒、告示牌这些方块在客户端 jar 里只有
//! 一张「声明粒子贴图」的空壳模型（没有 parent、也没有 elements），真正的几何
//! 由游戏的方块实体渲染器用代码绘制。因此单靠 jar 里的数据显示不出它们。
//!
//! 这里用**本程序自己的代码**描述这些形状：盒子尺寸、朝向与 UV 偏移都由代码给出，
//! 贴图则一律指向用户自己下载的客户端 jar 里的 `entity/...` 贴图——程序本身
//! 不附带任何游戏资源文件，构建产物（含贴图）落在用户自己的应用数据目录。
//!
//! 朝向不烘焙进顶点，而是作为变体的 y 轴旋转写出，交给现有渲染器处理。

use crate::block::state::{ModelElementDef, ModelFace};

/// 一个状态变体：匹配条件、旋转（90 度倍数）、以及该状态的几何。
pub struct CodeVariant {
    pub when: String,
    pub rot_x_div90: u32,
    pub rot_y_div90: u32,
    pub elements: Vec<ModelElementDef>,
}

/// 六个面名，顺序与原版 `CubeListBuilder` 的 UV 布局一致。
const FACES: [&str; 6] = ["up", "down", "east", "north", "west", "south"];

/// 一个盒子六个面在贴图上的区域（像素），顺序与 `FACES` 一致。
fn box_regions(tex_off: [f32; 2], size: [f32; 3]) -> [[f32; 4]; 6] {
    let [dx, dy, dz] = size;
    let (u, v) = (tex_off[0], tex_off[1]);
    let (x1, x2, x3, x4) = (u + dz, u + dz + dx, u + dz + dx + dz, u + dz + dx + dz + dx);
    let y1 = v + dz;
    let y2 = v + dz + dy;
    // 首行两格并排，宽度都等于 dx：**第一格是底面、第二格才是顶面**。
    // 这一点在箱子贴图上验证得很清楚：箱盖的第一格是内腔深色（盖子底面），
    // 第二格是木纹（盖顶）；箱体反过来，第二格是深色（打开时看到的内腔）。
    // 写成 [顶][底] 会让箱子顶面取到内腔那块深色，画出来是一块黑顶。
    // 四个侧面在次行，宽度依次是 dz、dx、dz、dx；次行第二格曾误用
    // x3（u+dz+dx+dz），只有在 dx == dz 时才碰巧正确——箱子正好如此，床不是。
    [
        [x2, v, x2 + dx, v + dz],  // up
        [x1, v, x2, v + dz],       // down
        [u, y1, x1, y2],           // east（+x）
        [x1, y1, x2, y2],          // north（-z）
        [x2, y1, x3, y2],          // west（-x）
        [x3, y1, x4, y2],          // south（+z）
    ]
}

/// 用指定的六个区域造一个元素（区域按像素给出，输出仍是 0..16 空间）。
///
/// 床这类模型的每个面取自贴图上一块独立的区域，而不是盒子展开图，
/// 因此不能走 `box_element` 的自动推导。
fn painted_element(
    from: [f32; 3],
    to: [f32; 3],
    tex_px: [f32; 2],
    texture: &str,
    regions: [[f32; 4]; 6],
) -> ModelElementDef {
    let (sx, sy) = (16.0 / tex_px[0], 16.0 / tex_px[1]);
    let faces = FACES
        .iter()
        .zip(regions.iter())
        .map(|(name, r)| {
            (
                (*name).to_string(),
                ModelFace {
                    texture: texture.to_string(),
                    cullface: String::new(),
                    tintindex: -1,
                    rotation: 0,
                    uv: Some([r[0] * sx, r[1] * sy, r[2] * sx, r[3] * sy]),
                },
            )
        })
        .collect();
    ModelElementDef { from, to, faces, rotation: None }
}

/// 按原版规则把一个盒子展开成六个带 UV 的面。
///
/// `from`/`to` 是盒子的两个对角（1/16 格）；`tex_off` 是该盒子在贴图上的展开
/// 原点（贴图像素）；`tex_px` 是贴图的像素尺寸。布局与原版一致：上行是上、下
/// 两面，下行是四个侧面，顺序为 x-、z-、x+、z+。
///
/// 输出的 UV 落在**模型的 0..16 空间**——与方块库其余部分一致，该空间把整张贴图
/// 视作 16×16，因此按像素算出的区域必须换算：箱子贴图是 64×64，直接用像素值
/// 会取到贴图之外，渲染出来几乎全透明。
fn box_element(
    from: [f32; 3],
    to: [f32; 3],
    tex_off: [f32; 2],
    tex_px: [f32; 2],
    texture: &str,
) -> ModelElementDef {
    let size = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    painted_element(from, to, tex_px, texture, box_regions(tex_off, size))
}

/// 箱子的三种形态。
///
/// 大箱子由并排的两个箱子方块拼成，各自的方块状态里 `type` 分别是 `left`/`right`。
/// 原版把每一半建成 15 格宽：单箱两侧各留一格，大箱子的每一半则把靠拼缝的那一侧
/// 顶到方块边界，两半接起来正好 30 格宽。
#[derive(Clone, Copy)]
enum ChestHalf {
    Single,
    /// 左半：模型坐标系里拼缝在 **-x** 一侧（占 x 0..15），与原版左半层一致
    Left,
    /// 右半：模型坐标系里拼缝在 **+x** 一侧（占 x 1..16）
    Right,
}

impl ChestHalf {
    /// 方块状态里 `type` 的取值
    fn state_value(self) -> &'static str {
        match self {
            ChestHalf::Single => "single",
            ChestHalf::Left => "left",
            ChestHalf::Right => "right",
        }
    }
}

/// 半只箱子该用哪张贴图。
///
/// 原版四张半箱贴图各自把**拼缝那一面**留空——那一面被邻半挡住，不必画。
/// 实测（量每格的不透明像素数，箱盖与箱体两行都一样）：
///   `*_left`  空的是 **+x** 面，`*_right` 空的是 **-x** 面。
///
/// 而两半在**模型坐标系**里的拼缝是：左半占 x 0..15，拼缝在 **-x**；
/// 右半占 x 1..16，拼缝在 **+x**。两者合起来就意味着贴图与 `type` 是**交叉**
/// 对应的：左半要用 `_right`，右半要用 `_left`。
///
/// 用反了不会报错，只会把「朝外」的那一面画成空的：渲染出来像箱子缺了一面，
/// 两半也接不上。这一点用不透明像素数就能看出来——贴图配错时，某些朝向下
/// 箱子只有约 41% 的像素，配对了是 55%。
fn chest_texture(short: &str, half: ChestHalf) -> &'static str {
    match (short, half) {
        ("ender_chest", _) => "entity/chest/ender",
        ("trapped_chest", ChestHalf::Single) => "entity/chest/trapped",
        ("trapped_chest", ChestHalf::Left) => "entity/chest/trapped_right",
        ("trapped_chest", ChestHalf::Right) => "entity/chest/trapped_left",
        (_, ChestHalf::Single) => "entity/chest/normal",
        (_, ChestHalf::Left) => "entity/chest/normal_right",
        (_, ChestHalf::Right) => "entity/chest/normal_left",
    }
}

/// 单只箱子的几何：箱体 + 箱盖 + 锁扣，三个盒子共用一张贴图。
///
/// 箱子是否敞开记在方块实体数据里、不在方块状态中，因此一律按闭合状态绘制。
/// 贴图是原版的 64×64 展开图；大箱子的两半各有自己的贴图（`normal_left` /
/// `normal_right`），锁扣都贴在拼缝那一侧，合起来中间只有一个扣。
///
/// 坐标为**原版箱子模型的坐标系**：正面（锁扣一侧）在 +z。这一点与方块 JSON
/// 模型相反——那些模型的正面画在 north 面（见 `block/orientable_with_bottom.json`
/// 把 `front` 挂在 `north`），因此箱子的朝向旋转要比它们多转 180°，
/// 见 `variants_of` 里的说明。
fn chest_elements(texture: &str, half: ChestHalf) -> Vec<ModelElementDef> {
    let px = [64.0, 64.0];
    let (x0, x1, lock_x, lock_w) = match half {
        ChestHalf::Single => (1.0, 15.0, 7.0, 2.0),
        ChestHalf::Left => (0.0, 15.0, 0.0, 1.0),
        ChestHalf::Right => (1.0, 16.0, 15.0, 1.0),
    };
    vec![
        // 箱体：宽 14（单箱）或 15（半只），高 10，正面在 +z
        box_element([x0, 0.0, 1.0], [x1, 10.0, 15.0], [0.0, 19.0], px, texture),
        // 箱盖：贴在箱体上沿
        box_element([x0, 9.0, 1.0], [x1, 14.0, 15.0], [0.0, 0.0], px, texture),
        // 锁扣：与原版一致地嵌在正面那一层（z 14..15），而不是凸出到方块外
        box_element([lock_x, 7.0, 14.0], [lock_x + lock_w, 11.0, 15.0], [0.0, 0.0], px, texture),
    ]
}

/// 十六种染料色，与贴图文件名一致。
const DYE_COLORS: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray",
    "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black",
];

/// 床的几何：一块床垫加四条腿。
///
/// 原版的床模型是把每半张床建成一块 **16×16×6 的竖板**，渲染时再转平成床垫。
/// 转平后各面的对应关系是固定的：竖板的「-z 面」（画着枕头与木框，也就是床从
/// 上往下看的那一面）成为**顶面**，「+z 面」（木板纹）成为**底面**，「±y 面」
/// 成为床垫的两条长边。这里直接把转平后的结果写出来，免得依赖旋转的符号约定。
///
/// 贴图区域与原版一致：床头板展开在 (0,0)，床尾板展开在 (0,22)，床腿在 (50,6)。
fn bed_elements(texture: &str, head: bool) -> Vec<ModelElementDef> {
    let px = [64.0, 64.0];
    let v = if head { 0.0 } else { 22.0 };
    let r = box_regions([0.0, v], [16.0, 16.0, 6.0]);
    // 按「转平」后的面序重排：[顶, 底, x-, z-, x+, z+]
    let mattress = [
        r[3], // 顶面 <- 竖板的 -z 面（枕头那一面）
        r[5], // 底面 <- 竖板的 +z 面（木板）
        r[2], // x- <- 竖板的 -x 面
        r[1], // z- <- 竖板的 -y 面
        r[4], // x+ <- 竖板的 +x 面
        r[0], // z+ <- 竖板的 +y 面
    ];
    let mut out = vec![painted_element(
        [0.0, 3.0, 0.0],
        [16.0, 9.0, 16.0],
        px,
        texture,
        mattress,
    )];
    // 四条床腿：3×3×3，落在四角
    for (x, z) in [(0.0, 0.0), (13.0, 0.0), (0.0, 13.0), (13.0, 13.0)] {
        out.push(box_element(
            [x, 0.0, z],
            [x + 3.0, 3.0, z + 3.0],
            [50.0, 6.0],
            px,
            texture,
        ));
    }
    out
}

/// 给元素加上绕方块中心的 y 轴旋转。
///
/// 立式告示牌与地面头颅有 16 档朝向（每档 22.5°），而变体只支持 90 度倍数，
/// 因此这些角度只能写在元素自身上。
fn with_y_rotation(elements: &mut [ModelElementDef], degrees: f32) {
    for e in elements.iter_mut() {
        e.rotation = Some(crate::block::state::ElementRotation {
            origin: [8.0, 0.0, 8.0],
            axis: "y".into(),
            angle: degrees,
            rescale: false,
            rot_vec: None,
        });
    }
}

/// 潜影盒的几何：下半是箱体、上半是箱盖（箱盖建模高 12，下面 4 格藏在箱体里）。
///
/// 贴图是原版的盒子展开图：箱盖展开在 (0,0)（高 16+12=28），箱体展开在 (0,28)
/// （高 16+8=24），两段首尾相接，正好合上。
fn shulker_elements(texture: &str) -> Vec<ModelElementDef> {
    let px = [64.0, 64.0];
    vec![
        box_element([0.0, 0.0, 0.0], [16.0, 8.0, 16.0], [0.0, 28.0], px, texture),
        box_element([0.0, 4.0, 0.0], [16.0, 16.0, 16.0], [0.0, 0.0], px, texture),
    ]
}

/// 告示牌的几何：一块牌面加一根立柱（墙式牌没有立柱）。
///
/// 牌面宽 24、高 12、厚 2，比所在的方块还宽——原版就是这样，牌面横跨相邻方块。
/// 贴图 64×32：牌面展开在 (0,0)，立柱展开在 (0,14)。
fn sign_elements(texture: &str, wall: bool) -> Vec<ModelElementDef> {
    let px = [64.0, 32.0];
    if wall {
        // 墙式牌贴在方块的一侧，牌面下沿比立式牌低一些
        vec![box_element([-4.0, 2.0, 0.0], [20.0, 14.0, 2.0], [0.0, 0.0], px, texture)]
    } else {
        vec![
            box_element([-4.0, 6.0, 7.0], [20.0, 18.0, 9.0], [0.0, 0.0], px, texture),
            box_element([7.0, 0.0, 7.0], [9.0, 14.0, 9.0], [0.0, 14.0], px, texture),
        ]
    }
}

/// 头颅的几何：一个 8×8×8 的头方块。
///
/// 贴图 64×32（玩家与僵尸是 64×64）：头部展开在 (0,0)，帽子/外层在 (32,0)。
/// 外层不画：它要求与原版一致的半透明叠加，画不好反而更假。
fn skull_element(texture: &str, tex_px: [f32; 2], wall: bool) -> Vec<ModelElementDef> {
    let y = if wall { 4.0 } else { 0.0 };
    vec![box_element([4.0, y, 4.0], [12.0, y + 8.0, 12.0], [0.0, 0.0], tex_px, texture)]
}

/// 十六个地面朝向（每档 22.5°）对应的变体条件串。
fn floor_rotation_variants(
    elements: &[ModelElementDef],
) -> Vec<CodeVariant> {
    (0..16)
        .map(|n| {
            let mut els = elements.to_vec();
            with_y_rotation(&mut els, 22.5 * n as f32);
            CodeVariant {
                when: format!("rotation={n}"),
                rot_x_div90: 0,
                rot_y_div90: 0,
                elements: els,
            }
        })
        .collect()
}

/// 按方块名给出几何；不属于这里的方块返回 None。
///
/// 键是完整方块名。贴图写成 jar 内的具体路径，构建时会像普通贴图一样从
/// 客户端 jar 里读取，因此缺少该贴图的版本自然不会画出来，而不是画出错的图。
pub fn variants_of(block: &str) -> Option<Vec<CodeVariant>> {
    let short = block.strip_prefix("minecraft:").unwrap_or(block);

    if let Some(color) = short.strip_suffix("_bed") {
        if DYE_COLORS.contains(&color) {
            let texture = format!("entity/bed/{color}");
            // 朝向与床头/床尾原本都由游戏代码处理，这里按状态展开：
            // 4 个朝向 × 头/尾两半 = 8 条变体。
            let mut out = Vec::new();
            for head in [false, true] {
                let part = if head { "head" } else { "foot" };
                for (i, facing) in ["north", "east", "south", "west"].iter().enumerate() {
                    out.push(CodeVariant {
                        when: format!("facing={facing},part={part}"),
                        rot_x_div90: 0,
                        rot_y_div90: i as u32,
                        elements: bed_elements(&texture, head),
                    });
                }
            }
            return Some(out);
        }
    }

    // 潜影盒 17 种：未染色的方块名就叫 shulker_box，染色的形如 <颜色>_shulker_box
    let shulker_color = if short == "shulker_box" {
        Some("")
    } else {
        short.strip_suffix("_shulker_box")
    };
    if let Some(color) = shulker_color {
        // 潜影盒 17 种：16 种染料色，加上未染色的那种（方块名就是 shulker_box）。
        // 贴图文件名除未染色的一律带 shulker_ 前缀。
        let texture = if color.is_empty() {
            "entity/shulker/shulker".to_string()
        } else if DYE_COLORS.contains(&color) {
            format!("entity/shulker/shulker_{color}")
        } else {
            return None;
        };
        {
            let elements = shulker_elements(&texture);
            // 六个朝向：默认箱盖朝上，其余靠 x/y 轴旋转摆到对应方向。
            // 先绕 x 转（up→north 一档 90 度），再绕 y 转到四个侧面方向。
            let dirs: [(&str, u32, u32); 6] = [
                ("up", 0, 0),
                ("down", 2, 0),
                ("north", 1, 0),
                ("south", 3, 0),
                ("east", 1, 1),
                ("west", 1, 3),
            ];
            return Some(
                dirs.iter()
                    .map(|(facing, rx, ry)| CodeVariant {
                        when: format!("facing={facing}"),
                        rot_x_div90: *rx,
                        rot_y_div90: *ry,
                        elements: elements.clone(),
                    })
                    .collect(),
            );
        }
    }

    // 各种告示牌：立式 16 档朝向、墙式 4 个朝向
    if let Some(wood) = short.strip_suffix("_wall_sign") {
        let texture = format!("entity/signs/{wood}");
        return Some(
            ["north", "east", "south", "west"]
                .iter()
                .enumerate()
                .map(|(i, facing)| CodeVariant {
                    when: format!("facing={facing}"),
                    rot_x_div90: 0,
                    rot_y_div90: i as u32,
                    elements: sign_elements(&texture, true),
                })
                .collect(),
        );
    }
    if let Some(wood) = short.strip_suffix("_sign") {
        let texture = format!("entity/signs/{wood}");
        return Some(floor_rotation_variants(&sign_elements(&texture, false)));
    }

    // 头颅：方块名形如 <类型>_head（僵尸、玩家）/ <类型>_skull（骷髅、苦力怕）
    if let Some(kind) = short.strip_suffix("_head").or_else(|| short.strip_suffix("_skull")) {
        // 贴图与像素尺寸按类型区分：玩家的头贴图是 64×64，骷髅一类是 64×32，
        // 尺寸不同则同一个 8×8×8 头方块对应的展开区域也不同。
        let (texture, px) = match kind {
            "player" => ("entity/player/wide/steve", [64.0, 64.0]),
            "zombie" => ("entity/zombie/zombie", [64.0, 64.0]),
            "skeleton" => ("entity/skeleton/skeleton", [64.0, 32.0]),
            "wither_skeleton" => ("entity/skeleton/wither_skeleton", [64.0, 32.0]),
            "creeper" => ("entity/creeper/creeper", [64.0, 32.0]),
            // 龙首与猪灵头不是简单的立方体（有长吻与耳朵），不在此列
            _ => return None,
        };
        // 地面头颅 16 档旋转；墙上的另有 facing
        let mut out = floor_rotation_variants(&skull_element(texture, px, false));
        for (i, facing) in ["north", "east", "south", "west"].iter().enumerate() {
            out.push(CodeVariant {
                when: format!("facing={facing}"),
                rot_x_div90: 0,
                rot_y_div90: i as u32,
                elements: skull_element(texture, px, true),
            });
        }
        return Some(out);
    }

    if short == "chest" || short == "trapped_chest" || short == "ender_chest" {
        // 末影箱永远是单只；普通箱与陷阱箱都能两两拼成大箱子
        let halves: &[ChestHalf] = if short == "ender_chest" {
            &[ChestHalf::Single]
        } else {
            &[ChestHalf::Single, ChestHalf::Left, ChestHalf::Right]
        };
        // 朝向原先由游戏代码处理，这里展开成四条变体，靠 y 轴旋转区分。
        // 顺序：north、east、south、west 每档 90 度。普通箱与陷阱箱再按 `type`
        // 分成单箱与左右半，这样并排的两个箱子才接成一只大箱子；末影箱没有
        // `type` 属性（它永远单只），条件串里也就不能写它，否则匹配不上。
        //
        // **旋转数值要比方块 JSON 模型多 180 度**：JSON 模型把正面画在 north 面
        // （`block/orientable_with_bottom.json` 把 front 挂在 north），因此
        // facing=north 对应 `y=0`；而箱子模型是从原版渲染代码照搬的，正面在 +z，
        // 同一个 facing 要多转半圈。少这 180 度时锁扣会朝反方向，大箱子的拼缝
        // 也跟着错位——两半各贴各的外侧，中间裂开，看起来就是「没有合并」。
        let has_type = short != "ender_chest";
        let mut out = Vec::new();
        for (i, facing) in ["north", "east", "south", "west"].iter().enumerate() {
            for half in halves {
                out.push(CodeVariant {
                    when: if has_type {
                        format!("facing={facing},type={}", half.state_value())
                    } else {
                        format!("facing={facing}")
                    },
                    rot_x_div90: 0,
                    rot_y_div90: (i as u32 + 2) % 4,
                    elements: chest_elements(chest_texture(short, *half), *half),
                });
            }
        }
        return Some(out);
    }

    None
}

#[cfg(test)]
#[path = "../tests/pack/code_models.rs"]
mod tests;
