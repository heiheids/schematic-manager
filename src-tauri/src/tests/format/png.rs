use super::*;

/// 把扫描线打包成带滤波字节的形式
fn pack_rows(rows: &[Vec<u8>], filter: u8) -> Vec<u8> {
    let mut out = Vec::new();
    for r in rows {
        out.push(filter);
        out.extend_from_slice(r);
    }
    out
}

#[test]
fn filter_none_passes_through() {
    let rows = vec![vec![10u8, 20, 30], vec![40, 50, 60]];
    let filtered = pack_rows(&rows, 0);
    let got = unfilter(&filtered, 3, 2, 0, 8).unwrap();
    assert_eq!(got, vec![10, 20, 30, 40, 50, 60]);
}

#[test]
fn filter_sub_adds_left_neighbor() {
    // Sub：每个字节加上左侧同通道的还原值
    let rows = vec![vec![10u8, 20, 30]];
    let filtered = pack_rows(&rows, 1);
    let got = unfilter(&filtered, 3, 1, 0, 8).unwrap();
    assert_eq!(got[0], 10);
    assert_eq!(got[1], 30, "20 + 左侧 10");
    assert_eq!(got[2], 60, "30 + 左侧 30");
}

#[test]
fn filter_up_adds_previous_row() {
    let rows = vec![vec![10u8, 20], vec![5, 7]];
    let filtered = pack_rows(&rows, 2);
    let got = unfilter(&filtered, 2, 2, 0, 8).unwrap();
    assert_eq!(&got[0..2], &[10, 20]);
    assert_eq!(&got[2..4], &[15, 27], "第二行加上第一行");
}

#[test]
fn filter_average_matches_spec() {
    // Average：加 floor((左 + 上) / 2)。
    // 注意「左」指的是**已还原**的左侧像素，因此同一行内也会逐步传播，
    // 不能拿原始字节当期望值。
    let rows = vec![vec![10u8, 20], vec![4, 4]];
    let filtered = pack_rows(&rows, 3);
    let got = unfilter(&filtered, 2, 2, 0, 8).unwrap();

    // 第 0 行：x=0 左 0 上 0 → 10；x=1 左 10 上 0 → 20 + 5 = 25
    assert_eq!(got[0], 10);
    assert_eq!(got[1], 25, "20 + floor((10 + 0) / 2)");
    // 第 1 行：x=0 左 0 上 10 → 4 + 5 = 9；x=1 左 9 上 25 → 4 + 17 = 21
    assert_eq!(got[2], 9, "4 + floor((0 + 10) / 2)");
    assert_eq!(got[3], 21, "4 + floor((9 + 25) / 2)");
}

#[test]
fn filter_paeth_picks_nearest() {
    // Paeth：a=左 b=上 c=左上，取预测值最接近的邻居
    let rows = vec![vec![10u8], vec![20]];
    let filtered = pack_rows(&rows, 4);
    let got = unfilter(&filtered, 1, 2, 0, 8).unwrap();
    assert_eq!(got[0], 10);
    // x=0 时 a=0 b=10 c=0 → pa=|10-0|=10 pb=0 pc=10 → 取 b=10
    assert_eq!(got[1], 30, "20 + 10");
}

#[test]
fn unfilter_rejects_wrong_length() {
    // 少一行应报错，而不是给出错位像素
    let e = unfilter(&[0, 1, 2], 3, 2, 0, 8).unwrap_err().to_string();
    assert!(e.contains("不符"), "错误信息应说明长度不符：{e}");
}

#[test]
fn unfilter_rejects_unknown_filter_type() {
    let filtered = vec![9u8, 1, 2, 3];
    let e = unfilter(&filtered, 3, 1, 0, 8).unwrap_err().to_string();
    assert!(e.contains("滤波类型"), "应指出滤波类型无效：{e}");
}

