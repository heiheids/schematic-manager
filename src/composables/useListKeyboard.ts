/**
 * 列表的键盘导航：方向键换选中行、翻页键翻页、Home / End 跳首尾。
 *
 * 抽成一处的理由：界面上有六七处列表（投影列表、方块表、实体表、备份、
 * 转换窗口、日志），各自的状态存放位置都不同——有的在 store 的 selected，
 * 有的是本地的 activeItem，有的压根还没有「选中」这个概念。但它们要的
 * 键盘行为完全一样，因此这里只管「按键 → 应该选中第几行」，
 * 真正的选中动作由使用方通过 onSelect 完成。
 *
 * 监听绑在列表容器上而不是 window：绑 window 会让整个应用抢方向键，
 * 用户在搜索框里想移动光标却把列表选中项改了。
 */
import { onBeforeUnmount, watch, type Ref } from "vue";

/** 列表键盘导航的参数。 */
export interface ListKeyboardOptions {
  /** 承载列表的容器，键盘事件从它这里收 */
  host: Ref<HTMLElement | null>;
  /** 当前数据行数（分页后的可见行数，即屏幕上真正列出的那些） */
  rowCount: () => number;
  /** 当前选中行在同页内的下标；没有选中时返回 -1 */
  activeIndex: () => number;
  /** 选中第 index 行（同页内下标） */
  onSelect: (index: number) => void;
  /** 是否分页；不分页时翻页键退回为「跳到首/末行」 */
  paged: () => boolean;
  /** 当前页（从 1 起），仅分页时使用 */
  page?: () => number;
  /** 总页数，仅分页时使用 */
  pageCount?: () => number;
  /** 切到第 n 页（从 1 起），仅分页时使用 */
  onPage?: (page: number) => void;
  /** 行的 CSS 选择器，用于把选中行滚进可视区。
   *
   * 默认是表格的行；日志列表是 v-for 出来的 div，要传自己的类名。
   * 只影响滚动定位，不影响按键逻辑——所以非表格列表也能用这套。
   */
  rowSelector?: string;
}

/** 输入类元素里不接管按键：那些键属于光标与文本操作 */
function isEditable(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || el.isContentEditable;
}

export function useListKeyboard(opts: ListKeyboardOptions) {
  /** 把选中行滚进可视区。
   *
   * Arco 的表体在自己的滚动容器里，选中项跑到视口外时用户看不到高亮，
   * 键盘操作就等于没有反馈。
   *
   * 调用前判断方法是否存在：jsdom 没有实现 scrollIntoView，直接调用会抛
   * TypeError，而异常发生在键盘事件处理器里，只会变成一条未捕获错误，
   * 界面上看不出问题、测试里也仍然是绿的——那正是最难查的一类。
   */
  function reveal(index: number) {
    const host = opts.host.value;
    if (!host) return;
    const rows = host.querySelectorAll<HTMLElement>(opts.rowSelector ?? "tbody tr");
    const row = rows[index];
    if (row && typeof row.scrollIntoView === "function") {
      row.scrollIntoView({ block: "nearest" });
    }
  }

  function select(index: number) {
    const count = opts.rowCount();
    if (count === 0) return;
    const clamped = Math.min(Math.max(index, 0), count - 1);
    opts.onSelect(clamped);
    reveal(clamped);
  }

  function goPage(delta: number) {
    if (!opts.paged() || !opts.page || !opts.pageCount || !opts.onPage) return;
    const next = opts.page() + delta;
    const last = opts.pageCount();
    if (next < 1 || next > last) return;
    opts.onPage(next);
  }

  function onKeydown(e: KeyboardEvent) {
    if (isEditable(e.target)) return;
    const count = opts.rowCount();
    if (count === 0) return;
    const cur = opts.activeIndex();

    switch (e.key) {
      case "ArrowDown":
        // 没有选中时第一次按下选中第一行，与常见列表的习惯一致
        select(cur < 0 ? 0 : cur + 1);
        break;
      case "ArrowUp":
        select(cur < 0 ? count - 1 : cur - 1);
        break;
      case "PageDown":
        if (opts.paged()) goPage(1);
        else select(count - 1);
        break;
      case "PageUp":
        if (opts.paged()) goPage(-1);
        else select(0);
        break;
      case "Home":
        select(0);
        break;
      case "End":
        select(count - 1);
        break;
      default:
        return;
    }
    // 到这里说明这次按键被我们消费了，必须阻止默认行为：
    // 否则方向键会同时滚动外层滚动条，PageUp/PageDown 还会滚整个页面
    e.preventDefault();
  }

  // 监听容器本身，而不是只在挂载时绑一次。
  //
  // 多数列表的容器在 v-if 或抽屉里：挂载那一刻它还不存在（分析结果没到、
  // 对话框没打开），一次性绑定会静默地绑到 null 上，按键永远没反应。
  // 容器出现时绑、消失时解绑，才能覆盖这些情况。
  watch(
    opts.host,
    (el, prev) => {
      prev?.removeEventListener("keydown", onKeydown);
      el?.addEventListener("keydown", onKeydown);
    },
    { immediate: true, flush: "post" },
  );

  onBeforeUnmount(() => {
    opts.host.value?.removeEventListener("keydown", onKeydown);
  });

  return { onKeydown };
}
