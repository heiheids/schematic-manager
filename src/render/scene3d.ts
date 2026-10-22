//! 由场景数据构建 three.js 网格，以及离屏渲染成 PNG。
//!
//! 抽成独立模块的原因：批量导出需要渲染**当前没有显示**的投影。
//! 若复用界面上的那个 viewer，就必须先把界面切到每个文件、
//! 等它渲染完再导出，过程既慢又会把用户正在看的画面反复冲掉。
//!
//! 因此这里提供一个独立的离屏渲染器；界面的 SceneViewer 复用其中的
//! 网格构建与取景逻辑，避免两处各写一份而逐渐不一致。

import * as THREE from "three";
import { t } from "../i18n";
import type { SceneData } from "../types";

/** 一个视角预设：朝哪个方向看，以及用透视还是正射 */
export interface ViewPreset {
  /** 偏航角（度）。绕 Y 轴，0 表示朝向 +Z */
  yaw: number;
  /** 俯角（度）。正数表示相机在上方俯视 */
  pitch: number;
  /** 是否用正射投影 */
  ortho: boolean;
}

/**
 * 内置视角预设。
 *
 * 键名是持久化用的标识，**不要改名**：ui-state.json 里存的就是这些键，
 * 改掉会让用户上次停留的视角在重启后失效。新增可以，重命名不行。
 *
 * 六个轴向（顶/底/南/北/东/西）与等距视角并存：轴向用于精确对齐核对，
 * 等距用于看整体。其中「真等距」是等距角度 + 正射，工程制图上常用。
 */
export const VIEW_PRESETS: Record<string, ViewPreset> = {
  // 旧有的四个：默认视角、正对、两个等距（透视）
  left45: { yaw: -45, pitch: 30, ortho: false },
  right45: { yaw: 45, pitch: 30, ortho: false },
  front: { yaw: 0, pitch: 12, ortho: false },
  back: { yaw: 180, pitch: 12, ortho: false },

  // 六个轴向：正对着某一根坐标轴看。俯角取 ±90 时偏航会退化，
  // 此时方向由 pitch 单独决定，presetDir 里已按此处理。
  top: { yaw: 0, pitch: 90, ortho: false },
  bottom: { yaw: 0, pitch: -90, ortho: false },
  south: { yaw: 0, pitch: 0, ortho: false },
  north: { yaw: 180, pitch: 0, ortho: false },
  east: { yaw: 90, pitch: 0, ortho: false },
  west: { yaw: -90, pitch: 0, ortho: false },

  // 两个真等距：与上面两个等距同角度，只是换成正射
  isoRight: { yaw: 45, pitch: 30, ortho: true },
  isoLeft: { yaw: -45, pitch: 30, ortho: true },
};

/** 右键菜单里「预设角度」分组的显示顺序与 i18n 键 */
export const PRESET_MENU: Array<{ key: string; labelKey: string }> = [
  { key: "top", labelKey: "view.top" },
  { key: "bottom", labelKey: "view.bottom" },
  { key: "south", labelKey: "view.south" },
  { key: "north", labelKey: "view.north" },
  { key: "east", labelKey: "view.east" },
  { key: "west", labelKey: "view.west" },
  { key: "right45", labelKey: "view.isoRight" },
  { key: "left45", labelKey: "view.isoLeft" },
  { key: "isoRight", labelKey: "view.trueIsoRight" },
  { key: "isoLeft", labelKey: "view.trueIsoLeft" },
];

/** 方向标六个轴向的键，顺序与小球绘制一致 */
export const GIZMO_AXES = [
  { key: "east", axis: "+x" },
  { key: "west", axis: "-x" },
  { key: "top", axis: "+y" },
  { key: "bottom", axis: "-y" },
  { key: "south", axis: "+z" },
  { key: "north", axis: "-z" },
] as const;

/** 一组已构建的网格及其需要释放的资源 */
export interface SceneGroup {
  group: THREE.Group;
  dispose: () => void;
}

/** 用户保存的视角预设。
 *
 * 存的是完整的相机姿态而不是角度对：正交相机的位置与 zoom 都有意义，
 * 而且以后若要支持「滚动缩放后的视角」也只需用同一份结构。
 */
export interface SavedViewPreset {
  /** 用户起的名字，作为菜单里的显示文本与唯一标识 */
  name: string;
  /** 是否正射投影 */
  ortho: boolean;
  /** 相机位置 */
  position: [number, number, number];
  /** 相机注视的目标点 */
  target: [number, number, number];
  /** 偏航、俯角（度），与内置预设同一套表示 */
  yaw: number;
  pitch: number;
}

