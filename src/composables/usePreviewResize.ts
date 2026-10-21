/**
 * 预览区尺寸的拖动调整。
 *
 * 预览行（3D 舞台 + 投影信息列）外面有两条分隔条，分别控制两个方向：
 *   - 预览行与下方页签区之间的水平分隔条：往上拖让预览占更多高度；
 *   - 舞台与信息列之间的垂直分隔条：拖动改变舞台宽度，信息列吃掉剩余宽度。
 *
 * 宽与高是两个独立的值，互不推导。早期实现让舞台按 398:344 等比缩放，
 * 于是拖高度时宽度跟着变——用户拖的是高度，宽度却在动，这不可接受。
 *
 * 两者都只在用户拖动时改变，不随窗口缩放自动变化：窗口放大时多出来的宽度
 * 由信息列占用、多出来的高度留在页签区，「界面不自行变化尺寸」因此仍成立。
 */
import { computed, ref } from "vue";
import { usePanelResize } from "./usePanelResize";

/** 舞台的默认宽度，取自用户截图里标注的红框（398×344）。 */
export const PREVIEW_DEFAULT_W = 398;

/** 舞台宽度的下限：再窄就放不下预览画面，也点不准里面的内容。 */
export const PREVIEW_MIN_W = 160;

/** 舞台宽度的上限。信息列至少要留出 300px（否则「信息在预览右侧」名存实亡），
 *  因此上限由窗口宽度减去这些占位算出，而不是写死一个数。 */
export const PREVIEW_MAX_W = 1600;

/** 预览面板在默认尺寸下的高度。
 *
 * 与原先写死的值一致：预览行 360（舞台 344 + 上下内边距 16）+ 控制条 40
 * + 分隔线 1。
 */
export const PREVIEW_DEFAULT_H = 401;

/** 面板高度的下限：再矮就没有预览价值，且控制条会压到舞台上。 */
export const PREVIEW_MIN_H = 200;

/** 面板高度的上限。窗口再高也不该让预览区独占整个详情区：
 *  至少给页签区留出 160px（统计条加一行表格）。 */
export const PREVIEW_MAX_H = 2000;

export function usePreviewResize() {
  /** 预览行高度：水平分隔条拖动它 */
  const height = usePanelResize({
    axis: "y",
    defaultSize: PREVIEW_DEFAULT_H,
    min: PREVIEW_MIN_H,
    max: PREVIEW_MAX_H,
    // 预览行在分隔条**上方**，尺寸随指针同向变化：指针下移（clientY 变大）
    // 预览行变高，分隔条因此始终跟着鼠标走。
    // 详情区高度 = 窗口高 − 标题栏与工具栏占位，页签区还要留 160
    maxByWindow: () => window.innerHeight - 199 - 160,
  });

  /** 舞台宽度：垂直分隔条拖动它 */
  const width = usePanelResize({
    axis: "x",
    defaultSize: PREVIEW_DEFAULT_W,
    min: PREVIEW_MIN_W,
    max: PREVIEW_MAX_W,
    // 舞台左侧没有别的分区，指针右移即变宽，无需反向
    // 详情区宽度 = 窗口宽 − 目录栏与分隔条占位；信息列至少 300
    maxByWindow: () => window.innerWidth - 671 - 300,
  });

  /** 预览行**高度**的实测值，由组件用 ResizeObserver 写入。
   *
   * 不能用「面板高度 − 固定常数」推算舞台高度：控制条是 flex-wrap 的，
   * 预览区窄到放不下那一排控件时会折成两行，实际占用的高度从 40 变成 80，
   * 按常数算出来的舞台就会超出可用空间、被 overflow 裁掉。
   *
   * 只取高度不取宽度：宽度已由垂直分隔条独立控制，不需要再反算。
   */
  const rowHeight = ref<number | null>(null);

  function measureRowHeight(h: number) {
    rowHeight.value = h > 0 ? h : null;
  }

  /** 舞台实际高度：填满预览行高度。
   *
   * 面板被拖高时舞台跟着变高、不留空白；拿不到实测值时（首帧、测试环境）
   * 按面板高度推算，扣掉上下内边距 16、控制条 40、分隔线 1。
   */
  const stageHeight = computed(() => {
    const avail = rowHeight.value
      ? rowHeight.value - 16
      : Math.max(height.size.value - 57, 40);
    return Math.round(Math.max(avail, 40));
  });

  return {
    previewHeight: height.size,
    previewWidth: width.size,
    previewStageHeight: stageHeight,
    previewRowHeight: rowHeight,
    measurePreviewRowHeight: measureRowHeight,
    previewHorizResizing: height.resizing,
    previewVertResizing: width.resizing,
    clampPreviewHeight: height.clamp,
    clampPreviewWidth: width.clamp,
    startPreviewHeightResize: height.startResize,
    startPreviewWidthResize: width.startResize,
    togglePreviewHeight: height.toggleSize,
    togglePreviewWidth: width.toggleSize,
    onPreviewHeightKey: height.onSplitterKey,
    onPreviewWidthKey: width.onSplitterKey,
  };
}
