//! 面的几何：角点生成。
//!
//! 对照 legacy-cpp/render.cpp 的 BuildFaceCorners。元素坐标为模型空间 0..16，
//! 四个角点的顺序与 UV 的对应关系必须与 Minecraft 的模型规范一致，
//! 否则贴图会旋转或镜像。
//!
//! 法线计算不在这里：相机与光照由前端的 three.js 负责，
//! 后端只给出明暗系数（FACE_SHADE）。

use crate::block::db::Face;

/// 一个面的四个角点及其 UV
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceCorners {
    pub pos: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
}

/// 按面的 rotation 字段旋转**四个角点各自取到的贴图坐标**。
///
/// 输入是 `build_face_corners` 给出的四个 uv（顺序为 v0..v3），
/// 输出是旋转后每个顶点应取的 uv。
///
/// 依据 Minecraft 模型格式的定义（Minecraft Wiki, Model）：
///   - 「rotation 把贴图顺时针旋转给定角度（**down 面为逆时针**）」；
///   - 「旋转**不改变取贴图的那块区域**，只是把所选到的贴图顶点做一次置换」。
/// 所以这里只对四个角做循环置换，采样区域原样保留。
///
/// 不能改成「把 uv 坐标绕贴图中心 (0.5,0.5) 旋转」：当 uv 只占贴图一角时
/// （活塞杆是 `[0,0,16,4]`，即 `piston_side` 顶部 16×4 的那条），绕中心一转
/// 采样区域就挪到贴图的另一块地方，于是贴图整个错位。这正是用户报的
/// 「活塞臂贴图渲染位置有错误」。
///
/// 方向：顺时针时，贴图左上角的图案转到面的右上角，即顶点 i 取原来的
/// 角 c_{(i-1)}，故用 `i - k`。down 面按定义反过来。
pub fn rotate_uv_corners(face: Face, uv: [[f32; 2]; 4], rotation: u16) -> [[f32; 2]; 4] {
    let k = ((rotation % 360) / 90) as usize;
    if k == 0 {
        return uv;
    }
    if face == Face::Down {
        // down 面按格式定义是逆时针
        std::array::from_fn(|i| uv[(i + k) % 4])
    } else {
        std::array::from_fn(|i| uv[(i + 4 - k) % 4])
    }
}

/// 按面朝向生成角点。
///
/// `from` / `to` 为元素的两个对角（模型空间 0..16），
/// `uv` 为该面的 [u1, v1, u2, v2]（**未旋转**；旋转由调用方用
/// `rotate_uv_corners` 处理）。
///
/// 各面的 u/v 增长方向由 Minecraft 模型规范固定，不能随意调换：
///   Down  u:+x v:-z     Up    u:+x v:+z
///   North u:-x v:-y     South u:+x v:-y
///   West  u:+z v:-y     East  u:-z v:-y
/// 四个角点依次取 (u1,v1) (u2,v1) (u2,v2) (u1,v2)。
pub fn build_face_corners(face: Face, from: [f32; 3], to: [f32; 3], uv: [f32; 4]) -> FaceCorners {
    let [x1, y1, z1] = from;
    let [x2, y2, z2] = to;
    let [u1, v1, u2, v2] = uv;

    let (pos, uvs) = match face {
        // u 随 +x，v 随 -z
        Face::Down => (
            [[x1, y1, z2], [x2, y1, z2], [x2, y1, z1], [x1, y1, z1]],
            [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        ),
        // u 随 +x，v 随 +z
        Face::Up => (
            [[x1, y2, z1], [x2, y2, z1], [x2, y2, z2], [x1, y2, z2]],
            [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        ),
        // u 随 -x，v 随 -y
        Face::North => (
            [[x2, y2, z1], [x1, y2, z1], [x1, y1, z1], [x2, y1, z1]],
            [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        ),
        // u 随 +x，v 随 -y
        Face::South => (
            [[x1, y2, z2], [x2, y2, z2], [x2, y1, z2], [x1, y1, z2]],
            [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        ),
        // u 随 +z，v 随 -y
        Face::West => (
            [[x1, y2, z1], [x1, y2, z2], [x1, y1, z2], [x1, y1, z1]],
            [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        ),
        // u 随 -z，v 随 -y
        Face::East => (
            [[x2, y2, z2], [x2, y2, z1], [x2, y1, z1], [x2, y1, z2]],
            [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        ),
    };

    FaceCorners { pos, uv: uvs }
}

/// 邻面剔除时，由 cullface 推出的邻居方块偏移
pub fn cull_neighbor_offset(face: Face) -> [i32; 3] {
    match face {
        Face::Down => [0, -1, 0],
        Face::Up => [0, 1, 0],
        Face::North => [0, 0, -1],
        Face::South => [0, 0, 1],
        Face::West => [-1, 0, 0],
        Face::East => [1, 0, 0],
    }
}

/// 各面的明暗系数。数值来自 legacy-cpp 的 FaceShade，
/// 用于在无光照计算的情况下表现立体感；UP 最亮、DOWN 最暗。
pub const FACE_SHADE: [f32; 6] = [
    0.55, // Down
    1.00, // Up
    0.82, // North
    0.72, // South
    0.88, // West
    0.68, // East
];

pub fn face_shade(face: Face) -> f32 {
    FACE_SHADE[face as usize]
}

#[cfg(test)]
#[path = "../tests/render/geometry.rs"]
mod tests;
