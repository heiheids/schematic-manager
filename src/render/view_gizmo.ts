/**
 * 方向标的坐标换算：把世界坐标的三个轴按相机朝向投到屏幕平面上。
 *
 * 抽成独立模块的原因：这段几何换算容易出错（旋转方向搞反、y 轴上下颠倒），
 * 而错了以后界面上只是「小球位置不太对」，很难一眼看出。独立后才能直接用
 * 单测把「正视角下哪个轴该出现在哪」钉死。
 *
 * 注意屏幕坐标系的原点在中心、y 向下；调用方按需自行平移。
 */
import * as THREE from "three";
import { GIZMO_AXES } from "./scene3d";

/** 方向标半径（SVG 用户单位）。绘制与命中检测必须用同一个值。 */
export const GIZMO_RADIUS = 34;

/** 方向标 SVG 的画布半径：半径再留一圈给小球与文字 */
export const GIZMO_VIEW_RADIUS = GIZMO_RADIUS + 12;

/** 端点小球的绘制半径（SVG 用户单位） */
export const GIZMO_DOT_RADIUS = 9;

/** 方向标上的一枚端点小球 */
export interface GizmoDot {
  /** 视角预设的键，点击时用它切视角 */
  key: string;
  /** 轴向标识，如 "+x" */
  axis: string;
  /** 屏幕横坐标（右为正） */
  x: number;
  /** 屏幕纵坐标（下为正，与 SVG 一致） */
  y: number;
  /** 该轴在相机坐标系里的朝向分量：正数表示朝向观察者 */
  depth: number;
}

/** 方向标命中的目标 */
export type GizmoHit = { kind: "axis"; key: string } | { kind: "center" };

/** 中心复位方块的半边长（SVG 用户单位）。模板按它画，命中检测也按它判。 */
export const GIZMO_CENTER_HALF = 7;

/** 中心方块的命中容差（SVG 用户单位） */
const CENTER_SLOP = 2;

/**
 * 判断点落在方向标的哪个可点击目标上。
 *
 * 命中检测放在这里而不是靠 SVG 自己的事件，是因为方向标要让「按住拖动旋转
 * 画面」也能用：若事件被 SVG 收下，画布就收不到 pointerdown，拖动直接失效。
 * 因此 SVG 整块不接收指针事件，改由画布在 click 时把坐标喂进来查一遍。
 *
 * 先判小球、后判中心：俯角大的时候 +Y/−Y 小球几乎压在中心方块上，
 * 若中心优先，用户想切顶视却会触发复位。中心方块比小球小，
 * 让小球优先既符合「点哪个是哪个」，也不会让中心区域变得难以点中。
 *
 * 多个小球在屏幕上可能重叠（斜视角时前后轴几乎重在一起），按距离取最近的。
 * 正对某根轴看过去时，这根轴的两端会投影到同一点，距离完全相等——此时取
 * 朝向观察者的那一端（depth 大），因为它画在另一个上面，用户看到并想点的
 * 就是它。
 */
export function hitGizmo(
  dots: readonly GizmoDot[],
  x: number,
  y: number,
  dotRadius = GIZMO_DOT_RADIUS,
): GizmoHit | null {
  let best: GizmoDot | null = null;
  let bestDist = Infinity;
  for (const d of dots) {
    const dist = Math.hypot(x - d.x, y - d.y);
    if (dist > dotRadius) continue;
    if (dist < bestDist - 1e-9 || (Math.abs(dist - bestDist) <= 1e-9 && best && d.depth > best.depth)) {
      best = d;
      bestDist = dist;
    }
  }
  if (best) return { kind: "axis", key: best.key };
  const reach = GIZMO_CENTER_HALF + CENTER_SLOP;
  if (Math.abs(x) <= reach && Math.abs(y) <= reach) return { kind: "center" };
  return null;
}

/** 轴向标识转单位向量。只在这里用，因此不对外导出。 */
function axisVectorUnchecked(axis: string): THREE.Vector3 {
  return new THREE.Vector3(
    axis === "+x" ? 1 : axis === "-x" ? -1 : 0,
    axis === "+y" ? 1 : axis === "-y" ? -1 : 0,
    axis === "+z" ? 1 : axis === "-z" ? -1 : 0,
  );
}

/**
 * 把六个轴向按相机朝向投到屏幕上。
 *
 * 做法：世界向量乘相机旋转的逆，得到它在相机坐标系里的分量。相机的
 * 局部 +x 是屏幕向右、+y 是屏幕向上，因此 local.x 直接是横坐标，
 * local.y 取负才是 SVG 的纵坐标（SVG 的 y 向下）。
 *
 * 返回结果按 depth 升序（越远的越靠前），调用方顺序绘制即可得到正确遮挡。
 */
export function gizmoDots(
  orientation: THREE.Quaternion | null,
  radius: number,
): GizmoDot[] {
  const inv = orientation ? orientation.clone().invert() : null;
  const dots = GIZMO_AXES.map((entry) => {
    const local = axisVectorUnchecked(entry.axis);
    if (inv) local.applyQuaternion(inv);
    return {
      key: entry.key,
      axis: entry.axis,
      x: local.x * radius,
      y: -local.y * radius,
      depth: local.z,
    };
  });
  return dots.sort((a, b) => a.depth - b.depth);
}
