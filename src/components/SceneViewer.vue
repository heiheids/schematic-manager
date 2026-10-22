<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { t } from "../i18n";
import type { SceneData } from "../types";
import ViewGizmo from "./ViewGizmo.vue";
import {
  GIZMO_RADIUS,
  GIZMO_VIEW_RADIUS,
  gizmoDots,
  hitGizmo,
  type GizmoHit,
} from "../render/view_gizmo";
import {
  adaptOrthoAspect,
  buildSceneGroup,
  cameraUpFor,
  defaultViewDir,
  dirFromAngles,
  fitCamera as fitCameraTo,
  frameBox,
  frameOrthoBox,
  orthoVisibleExtent,
  orthoZoomLimits,
  perspectiveViewHalfHeight,
  presetDir,
  presetIsOrtho,
  setOrthoViewExtent,
  syncClipPlanes,
  zoomLimits,
  type SavedViewPreset,
  type SceneGroup,
} from "../render/scene3d";

const props = withDefaults(
  defineProps<{
    scene: SceneData | null;
    /** 亮度系数，1.0 为原始 */
    brightness?: number;
  }>(),
  { brightness: 1.0 },
);

/** 方向标点了某个轴向时通知父级。
 *
 * 视角状态（activeView）由 store 持有，这里不直接改它——否则同一次操作会
 * 出现两条改状态的路径，界面上的高亮与实际视角迟早对不上。父级收到事件后
 * 调 store 的 setView，再由 store 回头调用本组件的 applyViewPreset。
 */
const emit = defineEmits<{
  (e: "pickView", key: string): void;
}>();

const host = ref<HTMLDivElement | null>(null);
const error = ref("");

/** 方向标元素，供命中检测读取它在屏幕上的位置 */
const gizmoEl = ref<InstanceType<typeof ViewGizmo> | null>(null);

/** 按下与松开的最大位移（像素）：超过就当成拖拽，不触发方向标点击 */
const GIZMO_CLICK_SLOP = 4;

/** 画布上按下的位置，用来区分「点击」与「拖拽后松开」 */
let pressAt: { x: number; y: number } | null = null;

// three 的对象不需要响应式代理（会被深度追踪，既慢又可能出错）
const renderer = shallowRef<THREE.WebGLRenderer | null>(null);
let built: SceneGroup | null = null;
let threeScene: THREE.Scene | null = null;
/** 透视相机：默认使用 */
let perspCamera: THREE.PerspectiveCamera | null = null;
/** 正交相机：切到「正射投影」时使用 */
let orthoCamera: THREE.OrthographicCamera | null = null;
/** 当前参与渲染与交互的那一个。
 *
 * 两个相机共享同一套位置与朝向，切换投影方式时只换对象、不重算视角，
 * 用户看到的是「同一个角度换了投影」，而不是画面忽然跳一下。
 */
let useOrtho = false;
let controls: OrbitControls | null = null;
// 按需渲染：只在场景或相机变化时重绘。持续跑 requestAnimationFrame
// 会让笔记本常态占用 GPU，而这是静态预览，不操作时画面不会变。
let raf = 0;
let needsRender = true;

/** 上一次结算时的已渲染帧数。每秒结算一次，差值就是这一秒的帧率。 */
let fpsFrames = 0;
let fpsMark = 0;

/** 实际渲染帧率。按需渲染下静止时是 0——这正是要看的信息：
 *  0 表示「当前没有在画」，而不是「卡死了」。
 *
 * 不计入 exportPng 里那一次离屏渲染：它不是主循环画的帧，
 * 算进去会让导出瞬间的读数虚高。 */
const fps = ref(0);

/** 相机朝向，供方向标绘制。只在朝向变化时更新——每帧写响应式 ref 会让
 *  Vue 持续重渲染，而方向标只在相机转动时才需要重画。 */
const orientation = ref<THREE.Quaternion | null>(null);

/** 把当前相机朝向同步给方向标。
 *
 * 只在朝向真的变了时才写 ref：每帧都写会让 Vue 持续重渲染，
 * 而方向标只有在相机转动时才需要重画。滚轮缩放会触发 controls 的
 * change，但那不改朝向——挡掉重复写才不会白白重渲方向标。
 */
function syncOrientation() {
  const cam = activeCamera();
  const next = cam ? cam.quaternion : null;
  const prev = orientation.value;
  if (next && prev && prev.angleTo(next) < 1e-6) return;
  orientation.value = next ? next.clone() : null;
}

