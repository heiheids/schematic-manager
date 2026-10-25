use super::*;

fn elem(has_rotation: bool, rot_kind: u8, axis: u8, angle: f32, origin: [f32; 3]) -> ModelElement {
    ModelElement {
        from: [0.0, 0.0, 0.0],
        to: [16.0, 16.0, 16.0],
        has_rotation,
        rot_kind,
        origin,
        axis,
        angle,
        rescale: false,
        rot_vec: [0.0; 3],
        face_mask: 0x3F,
        faces: Vec::new(),
    }
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn approx3(a: [f32; 3], b: [f32; 3]) -> bool {
    approx(a[0], b[0]) && approx(a[1], b[1]) && approx(a[2], b[2])
}

#[test]
fn no_element_rotation_returns_input() {
    let e = elem(false, 0, 1, 90.0, [8.0, 8.0, 8.0]);
    assert_eq!(rotate_element([1.0, 2.0, 3.0], &e), [1.0, 2.0, 3.0]);
}

#[test]
fn single_axis_rotation_is_around_origin() {
    // 绕 Y 轴 90 度，原点在中心：点 (12,8,8) -> (8,8,4)
    let e = elem(true, 0, 1, 90.0, [8.0, 8.0, 8.0]);
    assert!(approx3(rotate_element([12.0, 8.0, 8.0], &e), [8.0, 8.0, 4.0]));

    // 原点处的点不受影响
    assert!(approx3(rotate_element([8.0, 8.0, 8.0], &e), [8.0, 8.0, 8.0]));

    // 360 度应回到原位
    let e360 = elem(true, 0, 1, 360.0, [8.0, 8.0, 8.0]);
    assert!(approx3(rotate_element([12.0, 8.0, 8.0], &e360), [12.0, 8.0, 8.0]));
}

#[test]
fn three_axis_rotation_uses_rot_vec() {
    let mut e = elem(true, 1, 0, 0.0, [0.0, 0.0, 0.0]);
    e.rot_vec = [90.0, 0.0, 0.0];
    // 绕 X 90 度：y->z 方向，(0,1,0) -> (0,0,1)
    assert!(approx3(rotate_element([0.0, 1.0, 0.0], &e), [0.0, 0.0, 1.0]));
}

#[test]
fn variant_rotation_matches_model_spec() {
    // 绕 X 90 度：Minecraft 的 x 是「绕轴 -x·90 度」，因此 (x,y,z) -> (x, z, 16-y)
    assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 1, 0), [1.0, 3.0, 14.0]));
    // 绕 X 180 度（正负同值，与方向无关）
    assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 2, 0), [1.0, 14.0, 13.0]));
    // 绕 X 270 度
    assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 3, 0), [1.0, 13.0, 2.0]));
    // 绕 Y 90 度：(x, y, z) -> (16-z, y, x)
    assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 0, 1), [13.0, 2.0, 1.0]));
    // 绕 Y 180 度
    assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 0, 2), [15.0, 2.0, 13.0]));
    // 绕 Y 270 度
    assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 0, 3), [3.0, 2.0, 15.0]));
}

/// 贴在东西向墙面上的方块：底座朝西、本体朝东。
///
/// 这条钉的是真实的方块状态：投影里贴着西面墙、朝向为东的拉杆
/// （`face=wall,facing=east`，Minecraft 记为 x=90,y=90）。
/// 曾经 x 的两档写反，这类方块整个被镜像——底座被画到墙的另一侧、
/// 把手扎进墙里，看上去就是「没有依附在方块上的悬空拉杆」。
#[test]
fn wall_mounted_block_faces_away_from_its_wall() {
    // 竖直向上的杆（模型里 +Y 方向的一点），转到 facing=east 后应指向 +X
    let up = rotate_variant([8.0, 16.0, 8.0], 1, 1);
    assert!(
        approx3(up, [16.0, 8.0, 8.0]),
        "facing=east 的墙上方块应朝东，实际 {up:?}"
    );
    // 底座那一面（模型里朝下、贴在方块底面的那一面）必须落在西侧 x=0
    let base = rotate_variant([8.0, 0.0, 8.0], 1, 1);
    assert!(
        approx3(base, [0.0, 8.0, 8.0]),
        "底座应贴在西面墙上（x=0），实际 {base:?}"
    );
    // 天花板（x=180）与地面（x=0）方向不受影响，避免修一处坏一处
    assert!(approx3(rotate_variant([8.0, 0.0, 8.0], 2, 0), [8.0, 16.0, 8.0]));
    assert!(approx3(rotate_variant([8.0, 0.0, 8.0], 0, 0), [8.0, 0.0, 8.0]));
}