/** 名称长度上限。菜单里要横向排得下，也不至于把菜单撑得比屏幕还宽。 */
const PRESET_NAME_MAX = 24;

/** 校验并清洗一条从磁盘读回的自定义预设。
 *
 * 磁盘上的数据可能是旧版本写的、被手工改坏的、或者中途写了一半：
 * 少字段、多余字段、数值为 NaN 都要拦下来。返回 null 表示这条不可用，
 * 调用方丢弃它——把脏值套进相机只会得到一个指向 NaN 的画面。
 */
export function sanitizeViewPreset(raw: unknown): SavedViewPreset | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  const name = typeof o.name === "string" ? o.name.trim() : "";
  if (!name || name.length > PRESET_NAME_MAX) return null;

  const finiteTriple = (v: unknown): [number, number, number] | null => {
    if (!Array.isArray(v) || v.length !== 3) return null;
    const out = v.map((x) => (typeof x === "number" && Number.isFinite(x) ? x : NaN));
    if (out.some((x) => Number.isNaN(x))) return null;
    return [out[0], out[1], out[2]];
  };

  const position = finiteTriple(o.position);
  const target = finiteTriple(o.target);
  if (!position || !target) return null;
  if (typeof o.yaw !== "number" || !Number.isFinite(o.yaw)) return null;
  if (typeof o.pitch !== "number" || !Number.isFinite(o.pitch)) return null;
  // 位置与目标重合时相机没有朝向可言，取景会得到 NaN
  if (Math.hypot(position[0] - target[0], position[1] - target[1], position[2] - target[2]) < 1e-6) {
    return null;
  }

  return {
    name,
    ortho: o.ortho === true,
    position,
    target,
    yaw: o.yaw,
    pitch: o.pitch,
  };
}

/** 校验一批自定义预设：逐条清洗，重名的只留第一条。 */
export function sanitizeViewPresets(raw: unknown): SavedViewPreset[] {
  if (!Array.isArray(raw)) return [];
  const seen = new Set<string>();
  const out: SavedViewPreset[] = [];
  for (const item of raw) {
    const clean = sanitizeViewPreset(item);
    if (!clean) continue;
    if (seen.has(clean.name)) continue;
    seen.add(clean.name);
    out.push(clean);
  }
  return out;
}

/** 名称是否可用：非空、不超长、不与已有预设重名。
 *
 * 重名必须挡住：自定义预设以名字作为菜单项标识，重名会得到两个看起来
 * 一模一样、实际不同的条目，用户无从分辨点的是哪个。
 */
export function isPresetNameUsable(
  name: string,
  existing: readonly SavedViewPreset[],
): boolean {
  const n = name.trim();
  return n.length > 0 && n.length <= PRESET_NAME_MAX && !existing.some((p) => p.name === n);
}

/**
 * 由场景数据构建网格。
 *
 * 每个批次一张贴图、一个材质。顶点位置减去 origin 以居中；
 * UV 在模型空间是 0..16，除以 16 归一化。
 */
