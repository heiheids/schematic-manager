//! 资源池：按内容去重。
//!
//! 用途：构建资源包时，同一个模型会被大量方块与变体引用，同一张贴图也会
//! 被多个模型引用。按内容去重能显著缩小产物体积，也避免读取端重复解析。
//!
//! 对照 legacy-cpp/packbuild.cpp 的 texByHash 与 modelByHash。
//!
//! 去重键必须是**内容**而非名称：不同路径的贴图内容可能相同（复制粘贴的
//! 资源包很常见），同名不同内容的情形也存在。
//!
//! 纯逻辑，不涉及文件 IO，因此可完整测试。

use std::collections::HashMap;

/// 内容指纹：64 位 FNV-1a。
///
/// 只用于在哈希表里快速定位候选，**不承担相等判定**——真正的相等由
/// 内容本身逐字节比较得出。哈希碰撞若被当成相等，会让某个模型用错贴图，
/// 这种错误不会崩溃、只会让画面出错，因此必须避免。
fn fingerprint(data: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// 按内容去重的资源池。
///
/// 池里同时保存「内容的字节表示」与「解析后的项」，因此相等判定是
/// 逐字节比较内容，而不是比较指针或指纹。
#[derive(Debug, Clone)]
pub struct ContentPool<T> {
    items: Vec<T>,
    /// 与 items 一一对应的内容字节
    keys: Vec<Vec<u8>>,
    /// 指纹 -> 该指纹下的项下标（可能多个，指纹可能碰撞）
    by_hash: HashMap<u64, Vec<usize>>,
}

impl<T> Default for ContentPool<T> {
    fn default() -> Self {
        Self { items: Vec::new(), keys: Vec::new(), by_hash: HashMap::new() }
    }
}

impl<T> ContentPool<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// 取内容对应的下标；不存在则调用 `make` 构造并加入。
    ///
    /// 相等判定是 `key` 的逐字节比较：同指纹只说明「可能相同」，
    /// 必须再比内容才能确认。
    pub fn intern_by_key<F>(&mut self, key: &[u8], make: F) -> usize
    where
        F: FnOnce() -> T,
    {
        let h = fingerprint(key);
        if let Some(cands) = self.by_hash.get(&h) {
            for &i in cands {
                if self.keys[i] == key {
                    return i;
                }
            }
        }
        let idx = self.items.len();
        self.items.push(make());
        self.keys.push(key.to_vec());
        self.by_hash.entry(h).or_default().push(idx);
        idx
    }

    /// 取内容对应的下标；不存在则加入 `item` 本身
    pub fn intern(&mut self, key: &[u8], item: T) -> usize {
        self.intern_by_key(key, || item)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }
}

/// 把模型编码成用于去重的字节串。
///
/// 对照 legacy-cpp 的 ModelHash：逐元素、逐面地把影响渲染结果的字段
/// 全部纳入。**小数点后保留 3 位**是原实现的做法，也符合模型精度的
/// 实际量级（Minecraft 模型坐标为 1/16 方块，3 位足够区分）。
///
/// 未纳入的字段会影响去重正确性：漏一个就会把本应不同的两个模型合并，
/// 表现为某些方块的朝向或贴图错误。因此这里逐一列出所有渲染相关字段。
pub fn model_key(m: &crate::block::db::Model) -> Vec<u8> {
    let mut s = String::new();
    s.push(if m.translucent { '1' } else { '0' });
    for e in &m.elements {
        // 位置与尺寸
        s.push_str(&format!(
            "|{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
            e.from[0], e.from[1], e.from[2], e.to[0], e.to[1], e.to[2]
        ));
        // 旋转
        s.push_str(&format!(
            ",{},{}", if e.has_rotation { 1 } else { 0 }, e.rot_kind
        ));
        if e.has_rotation {
            s.push_str(&format!(
                ",{:.3},{:.3},{:.3},{},{:.3},{}",
                e.origin[0], e.origin[1], e.origin[2], e.axis, e.angle,
                if e.rescale { 1 } else { 0 }
            ));
            s.push_str(&format!(
                ",{:.3},{:.3},{:.3}",
                e.rot_vec[0], e.rot_vec[1], e.rot_vec[2]
            ));
        }
        s.push_str(&format!(",{}", e.face_mask));
        // 面：按顺序参与，因为读取端就是按 faceMask 的位序取面的
        for f in &e.faces {
            s.push_str(&format!(
                "#{},{}", f.texture, if f.cullface == 0xFF { -1 } else { f.cullface as i32 }
            ));
            s.push_str(&format!(
                ",{:.3},{:.3},{:.3},{:.3},{},{}",
                f.uv[0], f.uv[1], f.uv[2], f.uv[3], f.tintindex, f.rotation
            ));
        }
    }
    s.into_bytes()
}

#[cfg(test)]
#[path = "../tests/block/pool.rs"]
mod tests;