/** 累计一帧并每秒结算一次 */
function countFrame(now: number) {
  fpsFrames++;
  if (fpsMark === 0) {
    fpsMark = now;
    return;
  }
  const elapsed = now - fpsMark;
  if (elapsed < 1000) return;
  fps.value = Math.round((fpsFrames * 1000) / elapsed);
  fpsFrames = 0;
  fpsMark = now;
}

/** 每秒把读数拉回 0：停止拖动后不再渲染，但没人去改这个数，
 *  读数会一直停在最后一秒的值，看起来像还在跑。 */
let fpsIdle = 0;
function startFpsClock() {
  fpsIdle = window.setInterval(() => {
    // 这一秒内没有渲染过任何帧，说明画面是静止的
    if (fpsMark !== 0 && performance.now() - fpsMark >= 1000) {
      fps.value = 0;
      fpsFrames = 0;
      fpsMark = 0;
    }
  }, 1000);
}
// 容器尺寸变化用 ResizeObserver 而不是只监听 window：预览舞台会因
// 侧栏拖动、分隔条拖动、页签切换而改变大小，这些都不触发 window resize。
//
// 只重设画布尺寸、不重新取景：相机 FOV 是垂直方向的，而舞台的宽高比固定
// （398:344，由 composables/usePreviewResize 保证），因此画布变大后同一个
// 相机看到的是同一个角范围——模型占画面的**比例**不变而**像素**变多，
// 正好是「舞台变大就画得更大」。重新取景反而会把用户转好的视角掰回去。
let resizeObserver: ResizeObserver | null = null;

/** 请求在下一帧重绘 */
function requestRender() {
  needsRender = true;
}

/** 释放当前网格 */
function disposeScene() {
  built?.dispose();
  built = null;
}

/** 当前参与渲染与交互的相机。切换投影方式只换这一个。 */
function activeCamera(): THREE.PerspectiveCamera | THREE.OrthographicCamera | null {
  return useOrtho ? orthoCamera : perspCamera;
}

/**
 * 把相机拉到能完整看到整个投影的位置，并把**旋转轴心**放到模型中心。
 *
 * 轴心必须一起设：OrbitControls 绕 `target` 旋转，而 target 默认在原点。
 * 若只移动相机不改 target，结构偏离原点时就会「绕着世界原点转」——
 * 表现为拖动时模型绕着画面外的一个点甩动，越大的结构越明显。
 *
 * 两种相机的取景算法不同：透视按 FOV 解距离，正交按投影尺寸定视景体。
 */
function fitCamera() {
  const cam = activeCamera();
  if (!cam || !built) return;
  const box = new THREE.Box3().setFromObject(built.group);
  if (box.isEmpty()) {
    // 空场景（理论上不会走到，几何建好后包围盒就不空）：交给透视那套给个固定位置
    if (cam instanceof THREE.OrthographicCamera) {
      cam.position.set(12, 12, 12);
      cam.lookAt(0, 0, 0);
      cam.updateProjectionMatrix();
    } else {
      fitCameraTo(cam, built.group);
    }
    renderSoon();
    return;
  }
  const center = box.getCenter(new THREE.Vector3());
  if (cam instanceof THREE.OrthographicCamera) {
    frameOrthoBox(cam, box, defaultViewDir(), orthoAspect());
  } else {
    fitCameraTo(cam, built.group);
  }
  controls?.target.copy(center);
  rememberFit();
  controls?.update();
  renderSoon();
}

/**
 * 记下本次自适应的基准，并据此定出滚轮缩放的上下限。
 *
 * 两种相机限的不是同一个量：透视限的是到目标的距离（OrbitControls 每次
 * update 都会 clampLength），正交限的是 camera.zoom（它没有距离概念）。
 * 裁剪面也只有透视需要跟着距离走。
 */
function rememberFit() {
  const cam = activeCamera();
  if (!cam || !controls) return;
  if (cam instanceof THREE.OrthographicCamera) {
    const { min, max } = orthoZoomLimits();
    controls.minZoom = min;
    controls.maxZoom = max;
    controls.enableZoom = true;
    return;
  }
  const dist = cam.position.distanceTo(controls.target);
  const { min, max } = zoomLimits(dist);
  controls.minDistance = min;
  controls.maxDistance = max;
  syncClipPlanes(cam, controls.target);
}