export function buildSceneGroup(data: SceneData): SceneGroup {
  const group = new THREE.Group();
  const disposables: Array<{ dispose: () => void }> = [];

  for (const batch of data.batches) {
    const geo = new THREE.BufferGeometry();
    const pos = new Float32Array(batch.positions);
    for (let i = 0; i < pos.length; i += 3) {
      pos[i] -= data.origin[0];
      pos[i + 1] -= data.origin[1];
      pos[i + 2] -= data.origin[2];
    }
    geo.setAttribute("position", new THREE.BufferAttribute(pos, 3));

    const uvRaw = new Float32Array(batch.uvs);
    const uv = new Float32Array(uvRaw.length);
    for (let i = 0; i < uvRaw.length; i++) uv[i] = uvRaw[i] / 16;
    geo.setAttribute("uv", new THREE.BufferAttribute(uv, 2));

    // 顶点颜色直接来自后端：它已经把「面明暗」与「方块染色」乘好。
    // 材质不参与光照计算，立体感与染色全部由这里承载。
    geo.setAttribute(
      "color",
      new THREE.BufferAttribute(new Float32Array(batch.colors), 3),
    );
    geo.setIndex(new THREE.BufferAttribute(new Uint32Array(batch.indices), 1));
    geo.computeBoundingBox();

    const tex = new THREE.DataTexture(
      new Uint8Array(batch.textureRgba),
      batch.textureWidth,
      batch.textureHeight,
      THREE.RGBAFormat,
    );
    // Minecraft 贴图按方块单位采样，必须最近邻，否则边缘会糊
    tex.magFilter = THREE.NearestFilter;
    tex.minFilter = THREE.NearestFilter;
    tex.generateMipmaps = false;
    tex.colorSpace = THREE.SRGBColorSpace;
    tex.needsUpdate = true;

    // 按贴图的透明类型决定绘制方式。三者必须分开：
    //
    // - opaque：全不透明，正常写深度，正常互相遮挡。
    // - cutout：只有全透明与全不透明像素（树叶、草、藤蔓）。
    //   用 alphaTest 恰好合适——它把全透明像素丢掉，画出来的部分照常写深度。
    //   若改用混合，树叶之间会互相透视，看着像半透明的塑料片。
    //   阈值取 0.5：贴图只有 0 与 255，取任何 0..1 之间的值结果都一样。
    // - translucent：含真正的半透明像素（染色玻璃、水）。**不能用 alphaTest**：
    //   实测白色/黄色染色玻璃的贴图各有 191 个像素的 alpha 落在 1..127，
    //   它们会被整片丢弃，玻璃上出现窟窿。必须混合，并且不写深度，
    //   否则半透明的面会把后面的几何挡掉。
    const isCutout = batch.alphaMode === "cutout";
    const isTranslucent = batch.alphaMode === "translucent";
    const mat = new THREE.MeshBasicMaterial({
      map: tex,
      vertexColors: true,
      // 后端给出的三角形绕序已使其法线朝外，因此正面渲染即可。
      // 用 DoubleSide 会同时画出方块内部的面，透视效果反而不对。
      side: THREE.FrontSide,
      transparent: isTranslucent,
      alphaTest: isCutout ? 0.5 : 0,
      depthWrite: !isTranslucent,
    });

    const mesh = new THREE.Mesh(geo, mat);
    group.add(mesh);
    disposables.push(geo, tex, mat);
  }

  return {
    group,
    dispose: () => {
      for (const d of disposables) d.dispose();
      disposables.length = 0;
      group.clear();
    },
  };
}

/**
 * 求「把整个网格装进视口」所需的相机距离。
 *
 * 不能用外接球半径：外接球按对角线算直径，对细长结构等于要求把
 * 「对角线那么长的球」塞进视口高度，而结构在屏幕上的实际投影高度可能只有
 * 它的几分之一。实测一个 53×9×9 的机器只占画面 44.6%，四周一大片空白。
 *
 * 这里按**投影后的包围盒**拟合：把包围盒八个角点投到相机的横/纵轴上，
 * 分别求两个方向需要的最小距离，取较大者。对某个角点（相对中心偏移 x,y,z，
 * z 为正表示更远），它到相机的距离是 d+z，要落进视锥就要求
 * `|y| <= (d+z)·tan(fovY/2)` 与 `|x| <= (d+z)·tan(fovX/2)`，解出 d 的下界。
 *
 * `margin` 是留白系数：除以它相当于把可用视锥收窄一点，模型不至于顶到边。
 */
export function fitDistance(
  box: THREE.Box3,
  dir: THREE.Vector3,
  fovDeg: number,
  aspect: number,
  margin = 1.08,
): number {
  const center = box.getCenter(new THREE.Vector3());
  const forward = dir.clone().normalize();
  // 视线与世界上方平行时叉乘会退化，换一个不平行的参考方向
  const upHint =
    Math.abs(forward.y) > 0.99 ? new THREE.Vector3(0, 0, 1) : new THREE.Vector3(0, 1, 0);
  const right = new THREE.Vector3().crossVectors(forward, upHint).normalize();
  const up = new THREE.Vector3().crossVectors(right, forward).normalize();
  const tanY = Math.tan((fovDeg * Math.PI) / 360) / margin;
  const tanX = tanY * Math.max(aspect, 1e-3);

  let dist = 0;
  const v = new THREE.Vector3();
  for (let i = 0; i < 8; i++) {
    v.set(
      i & 1 ? box.max.x : box.min.x,
      i & 2 ? box.max.y : box.min.y,
      i & 4 ? box.max.z : box.min.z,
    ).sub(center);
    const x = v.dot(right);
    const y = v.dot(up);
    const z = v.dot(forward);
    dist = Math.max(dist, Math.abs(y) / tanY - z, Math.abs(x) / tanX - z);
  }
  // 退化包围盒（单点）时给一个非零距离，避免相机贴在模型上
  return Math.max(dist, 1e-3);
}

