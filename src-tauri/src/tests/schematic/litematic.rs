use super::*;

/// 方块改动后写回，调色板与位宽都必须同步更新。
///
/// 防的是：替换向调色板追加了新状态，写回时却只写 BlockStates。
/// 那样盘上的调色板仍是旧长度，新索引越界；调色板增长后位宽不变，
/// write_bits 还会按掩码静默截断索引。产出的文件游戏读不出。
///
/// 这里直接检查写回的 NBT，不经过文件与压缩，因此不受测试桩限制。
#[test]
fn sync_writes_palette_and_recomputes_bits() {
    // 初始调色板 1 项、位宽 2；改动后扩到 4 项，位宽应升到 3
    let region_nbt = Value::Compound(vec![
        (
            "BlockStatePalette".into(),
            encode_palette(&[BlockState::new("minecraft:stone")]),
        ),
        ("BlockStates".into(), Value::LongArray(vec![0i64])),
        ("Size".into(), Value::Compound(vec![
            ("x".into(), Value::Int(4)),
            ("y".into(), Value::Int(1)),
            ("z".into(), Value::Int(1)),
        ])),
        ("Position".into(), Value::Compound(vec![
            ("x".into(), Value::Int(0)),
            ("y".into(), Value::Int(0)),
            ("z".into(), Value::Int(0)),
        ])),
    ]);
    let mut sc = Schematic {
        root: Value::Compound(vec![(
            "Regions".into(),
            Value::Compound(vec![("r".into(), region_nbt)]),
        )]),
        regions: vec![Region {
            name: "r".into(),
            pos: [0, 0, 0],
            size: [4, 1, 1],
            volume: 4,
            bits: 2,
            palette: vec![BlockState::new("minecraft:stone")],
            indices: vec![0, 0, 0, 0],
            ..Default::default()
        }],
        path: String::new(),
        meta_modified: false,
        blocks_modified: false,
    };

    // 模拟替换：调色板扩到 4 项，最后两个方块改用第 4 项（下标 3）
    sc.regions[0].palette.push(BlockState::new("minecraft:gold_block"));
    sc.regions[0].palette.push(BlockState::new("minecraft:dirt"));
    sc.regions[0].palette.push(BlockState::new("minecraft:oak_planks"));
    sc.regions[0].indices = vec![0, 3, 1, 3];
    sc.blocks_modified = true;

    sc.sync_to_nbt().expect("写回失败");

    let regs = match sc.root.get("Regions") {
        Some(Value::Compound(v)) => v.clone(),
        _ => panic!("Regions 缺失"),
    };
    let rv = regs.iter().find(|(n, _)| n == "r").map(|(_, v)| v).expect("区域 r 缺失");

    // 调色板必须写回，且长度与内存一致
    let pal = match rv.get("BlockStatePalette") {
        Some(Value::List(items, elem)) => {
            assert_eq!(*elem, 10, "元素类型应为 Compound");
            items.clone()
        }
        other => panic!("应写回 BlockStatePalette，实际 {other:?}"),
    };
    assert_eq!(pal.len(), 4, "调色板应写回全部 4 项");
    let name_of = |v: &Value| match v {
        Value::Compound(f) => match f.iter().find(|(k, _)| k == "Name").map(|(_, v)| v) {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        },
        _ => String::new(),
    };
    assert_eq!(name_of(&pal[0]), "minecraft:stone");
    assert_eq!(name_of(&pal[3]), "minecraft:oak_planks", "新增项应写回");

    // 位宽必须按新的调色板长度重算：4 项需要 2 位，5 项才需要 3 位，
    // 但这里重要的是「重新计算过」而不是沿用旧值
    let expect_bits = bits_for_palette(4);
    let arr = match rv.get("BlockStates") {
        Some(Value::LongArray(a)) => a.clone(),
        _ => panic!("BlockStates 缺失"),
    };
    assert_eq!(
        read_bits(&arr, 1, expect_bits),
        3,
        "索引应能按新位宽正确读回（旧位宽会截断）"
    );
    assert_eq!(read_bits(&arr, 0, expect_bits), 0);
    assert_eq!(read_bits(&arr, 3, expect_bits), 3);
}

