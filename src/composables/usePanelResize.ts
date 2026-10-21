/**
 * 面板尺寸的拖动调整。
 *
 * 起初只有「左侧目录栏的宽度」这一处，逻辑直接写在 useSidebarResize 里；
 * 后来详情面板也需要调整预览区高度，两者除了方向不同以外完全一样
 * （夹合法区间、拖动期间挂全局指针监听、双击复位、键盘可操作、卸载清理），
 * 因此收口到这里，由调用方给出方向与区间。
 *
 * 使用方只需要拿到尺寸值与分隔条事件处理器；尺寸本身要不要记忆由
 * 使用方决定（目录栏宽度与预览区高度都记在 store/uiState 里）。
 */
import { onBeforeUnmount, onMounted, ref } from "vue";

/** 一个方向上的可拖动分隔条。 */
export interface PanelResizeOptions {
  /** 拖动方向：横向拖动改宽度，纵向拖动改高度 */
  axis: "x" | "y";
  /** 初始尺寸（像素） */
  defaultSize: number;
  /** 允许的最小尺寸 */
  min: number;
  /** 允许的最大尺寸 */
  max: number;
  /** 尺寸上限还要受容器大小限制时的计算函数。
   *
   * 拖动到窗口边缘时，只按固定 max 夹会允许面板压住别的分区，
   * 因此把「按当前窗口算出的上限」交给使用方给。
   */
  maxByWindow?: () => number;
}

export function usePanelResize(opts: PanelResizeOptions) {
  const size = ref(opts.defaultSize);
  /** 是否正在拖动。拖动期间禁用过渡并屏蔽文本选择 */
  const resizing = ref(false);

  function clamp(value: number): number {
    const upper = Math.min(opts.max, opts.maxByWindow ? opts.maxByWindow() : opts.max);
    const lower = Math.min(opts.min, upper);
    return Math.min(Math.max(Math.round(value), lower), upper);
  }

  /** 从指针位置算出尺寸：指针与尺寸同向变化，
   *  分隔条因此始终跟着鼠标走，不会出现「分隔条往一边、鼠标往另一边」。 */
  function sizeFromPointer(startPointer: number, startSize: number, current: number): number {
    return clamp(startSize + (current - startPointer));
  }

  let startPointer = 0;
  let startSize = 0;

  function onPointerMove(e: PointerEvent) {
    if (!resizing.value) return;
    const current = opts.axis === "x" ? e.clientX : e.clientY;
    size.value = sizeFromPointer(startPointer, startSize, current);
  }

  function stopResize() {
    if (!resizing.value) return;
    resizing.value = false;
    window.removeEventListener("pointermove", onPointerMove);
    window.removeEventListener("pointerup", stopResize);
    window.removeEventListener("pointercancel", stopResize);
  }

  /** 开始拖动。监听挂在 window 上而不是分隔条自身：指针移出那条几像素宽的
   *  窄条后事件仍会继续送到 window，拖动才不会中途断掉。 */
  function startResize(e: PointerEvent) {
    // 只响应左键，避免右键菜单等操作把界面拖乱
    if (e.button !== 0) return;
    e.preventDefault();
    resizing.value = true;
    startPointer = opts.axis === "x" ? e.clientX : e.clientY;
    startSize = size.value;
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", stopResize);
    window.addEventListener("pointercancel", stopResize);
  }

  /** 双击分隔条：在默认尺寸与最小尺寸之间切换，便于快速让出空间 */
  function toggleSize() {
    size.value = size.value <= opts.min + 8 ? clamp(opts.defaultSize) : opts.min;
  }

  /** 键盘操作：分隔条是 role="separator" 的可聚焦元素，
   *  只支持鼠标拖动会让键盘用户无法调整布局。
   *
   * 方向键的含义与鼠标拖动一致：「往分隔条自己移动的方向」= 变大。
   * 横向时右方向键加宽、纵向时下方向键加高。 */
  function onSplitterKey(e: KeyboardEvent) {
    const step = e.shiftKey ? 40 : 8;
    const grow = opts.axis === "x" ? "ArrowRight" : "ArrowDown";
    const shrink = opts.axis === "x" ? "ArrowLeft" : "ArrowUp";
    if (e.key === grow) {
      size.value = clamp(size.value + step);
    } else if (e.key === shrink) {
      size.value = clamp(size.value - step);
    } else if (e.key === "Home") {
      size.value = clamp(opts.min);
    } else if (e.key === "End") {
      size.value = clamp(opts.max);
    } else {
      return;
    }
    e.preventDefault();
  }

  /** 窗口变化时把越界的尺寸收回来 */
  function onWindowResize() {
    const clamped = clamp(size.value);
    if (clamped !== size.value) size.value = clamped;
  }

  onMounted(() => {
    window.addEventListener("resize", onWindowResize);
    onWindowResize();
  });

  // 卸载时摘掉 window 上的监听，避免拖动中关闭界面而留下悬挂的监听
  onBeforeUnmount(() => {
    window.removeEventListener("resize", onWindowResize);
    stopResize();
  });

  return {
    size,
    resizing,
    clamp,
    startResize,
    toggleSize,
    onSplitterKey,
  };
}