/**
 * 默认视角的「相机 → 目标」方向。
 *
 * 相机因此落在模型中心的 (-1,-1,-1) 一侧，也就是 **+x +y +z 那一侧**——
 * 与改造前的取景位置一致。方向写反了相机就会跑到模型的另一侧：正面的三角形
 * 会被背面剔除，界面上看到的是**一片空白**（实测就是这样把预览画没的）。
 */
export function defaultViewDir(): THREE.Vector3 {
  return new THREE.Vector3(-1, -1, -1).normalize();
}

/**
 * 由预设角度（偏航、俯角，单位为度）求「相机 → 目标」的方向向量。
 *
 * pitch 取 ±90 时 cos(pitch) 为 0，yaw 不再影响方向——顶视与底视因此
 * 写成 (0,∓1,0)，这是正确行为而不是退化 bug。
 *
 * 界面上的取景路径用它；未知键退回默认方向，因为调用方已经在处理
 * 「这个视角不存在」的兜底（见 tryPresetDir 说明）。
 */
export function presetDir(name: string): THREE.Vector3 {
  return tryPresetDir(name) ?? dirFromAngles(VIEW_PRESETS.front.yaw, VIEW_PRESETS.front.pitch);
}

/**
 * 查表取预设方向；键不存在时返回 null。
 *
 * 用 hasOwnProperty 而不是 `name in VIEW_PRESETS`：后者会被原型链击穿，
 * `"constructor"`、`"toString"` 这类键都会被判成「存在」，再一路算出 NaN。
 * 只认表里自己声明的键。
 */
export function tryPresetDir(name: string): THREE.Vector3 | null {
  if (!isBuiltinView(name)) return null;
  const preset = VIEW_PRESETS[name];
  return dirFromAngles(preset.yaw, preset.pitch);
}

/** 某个键是不是内置视角预设。
 *
 * 不用 `Object.hasOwn`（要求 ES2022，本项目目标是 ES2021）；
 * `name in obj` 也不行，它认原型链上的 `constructor`、`toString` 等键。
 */
export function isBuiltinView(name: string): boolean {
  return Object.prototype.hasOwnProperty.call(VIEW_PRESETS, name);
}

/** 由角度求方向向量。与 presetDir 同源，自定义预设走这里。 */
export function dirFromAngles(yawDeg: number, pitchDeg: number): THREE.Vector3 {
  const yaw = (yawDeg * Math.PI) / 180;
  const pitch = (pitchDeg * Math.PI) / 180;
  return new THREE.Vector3(
    Math.cos(pitch) * Math.sin(yaw),
    -Math.sin(pitch),
    Math.cos(pitch) * Math.cos(yaw),
  );
}

/** 由相机位置与观察目标反求 yaw / pitch（度）。用于把当前视角存成预设。
 *
 * 两个角度的符号都容易搞错，这里一次说清：presetDir 给的是「相机 → 目标」，
 * 而 `position - target` 得到的是「目标 → 相机」，两者互为反向。反向意味着
 * x、y、z 三个分量同时取反，于是
 *   - pitch：y = -sin(pitch) 在反向侧变成 y = sin(pitch)，取 +asin；
 *   - yaw：x 与 z 同时取反，等价于 yaw 加 180°（atan2(-x,-z) = atan2(x,z)+π）。
 * 漏掉任何一处，保存下来的预设用一次就会跑到模型另一侧。
 */
export function anglesFromPosition(
  position: THREE.Vector3,
  target: THREE.Vector3,
): { yaw: number; pitch: number } {
  const dir = new THREE.Vector3().subVectors(position, target);
  if (dir.lengthSq() === 0) return { yaw: 0, pitch: 0 };
  dir.normalize();
  const pitch = (Math.asin(THREE.MathUtils.clamp(dir.y, -1, 1)) * 180) / Math.PI;
  // 俯视/仰视到底时水平分量为 0，yaw 无意义，沿用 0
  const horiz = Math.hypot(dir.x, dir.z);
  if (horiz < 1e-6) return { yaw: 0, pitch };
  let yaw = (Math.atan2(dir.x, dir.z) * 180) / Math.PI + 180;
  // 归一到 [-180, 180)，与界面里自定义预设的角度表示保持一致
  yaw = (((yaw + 180) % 360) + 360) % 360 - 180;
  return { yaw, pitch };
}

/** 某个预设是否用正射投影。
 *
 * 未知键返回 false（按透视处理）：这只决定「切过去用哪台相机」，
 * 退到透视不会让画面指向错误的角度，而未知道具在取景那一步会被挡下。
 */
export function presetIsOrtho(name: string): boolean {
  if (!isBuiltinView(name)) return false;
  return VIEW_PRESETS[name].ortho;
}

