//! 单个方块的等轴测渲染图。
//!
//! 方块栏每行要显示一张方块小图。复用前端的 Three.js 场景走不通：
//! 那套东西一次只服务一个完整投影，为几百行各建一套几何与材质会把
//! 绘制调用和显存拉爆。这里在 Rust 侧把单个方块光栅化成 PNG，
//! 前端只负责显示与缓存。
//!
//! 面剔除、明暗与染色必须与三维预览完全一致，否则同一个方块在两处
//! 长得不一样。为此不另写一套取面逻辑，而是造一个只含该方块的
//! 1×1×1 投影交给 `visibility::collect_quads`——邻居恒在区域外，
//! 因此它不会剔掉任何面，走的是与预览同一条代码路径。

use crate::block::db::BlockDb;
use crate::error::{AppError, AppResult};
use crate::schematic::litematic::{BlockState, Region, Schematic};
use crate::format::png;
use crate::render::scene;
use crate::render::visibility::Quad;

/// 默认输出边长（像素）
pub const ICON_SIZE: u32 = 64;
/// 允许的输出边长区间。
///
/// 光栅化缓冲是 `(size*SUPERSAMPLE)² * 4` 字节，且每次请求可带最多 256 个方块，
/// 因此 size 必须有上界：size=4096 单张就要 1 GiB，而 Rust 分配失败是 abort
/// 进程、不是可捕获错误。下界保证图形还有可辨认的尺寸。
pub const MIN_ICON_SIZE: u32 = 16;
pub const MAX_ICON_SIZE: u32 = 256;

/// 校验请求的输出边长。
///
/// 单独成函数是为了能脱离 AppHandle 直接测（`render_block_icons` 本身
/// 需要 Tauri 运行时，单测拿不到）。
pub fn check_icon_size(size: u32) -> AppResult<u32> {
    if !(MIN_ICON_SIZE..=MAX_ICON_SIZE).contains(&size) {
        return Err(AppError::Parse(format!(
            "渲染图边长 {size} 超出允许范围 {MIN_ICON_SIZE}..={MAX_ICON_SIZE}"
        )));
    }
    Ok(size)
}

/// 超采样倍数。
///
/// 不做超采样时，一张 16×16 的贴图按最近邻投到 64 像素上，斜边会明显锯齿；
/// 先按 4 倍光栅化再取平均，边缘相当于 16 级灰度过渡，观感接近游戏内图标。
const SUPERSAMPLE: u32 = 4;
/// 图形四周留白占边长的比例
const MARGIN: f32 = 0.06;
/// 相机绕 Y 轴的偏航角（度）。45° 让世界 X/Z 两轴在屏幕上等长。
const YAW_DEG: f32 = 45.0;
/// 相机向下俯角（度）。30° 是方块图标惯用的俯视角，能同时看到顶面与两个侧面。
const PITCH_DEG: f32 = 30.0;

/// 视图基：都是单位向量。
///
/// `to_camera` 由原点指向相机，因此「面朝向相机」就是法线与它点积为正。
struct View {
    right: [f32; 3],
    up: [f32; 3],
    to_camera: [f32; 3],
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt();
    if len == 0.0 {
        return [0.0, 0.0, 0.0];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

/// 由偏航角与俯角构造视图基。
fn view_basis() -> View {
    let yaw = YAW_DEG.to_radians();
    let pitch = PITCH_DEG.to_radians();
    let to_camera = [
        pitch.cos() * yaw.cos(),
        pitch.sin(),
        pitch.cos() * yaw.sin(),
    ];
    // 屏幕右方向 = 相机方向 × 世界上方；屏幕上方 = 右 × 相机方向
    let right = normalize(cross(to_camera, [0.0, 1.0, 0.0]));
    let up = normalize(cross(right, to_camera));
    View { right, up, to_camera }
}

/// 把 `minecraft:oak_stairs[facing=east,half=bottom]` 拆成方块状态。
///
/// 不接受半截状态：`[` 与 `]` 必须成对，属性必须是 `键=值`。
/// 解析失败返回错误而不是退化成「忽略属性」——忽略属性会画出一个
/// 与列表里那一行不对应的方块，比画不出来更容易误导人。
pub fn parse_state_key(key: &str) -> AppResult<BlockState> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::Parse("方块状态为空".into()));
    }
    let Some(open) = key.find('[') else {
        return Ok(BlockState::new(key));
    };
    if !key.ends_with(']') {
        return Err(AppError::Parse(format!("方块状态缺少右括号：{key}")));
    }
    let name = &key[..open];
    if name.is_empty() {
        return Err(AppError::Parse(format!("方块状态缺少方块名：{key}")));
    }
    let inner = &key[open + 1..key.len() - 1];
    let mut props = Vec::new();
    if !inner.is_empty() {
        for part in inner.split(',') {
            let Some((k, v)) = part.split_once('=') else {
                return Err(AppError::Parse(format!("方块属性不是 键=值：{part}")));
            };
            if k.is_empty() || v.is_empty() {
                return Err(AppError::Parse(format!("方块属性有空的一项：{part}")));
            }
            props.push((k.to_string(), v.to_string()));
        }
    }
    Ok(BlockState { name: name.to_string(), props })
}