#[test]
fn rgba8_is_taken_verbatim() {
    let raw = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
    let got = to_rgba(&raw, 2, 1, 6, 8, &Palette::default(), None).unwrap();
    assert_eq!(got, vec![1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn rgb8_gets_opaque_alpha() {
    let raw = vec![1u8, 2, 3, 4, 5, 6];
    let got = to_rgba(&raw, 2, 1, 2, 8, &Palette::default(), None).unwrap();
    assert_eq!(got, vec![1, 2, 3, 255, 4, 5, 6, 255]);
}

#[test]
fn gray4_is_scaled_to_full_range() {
    // 两个 4 位灰度样本打包在一个字节：0x0F = 灰 0 与灰 15
    let raw = vec![0x0Fu8];
    let got = to_rgba(&raw, 2, 1, 0, 4, &Palette::default(), None).unwrap();
    assert_eq!(got[0], 0);
    assert_eq!(got[3], 255);
    assert_eq!(got[4], 255, "4 位最大值应映射到 255");
}

#[test]
fn palette_uses_alpha_table() {
    let pal = Palette {
        rgb: vec![[10, 20, 30], [40, 50, 60]],
        alpha: vec![128, 255],
    };
    let raw = vec![1u8, 0];
    let got = to_rgba(&raw, 2, 1, 3, 8, &pal, None).unwrap();
    assert_eq!(&got[0..4], &[40, 50, 60, 255]);
    assert_eq!(&got[4..8], &[10, 20, 30, 128]);
}

#[test]
fn palette_index_out_of_range_is_an_error() {
    let pal = Palette { rgb: vec![[1, 2, 3]], alpha: Vec::new() };
    let raw = vec![5u8];
    let e = to_rgba(&raw, 1, 1, 3, 8, &pal, None).unwrap_err().to_string();
    assert!(e.contains("越界"), "应指出调色板越界：{e}");
}

#[test]
fn gray_alpha_splits_channels() {
    let raw = vec![77u8, 128];
    let got = to_rgba(&raw, 1, 1, 4, 8, &Palette::default(), None).unwrap();
    assert_eq!(got, vec![77, 77, 77, 128]);
}

#[test]
fn transparency_key_makes_matching_pixel_fully_transparent() {
    let raw = vec![9u8, 9, 9, 1, 2, 3];
    let key = [[9u8, 9, 9]];
    let got = to_rgba(&raw, 2, 1, 2, 8, &Palette::default(), Some(&key)).unwrap();
    assert_eq!(got[3], 0, "与透明键相符的像素 alpha 应为 0");
    assert_eq!(got[7], 255, "其余像素保持不透明");
}

/// 1 位灰度的透明键也要生效（按原始采样值比较，不与 0..255 的缩放值比）。
#[test]
fn gray1_transparency_key_applies() {
    // 4 个 1 位样本打包成一字节 0b1010_0000：样本依次为 1、0、1、0
    let raw = vec![0b1010_0000u8];
    // 透明键取采样值 1
    let key = [[1u8, 0, 0]];
    let got = to_rgba(&raw, 4, 1, 0, 1, &Palette::default(), Some(&key)).unwrap();

    // 采样值 1 -> 灰 255 且全透明；采样值 0 -> 灰 0 且不透明
    assert_eq!(&got[0..4], &[255, 255, 255, 0], "第 1 个样本为 1，应透明");
    assert_eq!(&got[4..8], &[0, 0, 0, 255], "第 2 个样本为 0，应不透明");
    assert_eq!(&got[8..12], &[255, 255, 255, 0], "第 3 个样本为 1，应透明");
    assert_eq!(&got[12..16], &[0, 0, 0, 255], "第 4 个样本为 0，应不透明");
}

/// 4 位灰度的透明键同样按原始采样值比较。
#[test]
fn gray4_transparency_key_applies() {
    // 两个 4 位样本打包：0x0F -> 样本 0 与 15
    let raw = vec![0x0Fu8];
    let key = [[15u8, 0, 0]];
    let got = to_rgba(&raw, 2, 1, 0, 4, &Palette::default(), Some(&key)).unwrap();
    assert_eq!(got[3], 255, "采样值 0 不匹配透明键，应不透明");
    assert_eq!(got[7], 0, "采样值 15 匹配透明键，应全透明");
}

/// 组装一张最小 PNG：IHDR + 可选 tRNS + IDAT + IEND。
///
/// 块尾的 CRC 填 0：本模块的解码只负责取出像素，不做 CRC 校验，
/// 因此测试不必真实计算校验和。
fn build_png(
    width: u32,
    height: u32,
    color_type: u8,
    bit_depth: u8,
    trns: Option<&[u8]>,
    filtered_rows: &[u8],
) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out.extend_from_slice(&[0, 0, 0, 0]); // CRC 占位
    }
    use std::io::Write;

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(bit_depth);
    ihdr.push(color_type);
    ihdr.extend_from_slice(&[0, 0, 0]); // 压缩方式/滤波方式/隔行
    chunk(&mut out, b"IHDR", &ihdr);
    if let Some(t) = trns {
        chunk(&mut out, b"tRNS", t);
    }
    let mut enc =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(filtered_rows).unwrap();
    let comp = enc.finish().unwrap();
    chunk(&mut out, b"IDAT", &comp);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// 走完整解码路径验证低位深透明键：tRNS 是 2 字节大端，
/// 采样值在高字节为 0 的低字节里。这条能发现解析时取错字节
/// （例如误取高字节 body[0]，那会得到恒定 0）。
#[test]
fn decode_applies_gray1_transparency_key_from_chunk() {
    // 1 位灰度、宽 4 高 1：滤波字节 0 + 数据字节 0b1010_0000
    let rows = vec![0u8, 0b1010_0000];
    // tRNS：大端 0x0001，即透明键为采样值 1
    let png = build_png(4, 1, 0, 1, Some(&[0x00, 0x01]), &rows);

    let img = decode(&png).expect("应能解码");
    assert_eq!((img.width, img.height), (4, 1));
    assert_eq!(img.rgba.len(), 16);
    assert_eq!(img.rgba[3], 0, "第 1 个样本为 1，应透明");
    assert_eq!(img.rgba[7], 255, "第 2 个样本为 0，应不透明");
    assert_eq!(img.rgba[11], 0, "第 3 个样本为 1，应透明");
    assert_eq!(img.rgba[15], 255, "第 4 个样本为 0，应不透明");
}

/// 对照：不写 tRNS 块时整张图都应不透明。
///
/// 与上一条成对，确认「透明」确实来自 tRNS，而不是位深处理本身有别的错。
#[test]
fn decode_without_trns_leaves_gray1_opaque() {
    let rows = vec![0u8, 0b1010_0000];
    let png = build_png(4, 1, 0, 1, None, &rows);
    let img = decode(&png).expect("应能解码");
    for i in 0..4 {
        assert_eq!(img.rgba[i * 4 + 3], 255, "第 {} 个像素应不透明", i + 1);
    }
}

#[test]
fn decode_rejects_non_png() {
    let e = decode(b"not a png at all").unwrap_err().to_string();
    assert!(e.contains("签名"), "应指出签名不符：{e}");
}

#[test]
fn stride_accounts_for_sub_byte_depths() {
    // 5 个 4 位像素 = 20 位 = 3 字节
    assert_eq!(stride_of(5, 0, 4).unwrap(), 3);
    // 8 个 1 位像素 = 1 字节
    assert_eq!(stride_of(8, 0, 1).unwrap(), 1);
    // 3 个 RGB 像素 = 9 字节
    assert_eq!(stride_of(3, 2, 8).unwrap(), 9);
}

#[test]
fn unsupported_color_type_is_rejected() {
    let e = channels(7).unwrap_err().to_string();
    assert!(e.contains("色型"), "应指出色型不支持：{e}");
}

/// 自己编出来的 PNG 必须能被自己的解码器原样解回来。
///
/// 断言逐字节相等而不是「解得出图」：CRC 写错、扫描线少一个滤波字节、
/// 通道顺序反了，这些缺陷都会让图还能解出来但像素不对。
#[test]
fn encode_round_trips_through_decode() {
    let rgba: Vec<u8> = (0..8u32 * 5)
        .flat_map(|i| {
            let i = i as u8;
            [i.wrapping_mul(3), i.wrapping_mul(5), i.wrapping_mul(7), i.wrapping_add(1)]
        })
        .collect();
    let png = encode(8, 5, &rgba).expect("编码失败");
    let img = decode(&png).expect("编出来的 PNG 应能解回");
    assert_eq!(img.width, 8);
    assert_eq!(img.height, 5);
    assert_eq!(img.rgba, rgba, "往返后的像素必须与原像素一致");
}

/// 编码结果里的每个块都要带正确的 CRC32。
///
/// 这台机器上的解码器不看 CRC，所以前面那条往返测试发现不了 CRC 写错；
/// 但浏览器（前端用 data URL 显示渲染图）校验 CRC，写错就直接不显示。
#[test]
fn every_chunk_carries_a_valid_crc() {
    let png = encode(3, 2, &[0u8; 24]).expect("编码失败");
    let mut pos = 8usize;
    let mut seen = 0;
    while pos + 8 <= png.len() {
        let len = be32(&png[pos..]) as usize;
        let kind = &png[pos + 4..pos + 8];
        let body_end = pos + 8 + len;
        let want = be32(&png[body_end..]);
        let got = crc32(&png[pos + 4..body_end]);
        assert_eq!(
            want,
            got,
            "块 {} 的 CRC32 不符",
            String::from_utf8_lossy(kind)
        );
        seen += 1;
        pos = body_end + 4;
    }
    assert_eq!(seen, 3, "应写出 IHDR / IDAT / IEND 三个块");
    assert_eq!(pos, png.len(), "块序列应恰好铺满整个文件");
}

/// CRC32 必须与协议规定的取值一致。
///
/// 上面那条用的是同一个 crc32 函数去核对同一个函数写出来的值，**自洽但不自证**：
/// 多项式写错、位序搞反，它照样通过，而浏览器会因为 CRC 不符直接不显示图。
/// 这里用 CRC-32/ISO-HDLC 的标准校验值（RFC 1952 里 zlib 用的那一种）定死。
#[test]
fn crc32_matches_the_standard_check_value() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0);
}

/// 像素少于声明的尺寸时必须报错，而不是拿越界的图去编码。
#[test]
fn encode_rejects_short_pixel_buffer() {
    let e = encode(4, 4, &[0u8; 32]).unwrap_err().to_string();
    // 断言里必须带上准确的数目：只断言「含 4」时，随便一条含 4 的信息
    // （比如尺寸 64）都会通过，等于没断言。
    assert!(e.contains("64"), "错误信息应说明需要 64 字节：{e}");
    assert!(e.contains("32"), "错误信息应说明实际只有 32 字节：{e}");
}

/// 尺寸为 0 时必须报错：0 会让后续所有按宽高算出的下标失去意义。
#[test]
fn encode_rejects_zero_size() {
    for (w, h) in [(0u32, 4u32), (4, 0), (0, 0)] {
        let e = encode(w, h, &[]).unwrap_err().to_string();
        assert!(e.contains("尺寸"), "{w}×{h} 应被拒绝：{e}");
    }
}