/**
 * 求把整个包围盒装进正交视锥所需的半宽 / 半高。
 *
 * 关键点：视线方向的横纵基**必须取自相机自身**（matrixWorld 的 x/y 轴），
 * 不能在这里另算一套。否则 `frameOrthoBox` 里 `lookAt` 定出的朝向与这里
 * 用 `cross(forward, up)` 算出的基可能左右相反，算出来的尺寸对不上实际视锥，
 * 表现为「模型被裁掉一部分」——实测斜 45° 视角下横向溢出近 90%。
 *
 * 调用前相机的位置与朝向必须已经摆好（先 lookAt，再调这里）。
 */
export function fitOrthoHalfExtents(
  camera: THREE.Camera,
  box: THREE.Box3,
  aspect: number,
  margin = 1.08,
): { halfW: number; halfH: number } {
  // 相机矩阵要最新，否则取到的是上一次的朝向
  camera.updateMatrixWorld(true);
  const center = box.getCenter(new THREE.Vector3());
  const right = new THREE.Vector3().setFromMatrixColumn(camera.matrixWorld, 0).normalize();
  const up = new THREE.Vector3().setFromMatrixColumn(camera.matrixWorld, 1).normalize();

  let halfW = 0;
  let halfH = 0;
  const v = new THREE.Vector3();
  for (let i = 0; i < 8; i++) {
    v.set(
      i & 1 ? box.max.x : box.min.x,
      i & 2 ? box.max.y : box.min.y,
      i & 4 ? box.max.z : box.min.z,
    ).sub(center);
    halfW = Math.max(halfW, Math.abs(v.dot(right)));
    halfH = Math.max(halfH, Math.abs(v.dot(up)));
  }
  // 视锥的宽高比必须等于画布宽高比，否则画面会被拉伸。
  // 已算出的 halfW / halfH 是「装下模型至少需要多大」，因此只能往大里补：
  // 模型比画布「更方」时补宽，比画布「更扁」时补高，绝不缩小任何一边。
  const needRatio = halfW / Math.max(halfH, 1e-6);
  let finalW: number;
  let finalH: number;
  if (needRatio < aspect) {
    finalH = halfH;
    finalW = halfH * aspect;
  } else {
    finalW = halfW;
    finalH = halfW / Math.max(aspect, 1e-6);
  }
  return {
    halfW: Math.max(finalW * margin, 1e-3),
    halfH: Math.max(finalH * margin, 1e-3),
  };
}

/**
 * 把一个正交相机摆到能完整看到整个网格的位置。
 *
 * 相机位置离模型多远都行（正交投影下不影响成像大小），但仍要放到包围盒
 * 外侧并留出足够距离，否则 near 会切到模型。距离取包围盒对角线长度，
 * 足够包住任何朝向。
 */
export function frameOrthoBox(
  camera: THREE.OrthographicCamera,
  box: THREE.Box3,
  dir: THREE.Vector3,
  aspect: number,
) {
  const center = box.getCenter(new THREE.Vector3());
  const size = box.getSize(new THREE.Vector3());
  const diag = Math.max(size.length(), 1e-3);
  // 站在对角线外两倍处：任何朝向都在相机前方
  camera.position.copy(center).addScaledVector(dir.clone().normalize(), -diag * 2);
  camera.up.copy(cameraUpFor(dir));
  camera.lookAt(center);

  // 朝向必须**先**摆好再算视景体：fitOrthoHalfExtents 读的是相机矩阵里的
  // 横纵轴，顺序反了就会按上一帧的朝向算
  const { halfW, halfH } = fitOrthoHalfExtents(camera, box, aspect);
  camera.left = -halfW;
  camera.right = halfW;
  camera.top = halfH;
  camera.bottom = -halfH;
  camera.zoom = 1;
  // 记下取景基准：之后换宽高比在这个半高上重算，不吃掉用户已经看到的范围
  camera.userData.orthoBaseHalfHeight = halfH;
  camera.near = diag * 0.05;
  camera.far = diag * 5;
  camera.updateProjectionMatrix();
  return { center, dist: diag * 2 };
}

