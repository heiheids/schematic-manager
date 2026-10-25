use super::*;

const FROM: [f32; 3] = [1.0, 2.0, 3.0];
const TO: [f32; 3] = [5.0, 6.0, 7.0];
const UV: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

fn corners(f: Face) -> FaceCorners {
    build_face_corners(f, FROM, TO, UV)
}

#[test]
fn corners_lie_on_the_face_plane() {
    // 每个面的四个角点必须落在该面所在的那个平面上
    for (f, axis, want) in [
        (Face::Down, 1, FROM[1]),
        (Face::Up, 1, TO[1]),
        (Face::North, 2, FROM[2]),
        (Face::South, 2, TO[2]),
        (Face::West, 0, FROM[0]),
        (Face::East, 0, TO[0]),
    ] {
        for c in corners(f).pos {
            assert!(
                (c[axis] - want).abs() < 1e-5,
                "{f:?} 的角点 {c:?} 不在平面上（轴 {axis} 应为 {want}）"
            );
        }
    }
}

#[test]
fn corners_span_the_element_bounds() {
    // 角点应恰好覆盖元素范围。注意：面自身所在的那个轴是「平的」，
    // 四个角点取同一值（即该面），不会跨 FROM..TO；只有另外两个轴
    // 才跨越整个元素范围。
    for (f, plane_axis) in [
        (Face::Down, 1usize),
        (Face::Up, 1),
        (Face::North, 2),
        (Face::South, 2),
        (Face::West, 0),
        (Face::East, 0),
    ] {
        let c = corners(f);
        for axis in 0..3 {
            let vals: Vec<f32> = c.pos.iter().map(|p| p[axis]).collect();
            let lo = vals.iter().cloned().fold(f32::INFINITY, f32::min);
            let hi = vals.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            if axis == plane_axis {
                assert!((hi - lo).abs() < 1e-5, "{f:?} 轴 {axis} 应为平面（四角同值）");
            } else {
                assert!((lo - FROM[axis]).abs() < 1e-5, "{f:?} 轴 {axis} 下界");
                assert!((hi - TO[axis]).abs() < 1e-5, "{f:?} 轴 {axis} 上界");
            }
        }
    }
}

#[test]
fn uv_corners_are_assigned_in_order() {
    // 四个角点依次对应 (u1,v1) (u2,v1) (u2,v2) (u1,v2)
    let uv = [0.1f32, 0.2, 0.8, 0.9];
    for f in [Face::Down, Face::Up, Face::North, Face::South, Face::West, Face::East] {
        let c = build_face_corners(f, FROM, TO, uv);
        assert_eq!(c.uv[0], [0.1, 0.2], "{f:?}");
        assert_eq!(c.uv[1], [0.8, 0.2], "{f:?}");
        assert_eq!(c.uv[2], [0.8, 0.9], "{f:?}");
        assert_eq!(c.uv[3], [0.1, 0.9], "{f:?}");
    }
}

#[test]
fn uv_u_direction_matches_model_spec() {
    // 对每个面，取 uv[0] 与 uv[1] 对应的两个角点，
    // 它们的空间增量方向应符合该面的 u 增长方向
    let uv = [0.0f32, 0.0, 1.0, 1.0];
    for (f, axis, sign) in [
        (Face::Down, 0, 1.0),   // u 随 +x
        (Face::Up, 0, 1.0),     // u 随 +x
        (Face::North, 0, -1.0), // u 随 -x
        (Face::South, 0, 1.0),  // u 随 +x
        (Face::West, 2, 1.0),   // u 随 +z
        (Face::East, 2, -1.0),  // u 随 -z
    ] {
        let c = build_face_corners(f, FROM, TO, uv);
        let delta = c.pos[1][axis] - c.pos[0][axis];
        assert!(
            delta * sign > 0.0,
            "{f:?} 的 u 方向不符：轴 {axis} 增量 {delta}，期望符号 {sign}"
        );
    }
}