function applyPreset(name: string) {
  const cam = activeCamera();
  if (!cam || !built) return;
  const box = new THREE.Box3().setFromObject(built.group);
  if (box.isEmpty()) return;
  const dir = presetDir(name);
  const center = box.getCenter(new THREE.Vector3());
  if (cam instanceof THREE.OrthographicCamera) {
    const aspect = orthoAspect();
    frameOrthoBox(cam, box, dir, aspect);
  } else {
    frameBox(cam, box, dir);
  }
  // 换视角同样要把轴心留在模型中心，否则换完视角再拖动就会绕错点
  // 顶视/底视会把 up 换成 ±Z，而 controls 的极轴是构造时定死的：
  // 不重建的话，接下来水平拖动会变成抬降镜头
  ensureControlsUp();
  controls?.target.copy(center);
  rememberFit();
  controls?.update();
  renderSoon();
}

/**
 * 方向标点击：把画布坐标换算成方向标内部坐标后做命中检测。
 *
 * 方向标是 SVG，用 viewBox 等比缩放，因此「画布像素 → SVG 用户单位」要按
 * 实际显示尺寸与 viewBox 尺寸的比例换算。命中后按轴向切视角，点中心则取景复位。
 */
function onCanvasPointerDown(e: PointerEvent) {
  // 只记左键，右键留给右键菜单
  if (e.button !== 0) {
    pressAt = null;
    return;
  }
  pressAt = { x: e.clientX, y: e.clientY };
}

function onCanvasPointerUp(e: PointerEvent) {
  const start = pressAt;
  pressAt = null;
  if (!start) return;
  // 拖拽过就不算点击：OrbitControls 已经把画面转过，再切轴线视角会很突兀
  if (Math.hypot(e.clientX - start.x, e.clientY - start.y) > GIZMO_CLICK_SLOP) return;
  const hit = hitGizmoAt(e.clientX, e.clientY);
  if (!hit) return;
  if (hit.kind === "center") {
    fitCamera();
    return;
  }
  emit("pickView", hit.key);
}

/** 把一次屏幕点的位置换算成方向标坐标系里的命中结果。
 *
 * 取方向标元素的包围盒做换算，而不是把六个小球的屏幕位置另算一遍：
 * 方向标由 CSS 固定在左下角，位置由布局决定，DOM 的盒子里就已经包含了
 * 内边距与边框，直接从盒子读最省事也最不会出错。
 */
function hitGizmoAt(clientX: number, clientY: number): GizmoHit | null {
  const el = gizmoEl.value?.$el as HTMLElement | undefined;
  if (!el) return null;
  const rect = el.getBoundingClientRect();
  if (rect.width === 0 || rect.height === 0) return null;
  if (
    clientX < rect.left ||
    clientX > rect.right ||
    clientY < rect.top ||
    clientY > rect.bottom
  ) {
    return null;
  }
  // 显示尺寸 → viewBox 尺寸（viewBox 边长是 GIZMO_VIEW_RADIUS * 2）
  const unit = (GIZMO_VIEW_RADIUS * 2) / rect.width;
  const x = (clientX - rect.left) * unit - GIZMO_VIEW_RADIUS;
  const y = (clientY - rect.top) * unit - GIZMO_VIEW_RADIUS;
  return hitGizmo(gizmoDots(orientation.value, GIZMO_RADIUS), x, y);
}

/** 正交相机的宽高比：它不存 aspect，要由视景体反推。
 *
 * 视景体在 onResize 里按容器宽高与上一个 aspect 重新算，因此这里读的是
 * 当前画布的真实比例。取不到时退回 1。
 */
function orthoAspect(): number {
  const el = host.value;
  if (!el || el.clientHeight === 0) return 1;
  return el.clientWidth / el.clientHeight;
}

/**
 * 立即绘制一帧。
 *
 * 供导出预览图等「必须马上拿到画面」的场景使用；
 * 常规刷新走 requestRender，由渲染循环按需重绘。
 */
function render() {
  const cam = activeCamera();
  if (renderer.value && threeScene && cam) {
    needsRender = false;
    renderer.value.render(threeScene, cam);
  }
}

/** 相机或场景变化后请求重绘 */
function renderSoon() {
  requestRender();
}