/**
 * 由视线方向求相机该用的 up 向量。
 *
 * 平时用世界 +Y；但俯视/仰视到底时视线与 +Y 平行，`lookAt` 会退化（three
 * 内部靠 up 与视线叉乘求右方向，平行时叉乘为零向量，得到 NaN 或任意朝向）。
 *
 * 退化时改用 +Z 还是 -Z 必须按「屏幕右方应当是 +X」来定：
 *   - 顶视（相机在上方朝下看，dir 的 y 为负）：right = up × (-dir)。
 *     取 up = (0,0,-1) 得 right = (+1,0,0)，即 +X 朝右，与俯视图的通行画法一致；
 *     取 up = (0,0,+1) 会得到 -X 朝右，画面成了左右镜像
 *     （实测文字「TiPlus9100」被镜像成「-0019sulPiT」）。
 *   - 底视方向相反，up 也要反过来，才能保持 +X 仍然朝右。
 */
export function cameraUpFor(dir: THREE.Vector3): THREE.Vector3 {
  const d = dir.clone().normalize();
  if (Math.abs(d.y) > 0.99) {
    // 相机朝下看（d.y < 0）时 up 取 -Z；朝上看时取 +Z
    return new THREE.Vector3(0, 0, d.y < 0 ? -1 : 1);
  }
  return new THREE.Vector3(0, 1, 0);
}

/**
 * 把相机摆到「整个网格装进视口」的位置。
 *
 * 保持调用方给的视线方向，因此换视角后再自适应也不会把用户的视角掰回默认。
 */
export function frameBox(
  camera: THREE.PerspectiveCamera,
  box: THREE.Box3,
  dir: THREE.Vector3,
) {
  const center = box.getCenter(new THREE.Vector3());
  const dist = fitDistance(box, dir, camera.fov, camera.aspect);
  camera.position.copy(center).addScaledVector(dir.clone().normalize(), -dist);
  camera.up.copy(cameraUpFor(dir));
  camera.lookAt(center);
  // 近裁剪面按距离给，缩放时不会切掉模型；远裁剪面留足余量
  camera.near = Math.max(dist / 100, 0.02);
  camera.far = Math.max(dist * 12, 100);
  camera.updateProjectionMatrix();
  return { center, dist };
}

/** 把相机拉到能完整看到整个网格的位置（默认视角） */
export function fitCamera(camera: THREE.PerspectiveCamera, group: THREE.Group) {
  const box = new THREE.Box3().setFromObject(group);
  if (box.isEmpty()) {
    // 空场景：放一个固定位置，避免 Box3 给出 NaN 把相机弄成不可用状态
    camera.position.set(12, 12, 12);
    camera.lookAt(0, 0, 0);
    camera.near = 0.1;
    camera.far = 1000;
    camera.updateProjectionMatrix();
    return;
  }
  frameBox(camera, box, defaultViewDir());
}

/**
 * 按预设视角摆放相机。
 *
 * 与界面上的「正面 / 左侧 45° / 右侧 45° / 背面」使用同一套角度定义，
 * 导出结果才会和用户在界面上看到的一致。
 *
 * 未知的视角名直接抛错：这个函数给批量导出用，静默退回某个默认视角会让
 * 用户拿到一整批角度不对的图，却以为是自己选了那个角度。
 */
export function applyPreset(
  camera: THREE.PerspectiveCamera,
  group: THREE.Group,
  name: string,
) {
  const dir = tryPresetDir(name);
  if (!dir) throw new Error(`未知的视角预设：${name}`);
  const box = new THREE.Box3().setFromObject(group);
  if (box.isEmpty()) {
    fitCamera(camera, group);
    return;
  }
  frameBox(camera, box, dir);
}

/**
 * 按一条自定义预设的位姿摆放导出相机。
 *
 * 与内置预设的区别：内置预设按模型重新算距离（「完整装下」），自定义预设
 * 存的是用户当时的位置，照原样还原才是他要的那个画面。
 */
export function applySavedPose(
  camera: THREE.PerspectiveCamera,
  preset: SavedViewPreset,
) {
  const target = new THREE.Vector3(...preset.target);
  const position = new THREE.Vector3(...preset.position);
  const dir = new THREE.Vector3().subVectors(target, position);
  if (dir.lengthSq() < 1e-12) {
    // 位姿退化（位置与目标重合）不该静默出一张随机角度的图
    throw new Error(`自定义预设「${preset.name}」的位置与注视点重合`);
  }
  camera.position.copy(position);
  camera.up.copy(cameraUpFor(dir));
  camera.lookAt(target);
  const dist = position.distanceTo(target);
  camera.near = Math.max(dist / 1000, 0.01);
  camera.far = Math.max(dist * 12, 100);
  camera.updateProjectionMatrix();
}

/**
 * 滚轮缩放的距离区间，按自适应后的基准距离定出来。
 *
 * 必须跟着模型大小走：一个 3 格高的小机器和一个 50 格长的大机器，
 * 用同一组绝对距离必然一边贴脸一边成点。
 */