#[test]
fn variant_rotation_zero_is_identity() {
    assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 0, 0), [1.0, 2.0, 3.0]));
    assert!(approx3(rotate_variant([1.0, 2.0, 3.0], 4, 8), [1.0, 2.0, 3.0]));
    assert!(variant_rotation_is_identity(0, 0));
    assert!(variant_rotation_is_identity(4, 4));
    assert!(!variant_rotation_is_identity(1, 0));
    assert!(!variant_rotation_is_identity(0, 2));
}

#[test]
fn four_rotations_return_to_origin() {
    // 同一轴连续转四次应回到原位
    let mut p = [3.0, 5.0, 7.0];
    for _ in 0..4 {
        p = rotate_variant(p, 1, 0);
    }
    assert!(approx3(p, [3.0, 5.0, 7.0]));

    let mut q = [3.0, 5.0, 7.0];
    for _ in 0..4 {
        q = rotate_variant(q, 0, 3);
    }
    assert!(approx3(q, [3.0, 5.0, 7.0]));
}

#[test]
fn to_world_applies_order_and_translation() {
    let e = elem(false, 0, 0, 0.0, [0.0; 3]);
    // 无旋转时：除以 16 后加上方块坐标
    assert!(approx3(
        to_world([16.0, 0.0, 0.0], &e, 0, 0, [2, 3, 4]),
        [3.0, 3.0, 4.0]
    ));
    // 带变体旋转时，先旋转再平移（绕 X 90 度后 (1,2,3) -> (1,3,14)）
    let got = to_world([1.0, 2.0, 3.0], &e, 1, 0, [0, 0, 0]);
    let want = [1.0 / 16.0, 3.0 / 16.0, 14.0 / 16.0];
    assert!(approx3(got, want));
}

#[test]
fn cull_offset_rotates_with_variant() {
    // 无旋转时原样返回
    assert_eq!(rotate_cull_offset([1, 0, 0], 0, 0), [1, 0, 0]);
    // 绕 Y 90 度：+X 方向应变为 +Z 方向
    // （对照 C++ RotateBlock：Y90 为 (16-z, y, x)，
    //   方向向量按中心偏移换算后得 (0,0,1)）
    assert_eq!(rotate_cull_offset([1, 0, 0], 0, 1), [0, 0, 1]);
    // 绕 X 90 度：+Y 方向应变为 -Z 方向
    // （Minecraft 的 x 是绕轴 -x·90 度，与上面的 rotate_variant 同一约定）
    assert_eq!(rotate_cull_offset([0, 1, 0], 1, 0), [0, 0, -1]);
    // 绕 Y 180 度：+X 变为 -X
    assert_eq!(rotate_cull_offset([1, 0, 0], 0, 2), [-1, 0, 0]);
    // 绕 Y 90 度：+Z 方向应变为 -X 方向
    assert_eq!(rotate_cull_offset([0, 0, 1], 0, 1), [-1, 0, 0]);
    // 结果始终是单位偏移（不出现 0 以外的分量组合错误）
    for rx in 0u8..4 {
        for ry in 0u8..4 {
            for off in [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
                let r = rotate_cull_offset(off, rx, ry);
                let mag: i32 = r.iter().map(|v| v.abs()).sum();
                assert_eq!(mag, 1, "rx={rx} ry={ry} off={off:?} -> {r:?} 不是单位偏移");
            }
        }
    }
}

#[test]
fn element_rotation_precedes_variant_rotation() {
    // 两种变换不可交换，顺序必须为「先元素后变体」：
    // 元素绕 X 90 度把 (0,1,0) 移到 (0,0,1)，变体再绕 X 180 度得 (0,16,15)
    let mut e = elem(true, 1, 0, 0.0, [0.0; 3]);
    e.rot_vec = [90.0, 0.0, 0.0];
    let got = to_world([0.0, 1.0, 0.0], &e, 2, 0, [0, 0, 0]);
    assert!(
        approx3(got, [0.0, 16.0 / 16.0, 15.0 / 16.0]),
        "实际 {got:?}"
    );

    // 若顺序反了会得到不同结果，这里断言两者确实不同
    let wrong = rotate_element(rotate_variant([0.0, 1.0, 0.0], 2, 0), &e);
    assert!(!approx3(wrong, [0.0, 16.0, 15.0]), "顺序反了应得到不同结果");
}
