use super::*;
use crate::block::db::{BlockDef, Model, ModelElement, ModelFace, TextureInfo, Variant, VersionDef};
use std::collections::HashMap;

/// 状态键的解析：无属性、有属性、以及各种残缺写法。
#[test]
fn state_key_is_parsed_into_name_and_props() {
    let s = parse_state_key("minecraft:stone").unwrap();
    assert_eq!(s.name, "minecraft:stone");
    assert!(s.props.is_empty());

    let s = parse_state_key("minecraft:oak_stairs[facing=east,half=bottom]").unwrap();
    assert_eq!(s.name, "minecraft:oak_stairs");
    assert_eq!(
        s.props,
        vec![
            ("facing".to_string(), "east".to_string()),
            ("half".to_string(), "bottom".to_string()),
        ]
    );

    // 空的方括号等价于没有属性
    let s = parse_state_key("minecraft:stone[]").unwrap();
    assert_eq!(s.name, "minecraft:stone");
    assert!(s.props.is_empty());
}

/// 残缺状态必须报错而不是悄悄忽略属性。
#[test]
fn malformed_state_key_is_rejected() {
    for bad in [
        "",
        "minecraft:stone[",
        "minecraft:stone[facing]",
        "minecraft:stone[=east]",
        "minecraft:stone[facing=]",
        "[facing=east]",
    ] {
        assert!(
            parse_state_key(bad).is_err(),
            "{bad:?} 应被判定为非法状态"
        );
    }
}

/// 造一个不透明立方体方块的方块库（贴图是 2×2 的纯色块）
fn test_db() -> BlockDb {
    let faces: Vec<ModelFace> = (0..6u8)
        .map(|i| ModelFace {
            texture: 0,
            uv: [0.0, 0.0, 16.0, 16.0],
            cullface: i,
            tintindex: -1,
            rotation: 0,
        })
        .collect();
    let model = Model {
        translucent: false,
        elements: vec![ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [16.0, 16.0, 16.0],
            has_rotation: false,
            rot_kind: 0,
            origin: [0.0; 3],
            axis: 0,
            angle: 0.0,
            rescale: false,
            rot_vec: [0.0; 3],
            face_mask: 0x3F,
            faces,
        }],
    };
    let version = VersionDef {
        name: "1.20.1".into(),
        data_version: 3465,
        lang_idx: 0,
        texture_by_path: HashMap::from([("block/test".to_string(), 0u32)]),
        blocks: vec![BlockDef {
            name: "minecraft:stone".into(),
            variants: vec![Variant {
                when: String::new(),
                model_idx: 0,
                rot_x_div90: 0,
                rot_y_div90: 0,
                uvlock: false,
            }],
            multipart: Vec::new(),
        }],
        langs: Vec::new(),
        is_overlay: false,
    };
    BlockDb {
        strings: vec!["block/test".to_string()],
        langs: Vec::new(),
        textures: vec![TextureInfo {
            w: 2,
            h: 2,
            offset: 0,
            len: 16,
            has_alpha: false,
        }],
        models: vec![model],
        versions: vec![version],
        // 2×2 的白色像素
        pixels: vec![255; 16],
    }
}

/// 渲染一个方块：图必须是正方形、有实心像素、且四角是透明的。
///
/// 四角透明这一条是有效断言：方块投影成一个六边形，矩形画布的四角
/// 一定在图形之外。若相机没转起来（仍正视前方）或缩放算错导致图形
/// 撑满画布，四角就会变成实心。
#[test]
fn icon_is_square_with_transparent_corners() {
    let db = test_db();
    let img = render_rgba(&db, 0, "minecraft:stone", 64).unwrap();
    assert_eq!(img.width, 64);
    assert_eq!(img.height, 64);
    assert_eq!(img.rgba.len(), 64 * 64 * 4);

    let opaque = img.rgba.chunks_exact(4).filter(|p| p[3] > 200).count();
    assert!(opaque > 400, "应画出成片的实心像素，实际 {opaque}");

    for (x, y) in [(0usize, 0usize), (63, 0), (0, 63), (63, 63)] {
        let a = img.rgba[(y * 64 + x) * 4 + 3];
        assert_eq!(a, 0, "画布四角 ({x},{y}) 应为透明，实际 alpha={a}");
    }
}