export function zoomLimits(fitDist: number): { min: number; max: number } {
  return {
    min: Math.max(fitDist * 0.06, 0.05),
    max: Math.max(fitDist * 6, 1),
  };
}

/**
 * 正交相机的滚轮缩放区间。
 *
 * 正交相机没有「距离」这个概念，OrbitControls 改的是 camera.zoom，
 * 因此上下限要按 zoom 给，而不是按距离给。范围与透视侧同样取六倍余量。
 */
export function orthoZoomLimits(): { min: number; max: number } {
  return { min: 0.06, max: 6 };
}

/**
 * 透视相机在距离 dist 处看到的画面半高（竖直方向）。
 *
 * 两种投影切换时靠它对齐视野：正交相机没有 FOV，只有把「透视此刻的可见
 * 范围」换算成视景体，「同一角度换个投影」才真的只是换投影、画面不跳。
 */
export function perspectiveViewHalfHeight(dist: number, fovDeg: number): number {
  return Math.max(dist, 1e-6) * Math.tan((fovDeg * Math.PI) / 360);
}

/**
 * 正交相机当前「可见世界范围」的半宽 / 半高。
 *
 * 视景体（left/right/top/bottom）与 zoom 是两层缩放：最终可见范围是
 * `right / zoom`，不是 `right`。混用这两者会让缩放过的视角在每次
 * resize 后被再缩一遍。
 */
export function orthoVisibleExtent(cam: THREE.OrthographicCamera): {
  halfW: number;
  halfH: number;
} {
  const zoom = Math.max(cam.zoom, 1e-6);
  return { halfW: cam.right / zoom, halfH: cam.top / zoom };
}

/**
 * 把正交视景体设成「可见半高为 halfH、宽高比为 aspect」。
 *
 * 保持 zoom 不变：zoom 是用户滚轮调出来的缩放，取景函数不该把它抹掉。
 */
export function setOrthoViewExtent(
  cam: THREE.OrthographicCamera,
  halfH: number,
  aspect: number,
): void {
  const h = Math.max(halfH, 1e-3);
  const w = h * Math.max(aspect, 1e-3);
  // 记下取景基准：之后换宽高比只在这个基准上重算，不拿当前视景体反复推
  cam.userData.orthoBaseHalfHeight = h;
  const zoom = Math.max(cam.zoom, 1e-6);
  cam.left = -w * zoom;
  cam.right = w * zoom;
  cam.top = h * zoom;
  cam.bottom = -h * zoom;
  cam.updateProjectionMatrix();
}

/**
 * 按新的画布宽高比重设正交视景体。
 *
 * 竖直方向的取景是基准，水平方向交给宽高比决定——这是正交投影的通行做法，
 * 也是唯一稳定的做法：若每次都拿「当前视景体」去推下一次，撑大的宽度会被
 * 当成新基准，反复拖动分隔条就会让画面越缩越小且不可逆。
 *
 * 基准半高由取景函数写下（`frameOrthoBox` / `setOrthoViewExtent`），
 * 这里只读不改，因此同一宽高比无论经历多少次 resize 都回到同一个视景体。
 */
export function adaptOrthoAspect(cam: THREE.OrthographicCamera, aspect: number): void {
  const a = Math.max(aspect, 1e-3);
  setOrthoViewExtent(cam, orthoBaseHalfHeight(cam), a);
}

/** 正交相机的取景基准半高（不含 zoom）。
 *
 * 取景函数会写进 `userData`；没写过时（例如外部直接给了视景体）退回当前
 * 可见半高，并把它记成基准，后续 resize 才有稳定的参照。
 */
export function orthoBaseHalfHeight(cam: THREE.OrthographicCamera): number {
  const stored = cam.userData.orthoBaseHalfHeight;
  if (typeof stored === "number" && Number.isFinite(stored) && stored > 0) return stored;
  const halfH = Math.max(orthoVisibleExtent(cam).halfH, 1e-3);
  cam.userData.orthoBaseHalfHeight = halfH;
  return halfH;
}

/**
 * 按当前相机距离更新近/远裁剪面。
 *
 * OrbitControls 只管位置，**不管裁剪面**：取景时把 near 设成 dist/100 之后，
 * 用滚轮一直拉近就会穿进近裁剪面，模型被整片切掉——表现是「缩放几下画面就
 * 空了」。远裁剪面同理，拉远到头会把模型裁没。因此距离一变就要跟着更新。
 *
 * 返回是否真的变了，调用方据此决定要不要重绘。
 */