function onResize() {
  const el = host.value;
  const cam = activeCamera();
  if (!el || !renderer.value || !cam) return;
  const w = el.clientWidth;
  const h = el.clientHeight;
  if (w === 0 || h === 0) return;
  renderer.value.setSize(w, h, false);
  if (cam instanceof THREE.OrthographicCamera) {
    // 正交相机的成像大小由视景体与 zoom 决定，宽高比变化时必须重算边界，
    // 否则窗口一变宽画面就会被横向拉伸
    adaptOrthoAspect(cam, w / h);
  } else {
    cam.aspect = w / h;
    cam.updateProjectionMatrix();
  }
  renderSoon();
}

/** 重建网格并刷新画面 */
function rebuild(data: SceneData | null) {
  disposeScene();
  if (!data) {
    // 清空后也要重绘一次，否则画布上会残留上一个投影的画面
    renderSoon();
    return;
  }
  built = buildSceneGroup(data);
  if (threeScene) threeScene.add(built.group);
  // 切换投影后必须显式请求重绘。
  //
  // 这是「切换投影后预览不更新，要点一下画面才变」的成因：按需渲染只在
  // needsRender 或阻尼未停时才画，而重建网格既不置 needsRender、也不改变
  // 相机（fitCamera 只调 controls.update()，没有输入时阻尼不产生位移），
  // 于是画布一直停在上一帧，直到用户点一下画面触发交互才刷新。
  renderSoon();
}

onMounted(() => {
  const el = host.value;
  if (!el) return;
  try {
    const r = new THREE.WebGLRenderer({
      antialias: true,
      alpha: true,
      // 导出预览图要在 render() 之后异步读取画布，
      // 不保留绘制缓冲会读到空白图
      preserveDrawingBuffer: true,
    });
    r.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    r.setClearColor(0x000000, 0);
    el.appendChild(r.domElement);
    renderer.value = r;

    const scene = new THREE.Scene();
    threeScene = scene;

    // 不加光源：材质是 MeshBasicMaterial，本就不参与光照计算，
    // 立体感完全来自逐顶点的明暗系数（写入 color 属性）。

    perspCamera = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
    // 正交相机先给一个占位视景体，真正的尺寸在 fitCamera 里按模型算
    orthoCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10000);
    controls = makeControls(perspCamera, r.domElement);

    // 方向标的点击：事件必须由画布接，方向标自己不能收——否则在它上面
    // 按住拖动会被它吃掉，画布收不到 pointerdown，拖不动视角。
    // pointerup 时用移动距离区分「点击」与「拖拽后的松开」。
    const dom = r.domElement;
    dom.addEventListener("pointerdown", onCanvasPointerDown);
    dom.addEventListener("pointerup", onCanvasPointerUp);

    const loop = () => {
      raf = requestAnimationFrame(loop);
      // `update()` 必须每帧调：阻尼动画靠它逐帧收敛。
      // 但它返回的「有没有变化」不能当作是否重绘的依据——滚轮缩放时
      // OrbitControls 内部已经调过一次并丢掉了结果，这里再调就是 false。
      // 重绘统一由 controls 的 change 事件与 requestRender() 驱动。
      controls?.update();
      const cam = activeCamera();
      // 距离变了就要跟着改裁剪面，否则一直拉近会穿进近裁剪面、模型被切没。
      // 这项只对透视相机有意义：正交的裁剪面不随视角变化
      if (cam instanceof THREE.PerspectiveCamera && controls) {
        if (syncClipPlanes(cam, controls.target)) needsRender = true;
      }
      if (needsRender) {
        needsRender = false;
        if (renderer.value && cam) {
          renderer.value.render(scene, cam);
          countFrame(performance.now());
        }
      }
    };
    loop();
    startFpsClock();

    window.addEventListener("resize", onResize);
    // 窗口 resize 事件只在窗口尺寸变化时触发，而预览舞台现在会随
    // **侧栏宽度、分隔条拖动、页签切换**等多种原因改变大小。
    // 只用 window 事件会让画布停留在旧尺寸（表现为图像被拉伸或裁掉）。
    if (typeof ResizeObserver !== "undefined") {
      resizeObserver = new ResizeObserver(() => onResize());
      resizeObserver.observe(el);
    }
    onResize();
    if (props.scene) {
      rebuild(props.scene);
      fitCamera();
    }
  } catch (e) {
    // WebGL 不可用时给出明确提示，而不是留下一块空白
    error.value = t("scene.initFailed", { error: String(e) });
  }
});