/// 取该方块在该版本下全部可见的面。
///
/// 走 `collect_quads` 而不是自己遍历模型：面剔除、变体旋转、贴图池映射、
/// 明暗与染色全都只该有一份实现。
fn collect_faces(db: &BlockDb, version_idx: usize, state: &BlockState) -> Vec<Quad> {
    let region = Region {
        name: "icon".into(),
        pos: [0, 0, 0],
        size: [1, 1, 1],
        volume: 1,
        bits: 1,
        palette: vec![state.clone()],
        indices: vec![0],
        ..Default::default()
    };
    let sc = Schematic {
        root: crate::format::nbt::Value::Compound(Vec::new()),
        regions: vec![region],
        path: String::new(),
        meta_modified: false,
        blocks_modified: false,
    };
    crate::render::visibility::collect_quads(&sc, db, version_idx)
}

/// 已投影到屏幕空间、等待光栅化的一个面
struct ScreenQuad {
    /// 四个角点的 `[屏幕 x, 屏幕 y, 到相机的深度]`
    pts: [[f32; 3]; 4],
    /// 四个角点的贴图坐标，单位是模型格（0..16）
    uv: [[f32; 2]; 4],
    texture: u32,
    /// 该面的颜色系数（明暗 × 染色），三通道 0..1
    color: [f32; 3],
    /// 平均深度，用于从远到近排序
    depth: f32,
}

/// 把世界坐标的点投影成 `[屏幕 x(向右), 屏幕 y(向上), 深度(越大越近)]`
fn project(p: [f32; 3], view: &View) -> [f32; 3] {
    [dot(p, view.right), dot(p, view.up), dot(p, view.to_camera)]
}