/// 贴图旋转只能重新分配四个角，**不能改变采样的那块区域**。
///
/// 这是活塞臂贴图错位的根因所在：原先的实现把 uv 坐标绕贴图中心
/// (0.5,0.5) 旋转，当 uv 只占贴图一角时（活塞杆是 [0,0,16,4]）会把采样
/// 区域挪到另一块地方，于是取到的是贴图上完全不相干的像素。
#[test]
fn uv_rotation_keeps_the_sampled_region() {
    // 一个不方正的 uv 矩形：只占左上角，宽 16 高 4
    let uv = [[0.0f32, 0.0], [1.0, 0.0], [1.0, 0.25], [0.0, 0.25]];
    for rot in [0u16, 90, 180, 270] {
        let got = rotate_uv_corners(Face::East, uv, rot);
        // 四个角必须与原来那四个角是同一个集合（顺次置换）
        let mut a: Vec<[f32; 2]> = uv.to_vec();
        let mut b: Vec<[f32; 2]> = got.to_vec();
        a.sort_by(|x, y| x.partial_cmp(y).unwrap());
        b.sort_by(|x, y| x.partial_cmp(y).unwrap());
        assert_eq!(a, b, "rotation={rot} 改变了采样区域：{got:?}");
        // down 面走另一套方向，采样区域同样不能变
        let got_down = rotate_uv_corners(Face::Down, uv, rot);
        let mut c: Vec<[f32; 2]> = got_down.to_vec();
        c.sort_by(|x, y| x.partial_cmp(y).unwrap());
        assert_eq!(a, c, "rotation={rot}（down 面）改变了采样区域：{got_down:?}");
    }
}

/// rotation=0 是恒等；四次 90° 回到原状。
#[test]
fn uv_rotation_zero_is_identity_and_four_steps_wrap() {
    let uv = [[0.0f32, 0.0], [1.0, 0.0], [1.0, 0.25], [0.0, 0.25]];
    assert_eq!(rotate_uv_corners(Face::East, uv, 0), uv);
    let mut cur = uv;
    for _ in 0..4 {
        cur = rotate_uv_corners(Face::East, cur, 90);
    }
    assert_eq!(cur, uv, "四次 90° 应回到原状");
    // 360 与 0 等价，超过 360 的取值也应先取模
    assert_eq!(rotate_uv_corners(Face::East, uv, 360), uv);
    assert_eq!(
        rotate_uv_corners(Face::East, uv, 450),
        rotate_uv_corners(Face::East, uv, 90)
    );
}

/// 活塞：rotation 必须让 east 与 west 两侧的 u/v 落在正确的几何轴上。
///
/// `piston_head` 的侧面用 uv=[0,0,16,4]：u 覆盖 16 个单位、v 覆盖 4 个单位。
/// 这两侧的几何都是「y 方向 16 单位、z 方向 4 单位」，因此旋转后必须是
/// u 沿 y 跑满 16、v 沿 z 跑满 4。写成坐标等式断言，方向搞反或采样区域
/// 被挪走都会失败。
#[test]
fn piston_side_faces_map_uv_to_correct_axes() {
    let uv = [0.0f32, 0.0, 16.0, 4.0];
    // 活塞头侧面元素：[0,0,0] → [16,16,4]
    let (from, to) = ([0.0f32, 0.0, 0.0], [16.0f32, 16.0, 4.0]);

    // East rot=90：顺时针 → u 应等于 16 - y，v 应等于 z
    let mut c = build_face_corners(Face::East, from, to, uv);
    c.uv = rotate_uv_corners(Face::East, c.uv, 90);
    for i in 0..4 {
        let (y, z) = (c.pos[i][1], c.pos[i][2]);
        assert!(
            (c.uv[i][0] - (16.0 - y)).abs() < 1e-4,
            "East rot=90 顶点 {i}：u={} 应为 16-y={}",
            c.uv[i][0],
            16.0 - y
        );
        assert!(
            (c.uv[i][1] - z).abs() < 1e-4,
            "East rot=90 顶点 {i}：v={} 应为 z={z}",
            c.uv[i][1]
        );
    }

    // West rot=270：u 应等于 y，v 应等于 z
    let mut c = build_face_corners(Face::West, from, to, uv);
    c.uv = rotate_uv_corners(Face::West, c.uv, 270);
    for i in 0..4 {
        let (y, z) = (c.pos[i][1], c.pos[i][2]);
        assert!(
            (c.uv[i][0] - y).abs() < 1e-4,
            "West rot=270 顶点 {i}：u={} 应为 y={y}",
            c.uv[i][0]
        );
        assert!(
            (c.uv[i][1] - z).abs() < 1e-4,
            "West rot=270 顶点 {i}：v={} 应为 z={z}",
            c.uv[i][1]
        );
    }
}