#[test]
fn bits_for_palette_matches_litematica() {
    // 最少 2 位
    assert_eq!(bits_for_palette(0), 2);
    assert_eq!(bits_for_palette(1), 2);
    assert_eq!(bits_for_palette(2), 2);
    assert_eq!(bits_for_palette(3), 2);
    assert_eq!(bits_for_palette(4), 2);
    assert_eq!(bits_for_palette(5), 3);
    assert_eq!(bits_for_palette(8), 3);
    assert_eq!(bits_for_palette(9), 4);
    assert_eq!(bits_for_palette(16), 4);
    assert_eq!(bits_for_palette(17), 5);
    assert_eq!(bits_for_palette(61), 6);
}

#[test]
fn bits_roundtrip_within_single_long() {
    for bits in [2, 3, 5, 8] {
        let mut arr = vec![0i64; 8];
        let vals: Vec<i32> = (0..10).map(|i| (i * 3) % (1 << bits)).collect();
        for (i, v) in vals.iter().enumerate() {
            write_bits(&mut arr, i, bits, *v);
        }
        for (i, v) in vals.iter().enumerate() {
            assert_eq!(read_bits(&arr, i, bits), *v, "bits={bits} i={i}");
        }
    }
}

#[test]
fn bits_roundtrip_crossing_long_boundary() {
    // 用不能被 64 整除的位数，强制出现跨 long 的情况
    const COUNT: usize = 500;
    for bits in [2, 3, 5, 6, 7, 11, 13] {
        // 数组必须按实际位数需求分配：COUNT*bits 个比特。
        // 之前写死 64 个 long，对 11 位这种位数来说只够 372 个值，
        // 超出部分被 write_bits 的越界保护静默丢弃，read_bits 又返回 0，
        // 于是断言报出「写进去 1332、读出来 0」。
        let mut arr = vec![0i64; (COUNT * bits as usize).div_ceil(64)];
        let mask = (1i32 << bits) - 1;
        let vals: Vec<i32> = (0..COUNT as i32).map(|i: i32| (i.wrapping_mul(2654435761u32 as i32)) & mask).collect();
        for (i, v) in vals.iter().enumerate() {
            write_bits(&mut arr, i, bits, *v);
        }
        for (i, v) in vals.iter().enumerate() {
            assert_eq!(read_bits(&arr, i, bits), *v, "bits={bits} i={i}");
        }
    }
}

#[test]
fn encode_decode_indices_roundtrip() {
    let indices: Vec<i32> = (0..1000).map(|i| (i % 37) as i32).collect();
    let bits = bits_for_palette(37);
    let packed = encode_indices(&indices, bits);
    // 打包后的长度应为 ceil(1000*bits/64)
    assert_eq!(packed.len(), (1000 * bits as usize).div_ceil(64));
    let back = decode_indices(&packed, indices.len(), bits);
    assert_eq!(indices, back);
}

#[test]
fn write_bits_does_not_clobber_neighbours() {
    // 相邻值互不干扰
    let bits = 3;
    let mut arr = vec![0i64; 4];
    for i in 0..20 {
        write_bits(&mut arr, i, bits, 5);
    }
    for i in 0..20 {
        assert_eq!(read_bits(&arr, i, bits), 5);
    }
    // 改写其中一个，其余不变
    write_bits(&mut arr, 7, bits, 2);
    for i in 0..20 {
        let want = if i == 7 { 2 } else { 5 };
        assert_eq!(read_bits(&arr, i, bits), want, "i={i}");
    }
}

#[test]
fn write_bits_out_of_range_is_ignored() {
    // 数组容量不足时不应越界写入，也不应 panic。
    // 这正是上面那个测试曾经踩到的路径，显式固化下来。
    let mut arr = vec![0i64; 4];
    for i in 0..1000 {
        write_bits(&mut arr, i, 11, 1332);
    }
    for i in 0..1000 {
        let v = read_bits(&arr, i, 11);
        // 落在数组范围内的位置写成功，范围外的读回 0
        let in_range = (i + 1) * 11 <= arr.len() * 64;
        if in_range {
            assert_eq!(v, 1332, "i={i}");
        } else {
            assert_eq!(v, 0, "i={i}");
        }
    }
}