/// 渲染一个方块，返回边长 `size` 的 RGBA 图。
///
/// 分层目的是让测试能直接看像素：编码 PNG 只是最后一层包装。
pub fn render_rgba(
    db: &BlockDb,
    version_idx: usize,
    state_key: &str,
    size: u32,
) -> AppResult<png::Image> {
    check_icon_size(size)?;
    if version_idx >= db.versions.len() {
        return Err(AppError::Parse(format!("方块库中没有下标为 {version_idx} 的版本")));
    }
    let state = parse_state_key(state_key)?;
    let faces = collect_faces(db, version_idx, &state);
    if faces.is_empty() {
        return Err(AppError::Parse(format!(
            "方块 {state_key} 在 {} 里没有可用模型",
            db.versions[version_idx].name
        )));
    }

    let view = view_basis();
    let mut quads: Vec<ScreenQuad> = Vec::with_capacity(faces.len());
    for f in &faces {
        // 面法线按角点顺序 0-2-1 求出，与 visibility::shade_for 同一约定
        let n = normalize(cross(
            [
                f.corners[2][0] - f.corners[0][0],
                f.corners[2][1] - f.corners[0][1],
                f.corners[2][2] - f.corners[0][2],
            ],
            [
                f.corners[1][0] - f.corners[0][0],
                f.corners[1][1] - f.corners[0][1],
                f.corners[1][2] - f.corners[0][2],
            ],
        ));
        // 背面剔除：面积为 0 的退化面法线也是 0，一并丢掉
        if dot(n, view.to_camera) <= 0.0 {
            continue;
        }
        let pts = std::array::from_fn(|k| project(f.corners[k], &view));
        let depth = pts.iter().map(|p| p[2]).sum::<f32>() / 4.0;
        quads.push(ScreenQuad {
            pts,
            uv: f.uv,
            texture: f.texture,
            color: [
                f.shade * f.tint[0],
                f.shade * f.tint[1],
                f.shade * f.tint[2],
            ],
            depth,
        });
    }
    if quads.is_empty() {
        return Err(AppError::Parse(format!(
            "方块 {state_key} 的模型没有朝向相机的面"
        )));
    }

    // 先算投影后的包围盒，再把它等比缩放居中到画布上。
    // 等比而不是各轴独立缩放：非等比会把方块压扁，方块就不是方块了。
    let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
    let (mut min_y, mut max_y) = (f32::MAX, f32::MIN);
    for q in &quads {
        for p in &q.pts {
            min_x = min_x.min(p[0]);
            max_x = max_x.max(p[0]);
            min_y = min_y.min(p[1]);
            max_y = max_y.max(p[1]);
        }
    }
    // 某轴对称退化（例如平面正好与视线平行）时不能拿 0 当分母
    let ext_x = if max_x - min_x > 1e-6 { max_x - min_x } else { 1.0 };
    let ext_y = if max_y - min_y > 1e-6 { max_y - min_y } else { 1.0 };
    let ss = SUPERSAMPLE;
    let canvas = size as f32 * ss as f32;
    let usable = 1.0 - 2.0 * MARGIN;
    let scale = (usable * canvas / ext_x).min(usable * canvas / ext_y);
    let cx = (min_x + max_x) / 2.0;
    let cy = (min_y + max_y) / 2.0;
    let cx_px = canvas / 2.0;
    let to_px = |p: [f32; 3]| -> [f32; 3] {
        // 图像 y 向下增长，因此这里把屏幕上的「向上」翻过来
        [cx_px + (p[0] - cx) * scale, cx_px - (p[1] - cy) * scale, p[2]]
    };

    let mut buf = vec![0u8; (canvas as usize) * (canvas as usize) * 4];

    // 画家算法：远的先画。单个方块内各面互不相交，按平均深度排序足够；
    // 半透明的玻璃、水正是靠这个顺序才不会盖住自己后面的面。
    quads.sort_by(|a, b| a.depth.partial_cmp(&b.depth).unwrap_or(std::cmp::Ordering::Equal));

    for q in &quads {
        let p: [[f32; 3]; 4] = std::array::from_fn(|k| to_px(q.pts[k]));
        let Some(tex) = Texture::load(db, q.texture) else {
            // 贴图像素缺失时跳过这个面，而不是画成纯色占位：
            // 占位色会让用户以为该方块就长这样
            continue;
        };
        // 四边形拆成两个三角形，绕序与面法线一致（0-2-1 与 0-3-2）
        for tri in [[0usize, 2, 1], [0, 3, 2]] {
            raster_triangle(
                &mut buf,
                canvas as usize,
                [p[tri[0]], p[tri[1]], p[tri[2]]],
                [q.uv[tri[0]], q.uv[tri[1]], q.uv[tri[2]]],
                q.color,
                &tex,
            );
        }
    }

    // 一个不透明像素都没有时说明贴图全丢了，返回一张看不见的图会让界面
    // 显示一个空白 <img>——既不出「无图」也没有原因，比直接报错更难排查。
    if !buf.chunks_exact(4).any(|px| px[3] > 0) {
        return Err(AppError::Parse(format!(
            "方块 {state_key} 的贴图没有任何可见像素"
        )));
    }

    let mut out = downsample(&buf, canvas as usize, size as usize, ss as usize);
    unpremultiply(&mut out);
    Ok(png::Image { width: size, height: size, rgba: out })
}

/// 一张贴图的像素与尺寸。
///
/// 打包成一个类型而不是把像素、宽、高分开传：三者必须一起对应同一张贴图，
/// 分开传时形参列表里同时出现 usize 与 f32 两份尺寸，很容易传错位，
/// 而传错位的后果是取到别的像素而不是报错。
struct Texture {
    rgba: Vec<u8>,
    w: usize,
    h: usize,
}

impl Texture {
    /// 取某个贴图池下标对应的像素；缺数据或尺寸为 0 时返回 None。
    fn load(db: &BlockDb, index: u32) -> Option<Texture> {
        let tex = db.textures.get(index as usize)?;
        if tex.w == 0 || tex.h == 0 {
            return None;
        }
        Some(Texture {
            rgba: scene::texture_rgba(db, index)?,
            w: tex.w as usize,
            h: tex.h as usize,
        })
    }
}