onBeforeUnmount(() => {
  cancelAnimationFrame(raf);
  window.clearInterval(fpsIdle);
  window.removeEventListener("resize", onResize);
  resizeObserver?.disconnect();
  resizeObserver = null;
  const dom = renderer.value?.domElement;
  if (dom) {
    dom.removeEventListener("pointerdown", onCanvasPointerDown);
    dom.removeEventListener("pointerup", onCanvasPointerUp);
  }
  disposeScene();
  controls?.dispose();
  renderer.value?.dispose();
  dom?.remove();
  renderer.value = null;
  threeScene = null;
  perspCamera = null;
  orthoCamera = null;
  controls = null;
});

watch(
  () => props.scene,
  (data) => {
    rebuild(data);
    if (data) fitCamera();
    // 亮度：新建材质时要套用当前值，否则切换投影后画面回到 100%
    // 而滑杆仍显示原值，两者不一致
    applyBrightness(props.brightness);
    renderSoon();
  },
);

/** 把亮度套到现有材质上 */
function applyBrightness(b: number) {
  built?.group.traverse((o) => {
    const m = (o as THREE.Mesh).material as THREE.MeshBasicMaterial | undefined;
    if (m && "color" in m && m.vertexColors) {
      // 顶点颜色已承载面明暗，亮度通过材质颜色整体缩放
      m.color.setScalar(b);
      m.needsUpdate = true;
    }
  });
}

watch(
  () => props.brightness,
  (b) => {
    applyBrightness(b);
    render();
  },
);

/**
 * 按指定像素宽高导出当前画面为透明背景 PNG。
 *
 * 临时把渲染器调到目标尺寸、按目标宽高比调整相机，渲染后立即恢复，
 * 因此用户不会看到界面尺寸被改动。
 *
 * 两种相机调宽高比的方式不同：透视改 aspect，正交要按比例重算视景体
 * （它没有 aspect 字段）。导出前后都要还原，否则界面里的构图会变。
 */
async function exportPng(w: number, h: number): Promise<Uint8Array> {
  const r = renderer.value;
  const cam = activeCamera();
  if (!r || !cam || !threeScene) {
    throw new Error(t("scene.notReady"));
  }
  const prev = new THREE.Vector2();
  r.getSize(prev);
  const prevRatio = r.getPixelRatio();
  const isOrtho = cam instanceof THREE.OrthographicCamera;
  const prevAspect = isOrtho ? 0 : cam.aspect;
  const prevFrustum = isOrtho
    ? { l: cam.left, r: cam.right, t: cam.top, b: cam.bottom, z: cam.zoom }
    : null;

  try {
    r.setPixelRatio(1);
    r.setSize(w, h, false);
    if (isOrtho) {
      adaptOrthoAspect(cam, w / h);
    } else {
      cam.aspect = w / h;
      cam.updateProjectionMatrix();
    }
    r.render(threeScene, cam);

    const blob: Blob = await new Promise((resolve, reject) => {
      r.domElement.toBlob(
        (b) => (b ? resolve(b) : reject(new Error(t("scene.exportFailed")))),
        "image/png",
      );
    });
    return new Uint8Array(await blob.arrayBuffer());
  } finally {
    r.setPixelRatio(prevRatio);
    r.setSize(prev.x, prev.y, false);
    if (isOrtho && prevFrustum) {
      cam.left = prevFrustum.l;
      cam.right = prevFrustum.r;
      cam.top = prevFrustum.t;
      cam.bottom = prevFrustum.b;
      cam.zoom = prevFrustum.z;
      cam.updateProjectionMatrix();
    } else if (!isOrtho) {
      cam.aspect = prevAspect;
      cam.updateProjectionMatrix();
    }
    render();
  }
}

/** 创建并配置一台 OrbitControls。切换相机时必须重建：
 *  它在构造时绑死相机实例，没有换相机的接口。
 *
 * 重绘不能只靠渲染循环里 `update()` 的返回值：滚轮缩放走的是
 * `_handleMouseWheel`，它内部自己调一次 `update()` 并把结果丢掉；
 * 等循环再调一次时状态已经稳定，`zoomChanged` 为 false，于是正交投影下
 * 「缩放要等下一次拖动才生效」。改为订阅 `change` 事件请求重绘——
 * 只要 controls 改动了相机就一定会触发它。
 */