/// 顶面必须比侧面亮：明暗系数取自世界朝向，顶面 1.00、侧面 0.72~0.88。
///
/// 断言的是「最亮的行比中间行亮」这种位置关系，而不是写死某个灰度值——
/// 写死值只是在核对测试夹具里那张贴图的颜色。
#[test]
fn top_face_is_brighter_than_the_sides() {
    let db = test_db();
    let img = render_rgba(&db, 0, "minecraft:stone", 64).unwrap();
    let row_min = |y: usize| -> u8 {
        (0..64)
            .filter(|&x| img.rgba[(y * 64 + x) * 4 + 3] == 255)
            .map(|x| img.rgba[(y * 64 + x) * 4])
            .max()
            .unwrap_or(0)
    };
    // 图形居中：顶面落在上部，侧面在中下部
    let top = row_min(16);
    let side = row_min(46);
    assert!(
        top > side,
        "顶面({top})应比侧面({side})亮，否则明暗方向反了"
    );
}

/// 一张平放的 16×16 平面：图形必须居中、留白、且被压扁。
///
/// 这条钉的是取景数学。俯角 30° 会让水平面在屏幕上变扁，偏航 45° 又让它
/// 沿对角线张开：水平方向拿到对角线的全长 √2，垂直方向被压成
/// √2·sin30° = √2/2，因此宽高比恰是 1/sin30° = 2。
/// 各轴独立缩放会让这个比值变成 1，方块也就不是方块了。
#[test]
fn flat_plane_is_centred_uniformly_scaled() {
    let mut db = test_db();
    // 只留顶面，并把元素压成零厚度的平面
    db.models[0].elements[0].from = [0.0, 0.0, 0.0];
    db.models[0].elements[0].to = [16.0, 0.0, 16.0];
    db.models[0].elements[0].face_mask = 1 << 1;
    db.models[0].elements[0].faces[1].cullface = 0xFF;

    let img = render_rgba(&db, 0, "minecraft:stone", 64).unwrap();
    let solid: Vec<(usize, usize)> = (0..64)
        .flat_map(|y| (0..64).map(move |x| (x, y)))
        .filter(|&(x, y)| img.rgba[(y * 64 + x) * 4 + 3] > 200)
        .collect();
    assert!(!solid.is_empty(), "平面应当画出可见像素");

    let xs: Vec<usize> = solid.iter().map(|p| p.0).collect();
    let ys: Vec<usize> = solid.iter().map(|p| p.1).collect();
    let (min_x, max_x) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
    let (min_y, max_y) = (*ys.iter().min().unwrap(), *ys.iter().max().unwrap());
    let (w, h) = (max_x - min_x + 1, max_y - min_y + 1);

    // 居中的判定留 2 像素余量：图形边界是像素化的，四个角的极值不一定对称
    let cx = (min_x + max_x) as f32 / 2.0;
    let cy = (min_y + max_y) as f32 / 2.0;
    assert!((cx - 31.5).abs() <= 2.0, "水平方向应居中，实际中心 x={cx}");
    assert!((cy - 31.5).abs() <= 2.0, "垂直方向应居中，实际中心 y={cy}");

    // 取景留白 6%，因此图形不能顶到画布边缘；水平方向应基本撑满
    assert!(w <= 62, "宽度不应顶到画布边缘，实际 {w}");
    assert!(w >= 52, "平面在水平方向应撑满取景，实际 {w}");
    let ratio = w as f32 / h as f32;
    assert!(
        (ratio - 2.0).abs() < 0.2,
        "水平面的宽高比应为 1/sin30° = 2，实际 {ratio}"
    );
}