/// 把预乘 alpha 的缓冲还原成 PNG 要求的直通 alpha。
///
/// 光栅化阶段按预乘累加（`src*alpha + dst*(1-alpha)`），这对超采样取平均是
/// 必须的：预乘值可以线性平均，直通值不行。但 PNG（以及浏览器合成）用的都是
/// 直通 alpha，即 RGB 是**未乘**alpha 的颜色。直接把预乘值当直通值写出去，
/// 等于让浏览器再乘一次 alpha：50% 的纯红会被画成 (191,127,127) 而不是
/// (255,127,127)，染色玻璃整片发暗，抗锯齿边缘也会被背景吃掉。
fn unpremultiply(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3];
        if a == 0 {
            // 全透明像素的颜色无意义，归零以免解码方看到脏数据
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        } else if a < 255 {
            let inv = 255.0 / a as f32;
            for c in &mut px[..3] {
                *c = ((*c as f32 * inv).round()).clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// 光栅化一个三角形：三角形内的像素按重心坐标插值贴图坐标，做最近邻采样。
fn raster_triangle(
    buf: &mut [u8],
    size: usize,
    p: [[f32; 3]; 3],
    uv: [[f32; 2]; 3],
    color: [f32; 3],
    tex: &Texture,
) {
    let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1])
        - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
    if area.abs() < 1e-9 {
        return;
    }
    let inv_area = 1.0 / area;

    let min_x = p.iter().map(|v| v[0]).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
    let max_x = (p.iter().map(|v| v[0]).fold(f32::MIN, f32::max).ceil())
        .min(size as f32 - 1.0)
        .max(0.0) as usize;
    let min_y = p.iter().map(|v| v[1]).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
    let max_y = (p.iter().map(|v| v[1]).fold(f32::MIN, f32::max).ceil())
        .min(size as f32 - 1.0)
        .max(0.0) as usize;
    if max_x < min_x || max_y < min_y {
        return;
    }

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            // 取像素中心，避免整幅图往左上偏半个像素
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = ((p[1][0] - px) * (p[2][1] - py) - (p[2][0] - px) * (p[1][1] - py)) * inv_area;
            let w1 = ((p[2][0] - px) * (p[0][1] - py) - (p[0][0] - px) * (p[2][1] - py)) * inv_area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }

            // 贴图坐标单位是模型格（0..16），归一化后按最近邻取像素。
            // 与三维预览保持一致：Three.js 侧同样用 uv/16 采样。
            let u = (w0 * uv[0][0] + w1 * uv[1][0] + w2 * uv[2][0]) / 16.0;
            let v = (w0 * uv[0][1] + w1 * uv[1][1] + w2 * uv[2][1]) / 16.0;
            let tx = ((u * tex.w as f32) as isize).clamp(0, tex.w as isize - 1) as usize;
            let ty = ((v * tex.h as f32) as isize).clamp(0, tex.h as isize - 1) as usize;
            let ti = (ty * tex.w + tx) * 4;
            let src = match tex.rgba.get(ti..ti + 4) {
                Some(s) => s,
                None => continue,
            };
            let sa = src[3] as f32 / 255.0;
            if sa <= 0.0 {
                continue;
            }

            let oi = (y * size + x) * 4;
            let dst = &mut buf[oi..oi + 4];
            for c in 0..3 {
                let v = (src[c] as f32 / 255.0) * color[c];
                let s = (v * 255.0).clamp(0.0, 255.0);
                // 按**预乘** alpha 累加：RGB 侧乘上自己的 alpha，alpha 侧正常合成。
                // 预乘是超采样取平均的前提（只有预乘值能线性平均），
                // 写 PNG 之前再由 unpremultiply 还原成直通 alpha。
                let d = dst[c] as f32 / 255.0;
                dst[c] = ((s / 255.0 * sa + d * (1.0 - sa)) * 255.0).round() as u8;
            }
            let da = dst[3] as f32 / 255.0;
            dst[3] = ((sa + da * (1.0 - sa)) * 255.0).round() as u8;
        }
    }
}

/// 把超采样缓冲按 `factor`×`factor` 取平均降到目标尺寸。
///
/// 取平均而不是抽样：抽样会让贴图上的细线（红石线、铁轨）时有时无，
/// 同一个方块每次刷新还可能长得不一样。
fn downsample(src: &[u8], src_size: usize, dst_size: usize, factor: usize) -> Vec<u8> {
    let mut out = vec![0u8; dst_size * dst_size * 4];
    let n = (factor * factor) as u32;
    for y in 0..dst_size {
        for x in 0..dst_size {
            let mut acc = [0u32; 4];
            for dy in 0..factor {
                for dx in 0..factor {
                    let si = ((y * factor + dy) * src_size + (x * factor + dx)) * 4;
                    for c in 0..4 {
                        acc[c] += src[si + c] as u32;
                    }
                }
            }
            let di = (y * dst_size + x) * 4;
            for c in 0..4 {
                out[di + c] = (acc[c] / n) as u8;
            }
        }
    }
    out
}

/// base64 编码（标准字母表，带 `=` 填充）。
///
/// 只为了把 PNG 塞进 data URL 给前端，因此不引第三方库、也不做 URL 安全变体。
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// 渲染一个方块并包成可直接塞进 `<img src>` 的 data URL。
pub fn render_data_url(
    db: &BlockDb,
    version_idx: usize,
    state_key: &str,
    size: u32,
) -> AppResult<String> {
    let img = render_rgba(db, version_idx, state_key, size)?;
    let png = png::encode(img.width, img.height, &img.rgba)?;
    Ok(format!("data:image/png;base64,{}", base64(&png)))
}

#[cfg(test)]
#[path = "../tests/render/icon.rs"]
mod tests;
