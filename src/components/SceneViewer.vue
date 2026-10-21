<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { t } from "../i18n";
import type { SceneData } from "../types";
import {
  applyPreset as applyPresetTo,
  buildSceneGroup,
  fitCamera as fitCameraTo,
  syncClipPlanes,
  zoomLimits,
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

const host = ref<HTMLDivElement | null>(null);
const error = ref("");

// three 的对象不需要响应式代理（会被深度追踪，既慢又可能出错）
const renderer = shallowRef<THREE.WebGLRenderer | null>(null);
let built: SceneGroup | null = null;
let threeScene: THREE.Scene | null = null;
let camera: THREE.PerspectiveCamera | null = null;
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

/** 模型包围盒的中心；没有模型时返回 null */
function modelCenter(): THREE.Vector3 | null {
  if (!built) return null;
  const box = new THREE.Box3().setFromObject(built.group);
  if (box.isEmpty()) return null;
  return box.getCenter(new THREE.Vector3());
}

/**
 * 把相机拉到能完整看到整个投影的位置，并把**旋转轴心**放到模型中心。
 *
 * 轴心必须一起设：OrbitControls 绕 `target` 旋转，而 target 默认在原点。
 * 若只移动相机不改 target，结构偏离原点时就会「绕着世界原点转」——
 * 表现为拖动时模型绕着画面外的一个点甩动，越大的结构越明显。
 */
function fitCamera() {
  if (!camera || !built) return;
  fitCameraTo(camera, built.group);
  const c = modelCenter();
  if (c) controls?.target.copy(c);
  rememberFit();
  controls?.update();
  renderSoon();
}

/**
 * 记下本次自适应的距离，并据此定出滚轮缩放的上下限。
 *
 * 上下限交给 OrbitControls 自己夹（它每次 update 都会 clampLength），
 * 因此不需要另写一套拨轮逻辑——实测它自带的滚轮缩放本身是好用的，
 * 真正缺的是「距离有界」与「裁剪面跟着距离走」这两件事。
 */
function rememberFit() {
  if (!camera || !controls) return;
  const dist = camera.position.distanceTo(controls.target);
  const { min, max } = zoomLimits(dist);
  controls.minDistance = min;
  controls.maxDistance = max;
  syncClipPlanes(camera, controls.target);
}

function applyPreset(name: string) {
  if (!camera || !built) return;
  applyPresetTo(camera, built.group, name);
  // 换视角同样要把轴心留在模型中心，否则换完视角再拖动就会绕错点
  const c = modelCenter();
  if (c) controls?.target.copy(c);
  rememberFit();
  controls?.update();
  renderSoon();
}

/**
 * 立即绘制一帧。
 *
 * 供导出预览图等「必须马上拿到画面」的场景使用；
 * 常规刷新走 requestRender，由渲染循环按需重绘。
 */
function render() {
  if (renderer.value && threeScene && camera) {
    needsRender = false;
    renderer.value.render(threeScene, camera);
  }
}

/** 相机或场景变化后请求重绘 */
function renderSoon() {
  requestRender();
}

function onResize() {
  const el = host.value;
  if (!el || !renderer.value || !camera) return;
  const w = el.clientWidth;
  const h = el.clientHeight;
  if (w === 0 || h === 0) return;
  renderer.value.setSize(w, h, false);
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
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

    camera = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
    controls = new OrbitControls(camera, r.domElement);
    controls.enableDamping = true;
    controls.dampingFactor = 0.15;
    controls.screenSpacePanning = true;
    // 滚轮缩放由 OrbitControls 提供（默认就开）。它每次 update 都会把距离
    // 夹在 min/maxDistance 内，那两个值在 rememberFit 里按模型大小设定。
    controls.enableZoom = true;
    controls.zoomSpeed = 0.9;

    const loop = () => {
      raf = requestAnimationFrame(loop);
      let changed = controls?.update() ?? false;
      // 距离变了就要跟着改裁剪面，否则一直拉近会穿进近裁剪面、模型被切没
      if (camera && controls && syncClipPlanes(camera, controls.target)) changed = true;
      // 阻尼未停时必须每帧重绘，否则拖动动画会卡住
      if (needsRender || changed) {
        needsRender = false;
        if (renderer.value && camera) {
          renderer.value.render(scene, camera);
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
  disposeScene();
  controls?.dispose();
  renderer.value?.dispose();
  renderer.value?.domElement.remove();
  renderer.value = null;
  threeScene = null;
  camera = null;
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
 * 按指定边长导出当前画面为透明背景 PNG。
 *
 * 临时把渲染器调到目标尺寸、宽高比设为 1，渲染后立即恢复，
 * 因此用户不会看到界面尺寸被改动。
 */
async function exportPng(size: number): Promise<Uint8Array> {
  const r = renderer.value;
  if (!r || !camera || !threeScene) {
    throw new Error(t("scene.notReady"));
  }
  const prev = new THREE.Vector2();
  r.getSize(prev);
  const prevRatio = r.getPixelRatio();
  const prevAspect = camera.aspect;

  try {
    r.setPixelRatio(1);
    r.setSize(size, size, false);
    camera.aspect = 1;
    camera.updateProjectionMatrix();
    r.render(threeScene, camera);

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
    camera.aspect = prevAspect;
    camera.updateProjectionMatrix();
    render();
  }
}

defineExpose({
  applyPreset,
  exportPng,
  fitCamera,
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
    <div v-if="error" class="scene-viewer__error">{{ error }}</div>
    <div v-else-if="!scene" class="scene-viewer__empty">{{ t("scene.empty") }}</div>
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
}
</style>