function makeControls(cam: THREE.Camera, dom: HTMLElement): OrbitControls {
  const c = new OrbitControls(cam, dom);
  c.enableDamping = true;
  c.dampingFactor = 0.15;
  c.screenSpacePanning = true;
  c.enableZoom = true;
  c.zoomSpeed = 0.9;
  c.addEventListener("change", onControlsChange);
  return c;
}

/** controls 改动了相机：标记重绘，并同步方向标 */
function onControlsChange() {
  requestRender();
  syncOrientation();
}

/** 记下 controls 构造时的相机 up 与极轴。
 *
 * OrbitControls 在构造时把 `object.up` 换算成内部的一套旋转基准（`_quat`），
 * 之后 `camera.up` 再改它也不会跟着变。顶视/底视会把 up 换成 ±Z，
 * 若不重建 controls，水平拖动就变成抬降镜头（本该只绕竖轴转）。
 */
let controlsUp = new THREE.Vector3();

/** 相机换过 up（顶视↔斜视）后重建 controls，让极轴对上新的 up。 */
function ensureControlsUp(dom?: HTMLElement) {
  const cam = activeCamera();
  const el = dom ?? renderer.value?.domElement;
  if (!cam || !el) return;
  if (controls && cam.up.distanceTo(controlsUp) < 1e-6) return;
  const target = controls?.target.clone();
  controls?.dispose();
  controls = makeControls(cam, el);
  if (target) controls.target.copy(target);
  controlsUp.copy(cam.up);
}

/** 切换正射投影与透视投影。
 *
 * 两个相机共享位置、朝向与「看到的范围」：切换时先量出当前可见范围，
 * 换到新相机后按同一个范围摆好，用户看到的是「同一构图换了投影」。
 * 透视的可见范围由距离与 FOV 决定，正交的由视景体除以 zoom 决定，
 * 两者都是世界坐标长度，可以互相换算。
 *
 * OrbitControls 绑在具体相机实例上，没有换相机的接口，必须重建。
 */
function setOrtho(ortho: boolean) {
  if (useOrtho === ortho) return;
  const from = activeCamera();
  // 没有 controls 时（初始化中途）无从换算可见范围，退回按模型重新取景
  const prevTarget = controls?.target.clone() ?? null;

  let prevHalfH = 0;
  if (from && prevTarget) {
    if (from instanceof THREE.PerspectiveCamera) {
      prevHalfH = perspectiveViewHalfHeight(
        from.position.distanceTo(prevTarget),
        from.fov,
      );
    } else if (from instanceof THREE.OrthographicCamera) {
      prevHalfH = orthoVisibleExtent(from).halfH;
    }
  }

  useOrtho = ortho;
  const to = activeCamera();
  if (!to) return;

  if (from) {
    to.position.copy(from.position);
    to.quaternion.copy(from.quaternion);
    to.up.copy(from.up);
  }

  const dom = renderer.value?.domElement;
  if (dom) {
    controls?.dispose();
    controls = makeControls(to, dom);
    controlsUp.copy(to.up);
    if (prevTarget) controls.target.copy(prevTarget);
  }

  if (prevHalfH > 0 && prevTarget) {
    const aspect = orthoAspect();
    if (to instanceof THREE.OrthographicCamera) {
      // 正交下没有距离概念，位置只影响裁剪：沿用透视那个位置即可
      // （它离模型足够远），裁剪面按模型尺寸给足余量。
      fitOrthoClipPlanes(to, prevTarget);
      to.zoom = 1;
      setOrthoViewExtent(to, prevHalfH, aspect);
    } else if (to instanceof THREE.PerspectiveCamera) {
      // 透视保持自己的 FOV 不变，改用距离去凑同一个可见范围。
      // 换 FOV 会连透视的远近感一起变掉，那是另一个视角而不是换投影。
      const halfH = Math.max(prevHalfH, 1e-6);
      const dist =
        halfH / Math.tan((to.fov * Math.PI) / 360);
      const dir = new THREE.Vector3().subVectors(to.position, prevTarget);
      if (dir.lengthSq() > 1e-6) {
        to.position.copy(prevTarget).addScaledVector(dir.normalize(), dist);
      }
      to.aspect = aspect;
      to.updateProjectionMatrix();
    }
  } else {
    // 没有可换算的可见范围（初始化中途）：按新相机自己的方式重新取景
    fitCamera();
  }

  rememberFit();
  // 重建 controls 会新造一个 target，先把轴心补回去再 update
  if (prevTarget) controls?.target.copy(prevTarget);
  controls?.update();
  syncOrientation();
  renderSoon();
}