/// 半透明贴图渲染出的像素必须是**直通** alpha，不能是预乘。
///
/// 这条守的是 PNG 与浏览器合成语义：PNG 存的是未乘 alpha 的颜色，
/// 浏览器的 `<img>` 会把 RGB 再按 alpha 合成一次。光栅化阶段为了能对
/// 超采样取平均，必须按预乘累加，因此写出去之前必须还原——
/// 漏掉还原时，50% 的纯红在透明底上会存成 (128,0,0,128) 而不是
/// (255,0,0,128)，界面里染色玻璃整片发暗、抗锯齿边缘被背景吃掉。
///
/// 断言的判据是「存下来的 RGB 等于贴图原色」：这个关系与预乘与否直接冲突，
/// 写死某个灰度值做不到这一点。
#[test]
fn translucent_icon_stores_straight_alpha() {
    let mut db = test_db();
    // 2×2 的纯红半透明像素
    db.pixels = vec![255, 0, 0, 128].repeat(4);
    // 六个面全部保留，保证画面里有半透明像素
    for f in db.models[0].elements[0].faces.iter_mut() {
        f.cullface = 0xFF;
    }

    let img = render_rgba(&db, 0, "minecraft:stone", 64).unwrap();
    let mut partial = 0usize;
    let mut max_red = 0u8;
    for px in img.rgba.chunks_exact(4) {
        if px[3] == 0 || px[3] == 255 {
            continue;
        }
        partial += 1;
        max_red = max_red.max(px[0]);
    }
    assert!(partial > 0, "应画出半透明像素，实际一个都没有");
    // 顶面的明暗系数是 1.0，因此那里的半透明像素红通道应回到满值。
    // 存成预乘值时，红通道等于 255×alpha/255×明暗 ≈ 128，一眼可辨。
    assert!(
        max_red >= 250,
        "半透明像素存的是预乘值：最亮的红通道只有 {max_red}，\
         直通 alpha 下顶面（明暗 1.0）应接近 255"
    );
}

/// 输出边长超限必须报错，而不是申请一块巨型缓冲。
///
/// 缓冲是 `(size*4)² * 4` 字节：size=4096 就要 1 GiB，而 Rust 分配失败是
/// abort 进程、不是可捕获错误，界面会直接消失。
#[test]
fn icon_size_must_stay_within_bounds() {
    assert_eq!(check_icon_size(MIN_ICON_SIZE).unwrap(), MIN_ICON_SIZE);
    assert_eq!(check_icon_size(MAX_ICON_SIZE).unwrap(), MAX_ICON_SIZE);
    assert_eq!(check_icon_size(ICON_SIZE).unwrap(), ICON_SIZE);

    for bad in [0u32, 1, MIN_ICON_SIZE - 1, MAX_ICON_SIZE + 1, 4096, 65536] {
        let e = check_icon_size(bad).unwrap_err().to_string();
        assert!(
            e.contains(&bad.to_string()),
            "错误信息应指明收到的边长 {bad}：{e}"
        );
    }
    // 渲染入口也必须拦住，而不是只在命令层拦
    let db = test_db();
    assert!(render_rgba(&db, 0, "minecraft:stone", 4096).is_err());
}

/// 贴图全透明时应当报错，而不是给出一张看不见的图。
///
/// 返回全透明 PNG 时前端会渲染一个空白 `<img>`：既不出「无图」占位，
/// 也没有任何原因，比直接报错更难排查。
#[test]
fn fully_transparent_texture_is_an_error() {
    let mut db = test_db();
    db.pixels = vec![0u8; 16];
    let e = render_rgba(&db, 0, "minecraft:stone", 64)
        .unwrap_err()
        .to_string();
    assert!(e.contains("可见像素"), "错误信息应说明没有可见像素：{e}");
}

/// 没有该方块的模型时必须报错，而不是给出一张空图。
#[test]
fn unknown_block_is_an_error() {
    let db = test_db();
    let e = render_rgba(&db, 0, "minecraft:not_a_block", 64)
        .unwrap_err()
        .to_string();
    assert!(e.contains("not_a_block"), "错误信息应含方块名：{e}");
}

/// 版本下标越界必须报错。
#[test]
fn out_of_range_version_is_an_error() {
    let db = test_db();
    assert!(render_rgba(&db, 9, "minecraft:stone", 64).is_err());
}

/// data URL 必须是可用的 PNG：前缀正确，且 base64 解出来的字节
/// 能被自己的 PNG 解码器读出同样尺寸的图。
#[test]
fn data_url_contains_a_decodable_png() {
    let db = test_db();
    let url = render_data_url(&db, 0, "minecraft:stone", 32).unwrap();
    let b64 = url
        .strip_prefix("data:image/png;base64,")
        .expect("应是 PNG 的 data URL");
    let bytes = decode_base64(b64);
    let img = png::decode(&bytes).expect("data URL 里的应是可解码的 PNG");
    assert_eq!(img.width, 32);
    assert_eq!(img.height, 32);
    // 有损的调用会让图全透明
    assert!(img.rgba.chunks_exact(4).any(|p| p[3] > 0), "解出的图不应全透明");
}