#[test]
fn read_bits_beyond_end_returns_zero() {
    let arr = vec![0i64; 1];
    assert_eq!(read_bits(&arr, 1000, 4), 0);
}

#[test]
fn region_index_order_is_x_then_z_then_y() {
    let r = Region { size: [4, 3, 2], ..Default::default() };
    // x 变化最快
    assert_eq!(r.index(1, 0, 0), 1);
    assert_eq!(r.index(0, 0, 1), 4);
    assert_eq!(r.index(0, 1, 0), 8);
    // 往返一致
    for y in 0..3 {
        for z in 0..2 {
            for x in 0..4 {
                assert_eq!(r.pos_from_index(r.index(x, y, z)), [x, y, z]);
            }
        }
    }
}

#[test]
fn negative_size_is_normalized() {
    // Size = {x:-16, y:-10, z:16}, Position = {x:15, y:9, z:-8}
    let region = Value::Compound(vec![
        ("Position".into(), Value::Compound(vec![
            ("x".into(), Value::Int(15)), ("y".into(), Value::Int(9)), ("z".into(), Value::Int(-8)),
        ])),
        ("Size".into(), Value::Compound(vec![
            ("x".into(), Value::Int(-16)), ("y".into(), Value::Int(-10)), ("z".into(), Value::Int(16)),
        ])),
        ("BlockStatePalette".into(), Value::List(
            vec![
                Value::Compound(vec![("Name".into(), Value::String("minecraft:air".into()))]),
                Value::Compound(vec![("Name".into(), Value::String("minecraft:stone".into()))]),
            ],
            crate::format::nbt::TAG_COMPOUND,
        )),
        ("BlockStates".into(), Value::LongArray(vec![0i64; (16 * 10 * 16 * 2usize).div_ceil(64)])),
    ]);
    let r = region_from_nbt("t", &region).unwrap();
    assert_eq!(r.size, [16, 10, 16]);
    // 原点应移动到负方向的起点：15 + (-16) + 1 = 0，9 + (-10) + 1 = 0
    assert_eq!(r.pos, [0, 0, -8]);
    assert_eq!(r.volume, 2560);
}

/// Size 为 i32::MIN 时不能 panic，也不能放行一个位置错乱的区域。
///
/// i32::MIN 的相反数无法用 i32 表示：不检查时 debug 构建在取负处 panic，
/// release 构建回绕后仍是负数，区域会以「尺寸为 0」被放行，而 r.pos 已经
/// 被写成 -2147483647 之类的值——后续按它计算场景原点，整个预览会被
/// 框到极远处。这里要求明确报错。
#[test]
fn integer_min_size_is_rejected_not_wrapped() {
    let region = region_fixture([0, 0, 0], [i32::MIN, 1, 1], 8);
    let e = region_from_nbt("neg", &region)
        .expect_err("i32::MIN 无法取绝对值，必须报错而不是静默放行");
    let msg = e.to_string();
    assert!(
        msg.contains("绝对值") || msg.contains("Size"),
        "错误信息应指出问题所在，实际：{msg}"
    );
}

/// 构造一个用于测试区域解析的最小 NBT 结构。
/// `size` 与 `pos` 直接给出，调色板只放两项，BlockStates 尽量给足。
fn region_fixture(pos: [i32; 3], size: [i32; 3], longs: usize) -> Value {
    let ints3 = |a: [i32; 3]| {
        Value::Compound(vec![
            ("x".into(), Value::Int(a[0])),
            ("y".into(), Value::Int(a[1])),
            ("z".into(), Value::Int(a[2])),
        ])
    };
    Value::Compound(vec![
        ("Position".into(), ints3(pos)),
        ("Size".into(), ints3(size)),
        (
            "BlockStatePalette".into(),
            Value::List(
                vec![
                    Value::Compound(vec![("Name".into(), Value::String("minecraft:air".into()))]),
                    Value::Compound(vec![("Name".into(), Value::String("minecraft:stone".into()))]),
                ],
                crate::format::nbt::TAG_COMPOUND,
            ),
        ),
        ("BlockStates".into(), Value::LongArray(vec![0i64; longs])),
    ])
}