/** 给正交相机摆一对足够覆盖模型的裁剪面。
 *
 * 正交相机的位置不影响成像大小，但 near/far 仍会裁掉相机前后的东西：
 * 沿用透视那台相机算出来的值会在模型很大时把画面切掉。
 */
function fitOrthoClipPlanes(cam: THREE.OrthographicCamera, target: THREE.Vector3) {
  if (!built) return;
  const box = new THREE.Box3().setFromObject(built.group);
  if (box.isEmpty()) return;
  const diag = Math.max(box.getSize(new THREE.Vector3()).length(), 1e-3);
  const dist = cam.position.distanceTo(target);
  cam.near = Math.max(dist - diag * 2, 0.01);
  cam.far = dist + diag * 2;
  cam.updateProjectionMatrix();
}

/** 当前相机的位姿，供「保存为预设」记录。
 *
 * 直接给位置与目标点而不是只给角度：位置决定了透视下的观感（距离远近），
 * 只存角度会让存下来的预设一用就变成「同一个方向、不同的远近」。
 */
function cameraPose(): {
  position: [number, number, number];
  target: [number, number, number];
} | null {
  const cam = activeCamera();
  if (!cam || !controls) return null;
  return {
    position: [cam.position.x, cam.position.y, cam.position.z],
    target: [controls.target.x, controls.target.y, controls.target.z],
  };
}

/** 应用一条自定义预设：按存下来的位置与目标点还原视角。
 *
 * 与内置预设不同，这里不走 fitDistance 重新取景——用户存的是「他看到的
 * 那个画面」，按模型重新算距离会让缩放过的视角被掰回自适应位置。
 */
function applySavedView(preset: SavedViewPreset) {
  setOrtho(preset.ortho);
  const cam = activeCamera();
  if (!cam || !controls) return;
  cam.position.set(...preset.position);
  controls.target.set(...preset.target);
  // up 必须按新朝向校正：上一台相机可能停在顶视/底视，把 up 留成 (0,0,∓1)，
  // 此时 lookAt 会绕视线把画面滚一个角度——模型是斜的，用户只会觉得「歪了」
  cam.up.copy(cameraUpFor(presetDirFrom(preset)));
  cam.lookAt(controls.target);
  if (cam instanceof THREE.OrthographicCamera && built) {
    // 正交相机的位置不影响成像大小，但视景体要按当前朝向重算，
    // 否则横向被裁——它不能沿用别的朝向算出来的左右边界
    const box = new THREE.Box3().setFromObject(built.group);
    if (!box.isEmpty()) {
      const dir = new THREE.Vector3().subVectors(controls.target, cam.position);
      if (dir.lengthSq() > 0) {
        frameOrthoBox(cam, box, dir.normalize(), orthoAspect());
        // 位置与 up 都被 frameOrthoBox 改写了，按存档再摆回来
        cam.position.set(...preset.position);
        cam.up.copy(cameraUpFor(presetDirFrom(preset)));
        cam.lookAt(controls.target);
      }
    }
  }
  // up 变了 polar 基准也要跟着换，否则之后水平拖动会抬降镜头
  ensureControlsUp();
  controls.target.set(...preset.target);
  rememberFit();
  controls.update();
  syncOrientation();
  renderSoon();
}

/** 一条自定义预设的「相机 → 目标」方向。
 *
 * 优先用存下来的位置与目标点算，两者重合（数据异常）时退回用 yaw/pitch 推，
 * 保证任何情况下都能给 up 一个可用的视线方向。
 */
function presetDirFrom(preset: SavedViewPreset): THREE.Vector3 {
  const dir = new THREE.Vector3(
    preset.target[0] - preset.position[0],
    preset.target[1] - preset.position[1],
    preset.target[2] - preset.position[2],
  );
  if (dir.lengthSq() > 1e-12) return dir.normalize();
  return dirFromAngles(preset.yaw, preset.pitch).multiplyScalar(-1);
}

/** 按预设键切换视角：投影方式由预设自己决定（真等距是正射，其余是透视）。
 *
 * 方向标与右键菜单都走这里，确保「同一个预设」在两条入口下行为一致。
 */
