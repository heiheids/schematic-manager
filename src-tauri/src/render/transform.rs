//! 模型坐标变换：元素旋转、变体旋转与平移到方块世界坐标。
//!
//! 对照 legacy-cpp/render.cpp 的 RotateElement 与 RotateBlock。
//! 顺序固定为：元素自身旋转 -> 变体旋转 -> 按方块坐标平移（并除以 16）。
//! 顺序错了会让旋转后的模型错位。

use crate::block::db::ModelElement;

/// 绕某轴旋转一个点（角度为度，右手系）。
fn rotate_axis(p: [f32; 3], axis: u8, deg: f32) -> [f32; 3] {
    let a = deg.to_radians();
    let (s, c) = a.sin_cos();
    let [x, y, z] = p;
    match axis {
        0 => [x, y * c - z * s, y * s + z * c],
        1 => [x * c + z * s, y, -x * s + z * c],
        _ => [x * c - y * s, x * s + y * c, z],
    }
}

/// 应用元素自身的旋转（绕 origin）。
///
/// `rot_kind` 为 0 时绕单一轴旋转 `angle` 度；
/// 非 0 时依次绕 X、Y、Z 旋转 `rot_vec` 中的三个角度。
pub fn rotate_element(p: [f32; 3], e: &ModelElement) -> [f32; 3] {
    if !e.has_rotation {
        return p;
    }
    let o = e.origin;
    let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
    let r = if e.rot_kind == 0 {
        rotate_axis(d, e.axis, e.angle)
    } else {
        let a = rotate_axis(d, 0, e.rot_vec[0]);
        let b = rotate_axis(a, 1, e.rot_vec[1]);
        rotate_axis(b, 2, e.rot_vec[2])
    };
    [r[0] + o[0], r[1] + o[1], r[2] + o[2]]
}

/// 应用方块变体的旋转。
///
/// 参数为 90 度的倍数（0~3），先绕 X 再绕 Y。
/// 旋转以方块中心为参照，坐标范围为 0..16，因此用 16 取补。
///
/// `rot_x_div90` 的方向与 Minecraft 的 `x` 一致，而**不是**数学上的正向旋转：
/// Minecraft 的 blockstate 把 `x` 当作绕轴 **-x·90 度**（右手系）施加，
/// 因此 x=90 要把 +Y 转到 -Z（北）。曾经这里按正向旋转实现（把 90 度与 270 度
/// 写反了），后果是所有贴在墙上/天花板上的方块都被镜像：
/// 墙面拉杆的底座被画到墙的另一侧、把手扎进墙里，看上去就是「悬空的拉杆」。
/// `y` 与 Minecraft 一致（-y·90 度），因此不需要改动——两轴方向不一致纯属
/// 移植时的笔误，不是有意的约定。
pub fn rotate_variant(p: [f32; 3], rot_x_div90: u8, rot_y_div90: u8) -> [f32; 3] {
    let [mut x, mut y, mut z] = p;
    match rot_x_div90 % 4 {
        1 => {
            let (ny, nz) = (z, 16.0 - y);
            y = ny;
            z = nz;
        }
        2 => {
            y = 16.0 - y;
            z = 16.0 - z;
        }
        3 => {
            let (ny, nz) = (16.0 - z, y);
            y = ny;
            z = nz;
        }
        _ => {}
    }
    match rot_y_div90 % 4 {
        1 => {
            let (nx, nz) = (16.0 - z, x);
            x = nx;
            z = nz;
        }
        2 => {
            x = 16.0 - x;
            z = 16.0 - z;
        }
        3 => {
            let (nx, nz) = (z, 16.0 - x);
            x = nx;
            z = nz;
        }
        _ => {}
    }
    [x, y, z]
}

/// 把一个模型空间的角点变换到方块世界坐标。
///
/// 顺序：元素自身旋转 -> 变体旋转 -> 除以 16 并加上方块坐标。
pub fn to_world(
    corner: [f32; 3],
    elem: &ModelElement,
    rot_x_div90: u8,
    rot_y_div90: u8,
    block_pos: [i32; 3],
) -> [f32; 3] {
    let p = rotate_element(corner, elem);
    let p = rotate_variant(p, rot_x_div90, rot_y_div90);
    [
        block_pos[0] as f32 + p[0] / 16.0,
        block_pos[1] as f32 + p[1] / 16.0,
        block_pos[2] as f32 + p[2] / 16.0,
    ]
}

/// 把邻面剔除的方向按变体旋转一并旋转。
///
/// 方块被旋转后，它朝向某个邻居的「那一面」也跟着变了。
/// 若仍按未旋转的方向去找邻居，就会剔除错误的方块——
/// 表现为旋转过的楼梯、栅栏等周围出现空洞或缺面。
///
/// 做法与参考实现一致：把方向向量当作「自方块中心偏移的 16 分之一点」，
/// 旋转后再换算回方向，最后四舍五入回整数偏移。
pub fn rotate_cull_offset(off: [i32; 3], rot_x_div90: u8, rot_y_div90: u8) -> [i32; 3] {
    if variant_rotation_is_identity(rot_x_div90, rot_y_div90) {
        return off;
    }
    let p = [
        off[0] as f32 * 16.0 + 8.0,
        off[1] as f32 * 16.0 + 8.0,
        off[2] as f32 * 16.0 + 8.0,
    ];
    let r = rotate_variant(p, rot_x_div90, rot_y_div90);
    [
        ((r[0] - 8.0) / 16.0).round() as i32,
        ((r[1] - 8.0) / 16.0).round() as i32,
        ((r[2] - 8.0) / 16.0).round() as i32,
    ]
}

/// 变体的旋转是否为单位变换。用于跳过无谓计算。
pub fn variant_rotation_is_identity(rot_x_div90: u8, rot_y_div90: u8) -> bool {
    rot_x_div90 % 4 == 0 && rot_y_div90 % 4 == 0
}

#[cfg(test)]
#[path = "../tests/render/transform.rs"]
mod tests;