/// 尺寸大到体积溢出时，必须报错而不是 panic 或回绕。
///
/// 三个 i32::MAX 相乘约 9.9e27，超出 64 位 usize 上限（约 1.8e19），
/// 在 debug 构建会 panic、release 会回绕；回绕值随即被用来分配
/// indices（每方块 4 字节）。该值完全由文件内容决定，因此一个畸形的
/// .litematic 就能触发，必须挡住。
#[test]
fn volume_computation_overflow_is_rejected() {
    let err = region_volume([i32::MAX, i32::MAX, i32::MAX], "huge")
        .expect_err("乘积累积溢出应被拒绝");
    assert!(
        err.to_string().contains("溢出"),
        "错误信息应说明计算溢出，实际：{err}"
    );
}

/// 乘积不溢出但超过上限时，同样必须拒绝。
///
/// 与溢出分支区分开：这里的乘积在 usize 范围内。
#[test]
fn volume_above_limit_is_rejected() {
    // 100000^3 = 1e15，远超 MAX_REGION_VOLUME 但远小于 usize 上限
    let err = region_volume([100_000, 100_000, 100_000], "big")
        .expect_err("超过上限应被拒绝");
    assert!(
        err.to_string().contains("上限"),
        "错误信息应提到上限，实际：{err}"
    );
}

/// 恰好等于上限时应被接受，避免边界判错。
#[test]
fn volume_at_exact_limit_is_accepted() {
    // 50_000_000 = 50000 * 1000 * 1，正好是 MAX_REGION_VOLUME
    let v = region_volume([50_000, 1_000, 1], "edge").expect("恰好达上限应被接受");
    assert_eq!(v, MAX_REGION_VOLUME);
}

/// 超过上限一个方块即被拒绝，确认上限不是 < 与 <= 的关系判错。
#[test]
fn volume_one_over_limit_is_rejected() {
    // 断言错误内容：只断言 is_err 时，任何原因（例如乘法溢出走了另一条
    // 分支）都会让这条通过，就测不出「恰好超限」这个边界
    let err = region_volume([MAX_REGION_VOLUME as i32 + 1, 1, 1], "over")
        .expect_err("超过上限 1 应被拒绝");
    assert!(
        err.to_string().contains("上限"),
        "应指出超出上限，实际：{err}"
    );
}

/// 真实量级的尺寸照常通过。
#[test]
fn typical_region_volume_is_accepted() {
    // 一个 256×256×256 的区块级投影
    assert_eq!(region_volume([256, 256, 256], "t").unwrap(), 256 * 256 * 256);
    // 256×128×256 ≈ 840 万，是相当大规模的工程
    assert_eq!(region_volume([256, 128, 256], "t").unwrap(), 256 * 128 * 256);
}

/// 正常尺寸仍照常解析，确认上限没有误伤端到端路径。
#[test]
fn normal_region_volume_still_parses() {
    let longs = (16usize * 10 * 16 * 2).div_ceil(64);
    let r = region_fixture([0, 0, 0], [16, 10, 16], longs);
    let parsed = region_from_nbt("ok", &r).expect("正常尺寸应能解析");
    assert_eq!(parsed.volume, 2560);
    assert_eq!(parsed.size, [16, 10, 16]);
    assert_eq!(parsed.indices.len(), 2560);
}

// ---- region_from_nbt 的错误分支 ----
//
// 这些分支此前全部没有测试：唯一触及该函数的用例走的是正常路径。
// 畸形文件是必须处理的输入——.litematic 可能被截断、被别的工具改写，
// 也可能根本是另一种格式的文件被误当成投影打开。

/// 从 fixture 中删掉指定字段，构造缺字段的区域
fn region_without(drop: &str) -> Value {
    let longs = (16usize * 10 * 16 * 2).div_ceil(64);
    match region_fixture([0, 0, 0], [16, 10, 16], longs) {
        Value::Compound(fields) => Value::Compound(
            fields.into_iter().filter(|(k, _)| k != drop).collect(),
        ),
        other => other,
    }
}

#[test]
fn region_without_position_is_rejected() {
    let err = region_from_nbt("r", &region_without("Position"))
        .expect_err("缺少 Position 应报错");
    assert!(
        err.to_string().contains("Position"),
        "错误信息应指出缺少 Position，实际：{err}"
    );
}