export function syncClipPlanes(
  camera: THREE.PerspectiveCamera,
  target: THREE.Vector3,
): boolean {
  const dist = camera.position.distanceTo(target);
  if (!Number.isFinite(dist) || dist <= 0) return false;
  const near = Math.max(dist / 100, 0.02);
  const far = Math.max(dist * 12, 100);
  if (near === camera.near && far === camera.far) return false;
  camera.near = near;
  camera.far = far;
  camera.updateProjectionMatrix();
  return true;
}

/**
 * 导出尺寸的宽高比预设。
 *
 * 预览区可以单独调宽与调高，画面未必是正方形，导出也就不该只给正方形。
 * 宽与高都用「相对单位」，实际像素由短边长度乘出来（见 exportPixelSize）。
 */
export const EXPORT_ASPECTS = [
  { key: "1x1", w: 1, h: 1 },
  { key: "4x3", w: 4, h: 3 },
  { key: "3x4", w: 3, h: 4 },
  { key: "16x9", w: 16, h: 9 },
  { key: "9x16", w: 9, h: 16 },
] as const;

/** 导出尺寸的短边像素档位 */
export const EXPORT_SHORT_EDGES = [1024, 2048, 3072, 4096] as const;

/** 导出尺寸的宽高比标识 */
export type ExportAspect = (typeof EXPORT_ASPECTS)[number]["key"];

/** 由宽高比与短边像素算出实际导出的宽高（像素）。
 *
 * 「短边」按名字理解：宽高比里较小的那一边就是短边，它取给定像素数，
 * 长边按比例放大。3:4 这种竖构图因此以宽为短边，而不是把宽硬当成给定的数。
 *
 * 乘完取整，并保证不小于 1——否则极端比例下会算出 0 宽 0 高的画布。
 */
export function exportPixelSize(aspect: ExportAspect, shortEdge: number): { w: number; h: number } {
  const preset = EXPORT_ASPECTS.find((a) => a.key === aspect) ?? EXPORT_ASPECTS[0];
  const ratio = preset.w / preset.h;
  if (ratio >= 1) {
    // 横构图或正方：高是短边
    return { w: Math.max(1, Math.round(shortEdge * ratio)), h: Math.max(1, shortEdge) };
  }
  // 竖构图：宽是短边
  return { w: Math.max(1, shortEdge), h: Math.max(1, Math.round(shortEdge / ratio)) };
}

/**
 * 离屏渲染器：把任意场景数据渲染成透明背景的 PNG。
 *
 * 复用同一个 WebGL 上下文完成一次批量导出，而不是每个文件新建一个——
 * 浏览器同时可用的上下文数量有限（通常 16 个），逐个新建却不释放会
 * 在中途开始失败。用完由调用方 dispose()。
 */
export class OffscreenRenderer {
  private renderer: THREE.WebGLRenderer;
  private scene: THREE.Scene;
  private camera: THREE.PerspectiveCamera;

  constructor() {
    this.renderer = new THREE.WebGLRenderer({
      antialias: true,
      alpha: true,
      // 读像素前不能让浏览器丢弃绘制缓冲，否则导出得到空白图
      preserveDrawingBuffer: true,
    });
    this.renderer.setPixelRatio(1);
    this.renderer.setClearColor(0x000000, 0);
    this.renderer.setClearAlpha(0);
    this.scene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
  }

  /** 渲染一个场景并返回 PNG 字节。
   *
   * `view` 可以是内置视角名，也可以是自定义预设的位姿——批量导出时用户
   * 可能正停在一条自定义预设上，那时只有位姿能还原他要的角度。
   * 两样都不给才退回正面。
   */
  async renderPng(
    data: SceneData,
    w: number,
    h: number,
    view: string | SavedViewPreset = "front",
  ): Promise<Uint8Array> {
    const built = buildSceneGroup(data);
    try {
      this.scene.add(built.group);
      this.renderer.setSize(w, h, false);
      this.camera.aspect = w / h;
      if (typeof view === "string") {
        applyPreset(this.camera, built.group, view);
      } else {
        applySavedPose(this.camera, view);
      }
      this.renderer.render(this.scene, this.camera);

      const blob: Blob = await new Promise((resolve, reject) => {
        this.renderer.domElement.toBlob(
          (b) => (b ? resolve(b) : reject(new Error(t("export.canvasEmpty")))),
          "image/png",
        );
      });
      return new Uint8Array(await blob.arrayBuffer());
    } finally {
      this.scene.remove(built.group);
      built.dispose();
    }
  }

  dispose() {
    this.renderer.dispose();
    this.renderer.forceContextLoss();
  }
}
