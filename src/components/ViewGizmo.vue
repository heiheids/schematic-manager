/**
 * XYZ 三向方向标：显示当前视角下三个坐标轴各指向哪个方向。
 *
 * 它是「当前朝向的读数」而不是独立的 3D 场景：用 SVG 把六个轴向按相机朝向
 * 投到屏幕上即可，不必再开一个 WebGL 上下文——那要多一份渲染循环，且与主画面
 * 的朝向同步容易不同步。
 *
 * 这块**不接收指针事件**。方向标压在画布左上角，若它自己收下 pointerdown，
 * 在它上面按住拖动就转不动画面了（画布根本收不到事件）。因此点击由画布
 * 统一处理：pointerdown / click 都落在画布上，画布再把坐标换算进方向标坐标系
 * 做命中检测（见 hitGizmo）。这样「拖拽转视角」与「点小球切视角」两件事
 * 互不干扰。
 */
<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import * as THREE from "three";
import { GIZMO_CENTER_HALF, GIZMO_RADIUS, GIZMO_VIEW_RADIUS, gizmoDots } from "../render/view_gizmo";

const { t } = useI18n();

const props = defineProps<{
  /** 相机当前的朝向；null 表示还没有相机，方向标不显示轴向 */
  orientation: THREE.Quaternion | null;
}>();

/** 六个端点小球的位置与深度，按远近排好序（远的先画） */
const dots = computed(() => gizmoDots(props.orientation, GIZMO_RADIUS));

/** 三个轴的颜色。X/Y/Z 用红绿蓝是工程制图与 3D 软件的通行约定，
 *  不宜改用主题色——那会让「哪根是哪个轴」失去通用性。
 *  深色主题下提亮一档，避免在深底上发闷。 */
function axisColor(axis: string): string {
  if (axis.endsWith("x")) return "var(--gizmo-x)";
  if (axis.endsWith("y")) return "var(--gizmo-y)";
  return "var(--gizmo-z)";
}

/** 无障碍标签：轴向用「东（X+）」这类说法，与右键菜单的预设名一致 */
function ariaOf(axis: string): string {
  const map: Record<string, string> = {
    "+x": "view.east",
    "-x": "view.west",
    "+y": "view.top",
    "-y": "view.bottom",
    "+z": "view.south",
    "-z": "view.north",
  };
  return t(map[axis] ?? "view.top");
}
</script>

<template>
  <svg
    class="view-gizmo"
    :viewBox="`${-GIZMO_VIEW_RADIUS} ${-GIZMO_VIEW_RADIUS} ${GIZMO_VIEW_RADIUS * 2} ${GIZMO_VIEW_RADIUS * 2}`"
    :width="GIZMO_VIEW_RADIUS * 2 + 12"
    :height="GIZMO_VIEW_RADIUS * 2 + 12"
    role="group"
    :aria-label="t('view.gizmoLabel')"
  >
    <!-- 三根轴线：从中心连到对应端点。先画线再画球，球才盖在线上 -->
    <g>
      <line
        v-for="d in dots"
        :key="`line-${d.axis}`"
        :x1="0"
        :y1="0"
        :x2="d.x"
        :y2="d.y"
        :stroke="axisColor(d.axis)"
        :stroke-width="d.depth < 0 ? 1.2 : 2"
        :opacity="d.depth < 0 ? 0.45 : 0.95"
        stroke-linecap="round"
      />
    </g>
    <!-- 端点小球：点一下切到该轴正视角（命中由画布判定） -->
    <g>
      <circle
        v-for="d in dots"
        :key="d.axis"
        :cx="d.x"
        :cy="d.y"
        r="9"
        class="view-gizmo__dot"
        :fill="axisColor(d.axis)"
        :opacity="d.depth < 0 ? 0.55 : 1"
      >
        <title>{{ ariaOf(d.axis) }}</title>
      </circle>
      <!-- 轴向文字：同样随相机转动，随小球位置排布 -->
      <text
        v-for="d in dots"
        :key="`t-${d.axis}`"
        :x="d.x"
        :y="d.y + 3.2"
        class="view-gizmo__label"
        text-anchor="middle"
        :opacity="d.depth < 0 ? 0.6 : 1"
      >
        {{ d.axis }}
      </text>
    </g>
    <!-- 中心复位：回到默认等距视角。命中区与它一致，见 GIZMO_CENTER_HALF -->
    <rect
      :x="-GIZMO_CENTER_HALF"
      :y="-GIZMO_CENTER_HALF"
      :width="GIZMO_CENTER_HALF * 2"
      :height="GIZMO_CENTER_HALF * 2"
      rx="2"
      class="view-gizmo__center"
    >
      <title>{{ t("view.reset") }}</title>
    </rect>
  </svg>
</template>