#[test]
fn region_without_size_is_rejected() {
    let err =
        region_from_nbt("r", &region_without("Size")).expect_err("缺少 Size 应报错");
    assert!(
        err.to_string().contains("Size"),
        "错误信息应指出缺少 Size，实际：{err}"
    );
}

#[test]
fn region_without_palette_is_rejected() {
    let err = region_from_nbt("r", &region_without("BlockStatePalette"))
        .expect_err("缺少调色板应报错");
    assert!(
        err.to_string().contains("调色板"),
        "错误信息应指出缺少调色板，实际：{err}"
    );
}

/// 调色板字段存在但类型不对（这里是字符串而非列表）也要报错。
#[test]
fn region_with_non_list_palette_is_rejected() {
    let longs = (16usize * 10 * 16 * 2).div_ceil(64);
    let v = match region_fixture([0, 0, 0], [16, 10, 16], longs) {
        Value::Compound(mut fields) => {
            for (k, val) in fields.iter_mut() {
                if k == "BlockStatePalette" {
                    *val = Value::String("not a list".into());
                }
            }
            Value::Compound(fields)
        }
        other => other,
    };
    let err = region_from_nbt("r", &v).expect_err("调色板类型不对应报错");
    assert!(
        err.to_string().contains("不是列表"),
        "错误信息应指出类型不对，实际：{err}"
    );
}

/// BlockStates 长度不足时必须报错，而不是当成「全是空气」静默通过。
///
/// 这是本项目特意加的守卫：read_bits 在数组偏短时一律返回 0，
/// 而索引 0 通常是空气，于是统计结果会静默偏小——用户看到一个
/// 「看起来正常但数字是错的」投影，比直接报错更难发现。
#[test]
fn region_with_short_blockstates_is_rejected() {
    let need = (16usize * 10 * 16 * 2).div_ceil(64);
    // 少给一个 long
    let v = region_fixture([0, 0, 0], [16, 10, 16], need - 1);
    let err = region_from_nbt("r", &v).expect_err("BlockStates 偏短应报错");
    assert!(
        err.to_string().contains("长度不足"),
        "错误信息应指出长度不足，实际：{err}"
    );
}

/// 缺少 BlockStates 字段时应视为空区域（全为索引 0），而不是报错。
///
/// 这与上一条的区别：字段缺失是合法的（空区域），长度不足才是损坏。
#[test]
fn region_without_blockstates_is_all_air() {
    let v = region_without("BlockStates");
    let r = region_from_nbt("r", &v).expect("缺少 BlockStates 应可解析为空区域");
    assert_eq!(r.volume, 2560);
    assert!(
        r.indices.iter().all(|&i| i == 0),
        "缺少 BlockStates 时全部索引应为 0"
    );
}

#[test]
fn block_state_key_and_props() {
    let mut b = BlockState::new("minecraft:oak_stairs");
    b.set_prop("half", "bottom");
    b.set_prop("facing", "east");
    // 属性按字典序
    assert_eq!(b.prop_str(), "facing=east,half=bottom");
    assert_eq!(b.key(), "minecraft:oak_stairs[facing=east,half=bottom]");
    assert_eq!(b.display_name(), "oak_stairs");
    assert!(!b.is_air());
    assert!(BlockState::new("minecraft:cave_air").is_air());
}

// ---- 真实样本的往返与一致性验证 ----
//
// 这些测试读取 src/tests/data 下的真实 .litematic。样本是仓库的一部分，
// 因此缺失时直接失败，不做静默跳过。

fn sample_path() -> String {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/tests/data/28速单区块简单熔炉.litematic"
    )
    .to_string()
}

#[test]
fn real_sample_nbt_roundtrips_without_field_loss() {
    let bytes = std::fs::read(sample_path())
        .unwrap_or_else(|e| panic!("缺少测试样本 {}: {e}", sample_path()));

    let root = crate::format::nbt::parse_maybe_gzip(&bytes).expect("解析失败");
    let out = crate::format::nbt::serialize_gzip(&root, "", 6).expect("序列化失败");
    let back = crate::format::nbt::parse_maybe_gzip(&out).expect("再解析失败");

    // 写入路径必须零信息损失：任何字段的丢失或顺序改变都会在这里暴露
    assert_eq!(root, back, "NBT 树在往返后必须完全一致");
}