/// base64 的填充与长度：3 的倍数不带 `=`，余 1 带两个、余 2 带一个。
#[test]
fn base64_pads_correctly() {
    assert_eq!(base64(b"Man"), "TWFu");
    assert_eq!(base64(b"Ma"), "TWE=");
    assert_eq!(base64(b"M"), "TQ==");
    assert_eq!(base64(b""), "");
}

/// 用真实投影里的真实方块状态渲染一批图，写成人眼可核对的 PNG。
///
/// 标 `#[ignore]`：它只在需要人工看图时跑。渲染好不好看没法用断言表达，
/// 但「画出来的东西不对」必须能被人看出来，否则模型、贴图映射或
/// 相机角度出了问题都只会在界面上一片空白里被忽略。
///
/// 状态取自投影调色板而不是手写列表：手写的状态串多半在该版本里
/// 根本不是合法变体，那样测到的是自己的错，而不是真实数据。
///
/// 方块库不再随包分发，用 SM_ICON_DB 指向一份（通常是「版本库」生成的
/// 单版本包），并与投影的 dataVersion 对应。
#[test]
#[ignore = "需设置 SM_ICON_SRC 指向真实投影、SM_ICON_OUT 指定输出目录、SM_ICON_DB 指定方块库"]
fn dump_real_block_icons_for_visual_check() {
    use crate::schematic::litematic::Schematic;

    let src = std::env::var("SM_ICON_SRC").expect("需设置 SM_ICON_SRC 指向真实投影文件");
    let out = std::env::var("SM_ICON_OUT").expect("需设置 SM_ICON_OUT 指向输出目录");
    let db_path = std::env::var("SM_ICON_DB").expect(
        "需设置 SM_ICON_DB 指向方块库文件（如 app 数据目录 base/blocks/ 下的单版本包）",
    );
    let db = crate::block::db::BlockDb::load(&db_path, false).expect("方块库载入失败");

    let sc = Schematic::load(&src).expect("真实投影载入失败");
    let summary = sc.summary();
    let vi = db.pick_version(summary.data_version).expect("应能选到对应版本");
    std::fs::create_dir_all(&out).expect("创建输出目录失败");

    let a = sc.analyze();
    let mut states: Vec<&String> = a.blocks_by_state.keys().collect();
    states.sort();
    let mut ok = 0usize;
    let mut failed: Vec<String> = Vec::new();
    // 同时写一份索引：光看文件名看不出原始状态键（文件名里的分隔符是替换过的），
    // 拿它去对照界面或做别的人工核对时都需要原始键与中英文名。
    let mut index = String::from("file\tstate\tzh\ten\n");
    for (i, state) in states.iter().enumerate() {
        let name = state.split('[').next().unwrap_or(state);
        let (zh, en) = crate::block::names::block_display_names(&db, Some(vi), name);
        let safe = state.replace([':', '[', ']', '=', ','], "_");
        index.push_str(&format!("{i:03}_{safe}.png\t{state}\t{zh}\t{en}\n"));
        match render_rgba(&db, vi, state, 64) {
            Ok(img) => {
                let png = png::encode(img.width, img.height, &img.rgba).expect("编码失败");
                let path = std::path::Path::new(&out).join(format!("{i:03}_{safe}.png"));
                std::fs::write(&path, &png).expect("写 PNG 失败");
                ok += 1;
            }
            Err(e) => failed.push(format!("{state}: {e}")),
        }
    }
    std::fs::write(std::path::Path::new(&out).join("index.tsv"), index)
        .expect("写索引失败");
    println!("渲染成功 {ok} / {} 个状态", states.len());
    for f in &failed {
        println!("失败：{f}");
    }
    assert!(ok > 0, "真实投影里应当至少有一个方块能画出来");
}

/// 测试用 base64 解码：只求正确，不求快。
fn decode_base64(s: &str) -> Vec<u8> {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let Some(idx) = ALPHABET.iter().position(|&a| a == c) else {
            panic!("非法的 base64 字符");
        };
        acc = (acc << 6) | idx as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}