function applyViewPreset(name: string) {
  setOrtho(presetIsOrtho(name));
  applyPreset(name);
  syncOrientation();
}

defineExpose({
  applyPreset,
  applyViewPreset,
  exportPng,
  fitCamera,
  setOrtho,
  cameraPose,
  applySavedView,
});
</script>

<template>
  <div class="scene-viewer">
    <div ref="host" class="scene-viewer__canvas" />
    <!-- 帧率：按需渲染下静止时是 0，这一读数用来判断画面是否还在重绘
         （拖动视角时会跳动，停手后归零）。只在有场景且 WebGL 正常时显示。 -->
    <div
      v-if="scene && !error"
      class="scene-viewer__fps"
      :class="{ 'is-idle': fps === 0 }"
      :title="fps === 0 ? t('scene.fpsIdle') : t('scene.fps', { value: fps })"
    >
      {{ fps === 0 ? t("scene.fpsIdle") : t("scene.fps", { value: fps }) }}
    </div>
    <!-- 三角形数：画面右下角，与右上的帧率读数对称。
         它随场景变化而不随视角变化，因此只有一处读数、跟着 scene 走。 -->
    <div
      v-if="scene && !error"
      class="scene-viewer__tris"
      :title="t('scene.triangles', { count: scene.totalTriangles.toLocaleString() })"
    >
      {{ t("scene.triangles", { count: scene.totalTriangles.toLocaleString() }) }}
    </div>
    <div v-if="error" class="scene-viewer__error">{{ error }}</div>
    <div v-else-if="!scene" class="scene-viewer__empty">{{ t("scene.empty") }}</div>
    <!-- XYZ 方向标：显示当前朝向的三个坐标轴。它不接收指针事件——
         命中检测由画布统一做，这样在它上面按住拖动照样能转视角。 -->
    <ViewGizmo
      v-if="scene && !error"
      ref="gizmoEl"
      class="scene-viewer__gizmo"
      :orientation="orientation"
    />
  </div>
</template>

<style scoped lang="scss">
.scene-viewer {
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 200px;
  // 裁掉任何越界内容，作为「渲染图形不得超出范围」的最后一道防线
  overflow: hidden;

  &__canvas {
    // 画布容器：占满父容器（预览舞台），实际宽高由 .preview__stage 决定
    width: 100%;
    height: 100%;
    overflow: hidden;

    // three.js 的 setSize(w, h, false) 只设置 canvas 的 width/height **属性**，
    // 不改 CSS 尺寸。若不在这里约束 canvas 元素本身，它会按属性尺寸显示：
    // 实测容器 862px 而画布 987px，横向溢出 125px，画面被撑出面板之外。
    // 用 100% 让显示尺寸恒等于容器，位图分辨率另由属性决定（设备像素比放大）。
    :deep(canvas) {
      display: block;
      width: 100%;
      height: 100%;
    }
  }

  &__error,
  &__empty {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--color-text-3);
    font-size: 13px;
    pointer-events: none;
  }

  // 帧率读数：画面右上角，半透明底避免压住模型细节。
  // 静止时（0 帧）压暗，一眼能区分「在重绘」与「停着」。
  &__fps {
    position: absolute;
    top: 6px;
    right: 6px;
    padding: 1px 6px;
    border-radius: var(--border-radius-small);
    background: var(--color-bg-popup);
    color: var(--color-text-2);
    font-family: Consolas, "Courier New", monospace;
    font-size: 11px;
    line-height: 16px;
    opacity: 0.8;
    pointer-events: none;

    &.is-idle {
      opacity: 0.45;
    }
  }

  &__error {
    color: rgb(var(--danger-6));
  }

  // 三角形数读数：与右上角的帧率读数同一套样式，放在右下角对称位置。
  // 数字会到百万级，等宽字体让位数变化时宽度不跳。
  &__tris {
    position: absolute;
    right: 6px;
    bottom: 6px;
    padding: 1px 6px;
    border-radius: var(--border-radius-small);
    background: var(--color-bg-popup);
    color: var(--color-text-2);
    font-family: Consolas, "Courier New", monospace;
    font-size: 11px;
    line-height: 16px;
    opacity: 0.7;
    pointer-events: none;
  }

  // 方向标：画面左下角。放右下角会与读数打架，
  // 且左下角在多数投影图里是空白区（模型居中）。
  &__gizmo {
    position: absolute;
    left: 6px;
    bottom: 6px;
  }
}
</style>