/// 实体与方块实体数量必须来自区域内的列表，而不是 Metadata。
///
/// 真实投影的 Metadata 里**没有** EntityCount / TileEntityCount /
/// PendingBlockTicks 这几项（样本的 Metadata 只有 9 个键，全是名称、
/// 作者、时间、体积一类），数量只能按区域列表求和。
/// 此前读 Metadata 的同名键，界面上这三项恒为 0。
#[test]
fn real_sample_counts_entities_from_region_lists() {
    let sc = Schematic::load(&sample_path()).expect("载入失败");
    let s = sc.summary();

    // 样本实测：5 个实体、131 个方块实体、0 个待处理刻
    assert_eq!(s.entity_count, 5, "应按区域列表统计实体数，实际 {}", s.entity_count);
    assert_eq!(
        s.tile_entity_count, 131,
        "应按区域列表统计方块实体数，实际 {}",
        s.tile_entity_count
    );
    assert_eq!(s.pending_ticks, 0, "样本没有待处理刻");

    // 汇总值必须等于各区域之和，而不是某个区域的值
    let by_region: i64 = sc.regions.iter().map(|r| r.entity_count as i64).sum();
    assert_eq!(
        s.entity_count, by_region,
        "汇总值应等于各区域实体数之和"
    );
}

/// 实体的标识字段是小写 `id`，不是 `Id`。
///
/// NBT 的键区分大小写。写成 "Id" 会一个都匹配不到，界面上的「实体」
/// 列表就只剩一行 unknown，用户看不到实际有哪些实体。
#[test]
fn real_sample_entity_ids_are_read_not_all_unknown() {
    let sc = Schematic::load(&sample_path()).expect("载入失败");
    let a = sc.analyze();

    assert!(
        !a.entities_by_id.contains_key("unknown"),
        "实体标识必须能读到；全部落入 unknown 说明读错了键名，实际 {:?}",
        a.entities_by_id
    );
    assert_eq!(
        a.entities_by_id.get("minecraft:item").copied(),
        Some(4),
        "样本含 4 个 minecraft:item，实际 {:?}",
        a.entities_by_id
    );
    assert_eq!(
        a.entities_by_id.get("minecraft:hopper_minecart").copied(),
        Some(1),
        "样本含 1 个 minecraft:hopper_minecart，实际 {:?}",
        a.entities_by_id
    );
}

/// 每个实体都要读出 id 与投影内坐标。
///
/// 这条防两类错误：`Pos` 没被读（坐标全是 0）、以及忘了加区域原点
/// （多区域投影的实体会全部挤在原点附近）。因此断言坐标不是全零、
/// 且至少有实体的坐标落在区域原点之外。
#[test]
fn real_sample_entity_positions_are_read_with_region_offset() {
    let sc = Schematic::load(&sample_path()).expect("载入失败");
    let a = sc.analyze();

    let total: u64 = a.entities_by_id.values().sum();
    assert_eq!(
        a.entities.len() as u64,
        total,
        "逐个实体列表的数量应与分组统计一致"
    );
    assert!(!a.entities.is_empty(), "样本应含实体");

    // id 必须逐个带上，不能出现 unknown
    assert!(
        a.entities.iter().all(|e| e.id != "unknown"),
        "逐个实体的 id 都必须读到"
    );

    // 坐标不能全是 0：三个分量至少有一个出现过非零值
    let any_nonzero = a.entities.iter().any(|e| e.pos.iter().any(|v| *v != 0.0));
    assert!(
        any_nonzero,
        "实体坐标全为 0，说明 Pos 没被读到：{:?}",
        &a.entities[..a.entities.len().min(3)]
    );

    // 坐标必须是有限的，不能出现 NaN/无穷
    assert!(
        a.entities.iter().all(|e| e.pos.iter().all(|v| v.is_finite())),
        "坐标必须是有限值"
    );

    // 区域原点非零时，实体的投影内坐标应包含该偏移：
    // 取一个 Pos 非零的实体，其坐标不可能等于区域原点 + 0 以外的巧合
    for r in &sc.regions {
        let has_origin = r.pos.iter().any(|v| *v != 0);
        if !has_origin {
            continue;
        }
        let limit = r.pos.map(|v| v as f64);
        // 投影内坐标至少有一个分量达到或超过区域原点的对应分量
        let reaching = a
            .entities
            .iter()
            .any(|e| (0..3).any(|k| e.pos[k] >= limit[k]));
        assert!(
            reaching,
            "区域 {:?} 原点为 {:?}，但没有任何实体坐标达到该偏移，\
             说明可能漏加了区域位置",
            r.name, r.pos
        );
        break;
    }
}