/// 旋转方向：除 down 面外为顺时针，down 面为逆时针。
///
/// 方向搞反时贴图会上下颠倒 180°，对活塞那条只有 16×4 的贴图条尤其明显。
/// 顺时针 90° 时贴图左上角的图案转到面的右上角，因此顶点 0 取原左下角。
#[test]
fn uv_rotation_direction_follows_spec() {
    let uv = [[0.0f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    // 顺时针：顶点 i 取原角 (i-1)
    let got = rotate_uv_corners(Face::East, uv, 90);
    assert_eq!(got[0], uv[3], "顺时针 90° 时顶点 0 应取原左下角");
    assert_eq!(got[1], uv[0], "顺时针 90° 时顶点 1 应取原左上角");
    assert_eq!(got[2], uv[1]);
    assert_eq!(got[3], uv[2]);
    // down 面按格式定义是逆时针：顶点 i 取原角 (i+1)
    let down = rotate_uv_corners(Face::Down, uv, 90);
    assert_eq!(down[0], uv[1], "down 面逆时针 90° 时顶点 0 应取原右上角");
    assert_eq!(down[3], uv[0]);
    // 其余面与 East 一致，不能把 down 的例外扩散出去
    for f in [Face::Up, Face::North, Face::South, Face::West] {
        assert_eq!(rotate_uv_corners(f, uv, 90), got, "{f:?} 应与 East 同向");
    }
}

#[test]
fn degenerate_element_does_not_panic() {        // 零体积元素（from == to）在真实模型中存在，不应 panic 或产生 NaN
    for f in [Face::Down, Face::Up, Face::North, Face::South, Face::West, Face::East] {
        let c = build_face_corners(f, [2.0; 3], [2.0; 3], UV);
        for p in c.pos {
            assert!(p.iter().all(|v| v.is_finite()));
        }
    }
}

#[test]
fn cull_offsets_point_outward() {
    // 剔除偏移必须指向该面朝外的邻居方块；写死期望值而不是拿另一份
    // 实现对照——否则两份实现同时写错时会互相印证成「通过」。
    // 坐标约定：X 向东为正，Y 向上为正，Z 向南为正（与 Minecraft 一致）。
    let cases = [
        (Face::Down, [0, -1, 0]),
        (Face::Up, [0, 1, 0]),
        (Face::North, [0, 0, -1]),
        (Face::South, [0, 0, 1]),
        (Face::West, [-1, 0, 0]),
        (Face::East, [1, 0, 0]),
    ];
    for (f, want) in cases {
        assert_eq!(cull_neighbor_offset(f), want, "{f:?} 的剔除偏移不正确");
    }
}

#[test]
fn shade_is_within_unit_range_per_axis() {
    // 明暗系数只需落在 (0,1] 且顶面比底面亮——不锁定具体数值，
    // 这些系数会随观感调整
    for f in [Face::Down, Face::Up, Face::North, Face::South, Face::West, Face::East] {
        let s = face_shade(f);
        assert!(s > 0.0 && s <= 1.0, "{f:?} 明暗系数越界：{s}");
    }
    assert!(
        face_shade(Face::Up) > face_shade(Face::Down),
        "顶面应比底面亮，否则立体感方向相反"
    );
}