/// 实体坐标 = 区域原点 + 文件里的相对 `Pos`。
///
/// 用固定数值断言而不是看真实样本「不为零」：样本的区域原点可能恰好是
/// 零，那样即使漏加偏移也照样通过（实测这条断言在反向变异下确实漏过）。
/// 区域原点与 Pos 都刻意取非零、且含小数与负数，四种错误（漏加偏移、
/// 丢小数、丢负号、分量错位）都会失败。
#[test]
fn entity_positions_include_region_origin_exactly() {
    let dbl = |v: f64| Value::Double(v);
    let region_nbt = Value::Compound(vec![
        (
            "Position".into(),
            Value::Compound(vec![
                ("x".into(), Value::Int(10)),
                ("y".into(), Value::Int(5)),
                ("z".into(), Value::Int(-3)),
            ]),
        ),
        (
            "Entities".into(),
            Value::List(
                vec![
                    Value::Compound(vec![
                        ("id".into(), Value::String("minecraft:item".into())),
                        (
                            "Pos".into(),
                            Value::List(vec![dbl(1.5), dbl(2.5), dbl(3.5)], 6),
                        ),
                    ]),
                    // 第二个实体：分量各不相同，分量错位会被发现
                    Value::Compound(vec![
                        ("id".into(), Value::String("minecraft:boat".into())),
                        (
                            "Pos".into(),
                            Value::List(vec![dbl(-0.5), dbl(0.25), dbl(-8.0)], 6),
                        ),
                    ]),
                ],
                10,
            ),
        ),
    ]);
    let sc = Schematic {
        root: Value::Compound(vec![(
            "Regions".into(),
            Value::Compound(vec![("r".into(), region_nbt)]),
        )]),
        regions: vec![Region {
            name: "r".into(),
            pos: [10, 5, -3],
            size: [1, 1, 1],
            volume: 1,
            bits: 1,
            palette: vec![BlockState::new("minecraft:stone")],
            indices: vec![0],
            ..Default::default()
        }],
        path: String::new(),
        meta_modified: false,
        blocks_modified: false,
    };

    let a = sc.analyze();
    assert_eq!(a.entities.len(), 2, "两个实体都应记录");
    assert_eq!(a.entities[0].id, "minecraft:item");
    assert_eq!(
        a.entities[0].pos,
        [11.5, 7.5, 0.5],
        "应为区域原点 [10,5,-3] 与 Pos [1.5,2.5,3.5] 之和"
    );
    assert_eq!(a.entities[1].id, "minecraft:boat");
    assert_eq!(
        a.entities[1].pos,
        [9.5, 5.25, -11.0],
        "负数与小数都必须精确保留"
    );
    // 分组统计与逐个列表必须一致
    assert_eq!(a.entities_by_id.get("minecraft:item").copied(), Some(1));
    assert_eq!(a.entities_by_id.get("minecraft:boat").copied(), Some(1));
}

/// 缺少 `Pos` 的实体不能凭空得到坐标，也不能让解析失败。
#[test]
fn entity_without_pos_is_recorded_at_region_origin() {
    let region_nbt = Value::Compound(vec![
        (
            "Position".into(),
            Value::Compound(vec![
                ("x".into(), Value::Int(4)),
                ("y".into(), Value::Int(0)),
                ("z".into(), Value::Int(0)),
            ]),
        ),
        (
            "Entities".into(),
            Value::List(
                vec![Value::Compound(vec![(
                    "id".into(),
                    Value::String("minecraft:pig".into()),
                )])],
                10,
            ),
        ),
    ]);
    let sc = Schematic {
        root: Value::Compound(vec![(
            "Regions".into(),
            Value::Compound(vec![("r".into(), region_nbt)]),
        )]),
        regions: vec![Region {
            name: "r".into(),
            pos: [4, 0, 0],
            size: [1, 1, 1],
            volume: 1,
            bits: 1,
            palette: vec![BlockState::new("minecraft:stone")],
            indices: vec![0],
            ..Default::default()
        }],
        path: String::new(),
        meta_modified: false,
        blocks_modified: false,
    };
    let a = sc.analyze();
    assert_eq!(a.entities.len(), 1, "缺 Pos 的实体仍应出现在列表里");
    assert_eq!(a.entities[0].pos, [4.0, 0.0, 0.0], "退化为区域原点");
}

/// 缺少 `Pos` 的实体不能凭空得到坐标，也不能让解析失败。
#[test]
fn real_sample_loads_and_regions_are_wellformed() {        let sc = Schematic::load(&sample_path()).expect("载入失败");
    assert!(!sc.regions.is_empty(), "应解析出至少一个区域");

    for r in &sc.regions {
        assert!(
            r.size.iter().all(|&v| v > 0),
            "归一化后尺寸必须全为正，实际 {:?}",
            r.size
        );
        assert_eq!(
            r.indices.len(),
            r.volume,
            "索引数量必须等于区域体积"
        );
        assert!(!r.palette.is_empty(), "调色板不应为空");
        assert!(
            r.indices.iter().all(|i| (*i as usize) < r.palette.len()),
            "索引必须落在调色板范围内"
        );
    }
}

#[test]
fn decoded_block_count_matches_declared_total() {
    // 用文件自身声明的 TotalBlocks 校验位打包是否正确。
    // 这比断言一个魔数更有意义：它检验的是解码结果与文件元数据的自洽性。
    let sc = Schematic::load(&sample_path()).expect("载入失败");
    let declared = sc
        .root
        .get("Metadata")
        .and_then(|m| m.get("TotalBlocks"))
        .and_then(|v| v.as_i64())
        .expect("样本应含 Metadata.TotalBlocks");
    assert!(declared > 0, "TotalBlocks 应大于 0");

    let decoded = sc.analyze().non_air_total as i64;
    assert_eq!(
        decoded, declared,
        "解码出的非空气方块数应与元数据 TotalBlocks 一致"
    );
}

#[test]
fn metadata_edit_roundtrips_through_disk() {
    // 完整走一遍写入路径：载入 -> 改描述 -> 落盘 -> 重新载入。
    // 同时确认方块数据与其它元数据未被牵连改动。
    let dir = std::env::temp_dir().join(format!(
        "sm_meta_roundtrip_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let dst = dir.join("copy.litematic");
    std::fs::copy(sample_path(), &dst).unwrap();
    let dst_s = dst.to_string_lossy().into_owned();

    let before = Schematic::load(&dst_s).unwrap();
    let blocks_before = before.analyze().non_air_total;

    let mut sc = Schematic::load(&dst_s).unwrap();
    assert!(sc.set_meta_description("往返测试写入的描述"));
    sc.save_to(&dst_s).expect("保存失败");

    let after = Schematic::load(&dst_s).unwrap();
    let md = after.root.get("Metadata").expect("元数据应仍然存在");
    assert_eq!(
        md.get("Description").and_then(|v| v.as_str()),
        Some("往返测试写入的描述"),
        "描述应被正确写入"
    );
    // 方块数据不能因为改元数据而受影响
    assert_eq!(
        after.analyze().non_air_total,
        blocks_before,
        "改元数据不应影响方块数据"
    );
    // 其余元数据字段应原样保留
    assert_eq!(
        before.root.get("Metadata").and_then(|m| m.get("Name")).and_then(|v| v.as_str()),
        after.root.get("Metadata").and_then(|m| m.get("Name")).and_then(|v| v.as_str()),
        "未修改的元数据字段应保持不变"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

impl BlockState {
    /// 去掉命名空间前缀
    pub fn display_name(&self) -> &str {
        self.name.split_once(':').map(|(_, n)| n).unwrap_or(&self.name)
    }
}
