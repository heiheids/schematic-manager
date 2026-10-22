<script setup lang="ts">
/**
 * 运行日志列表：逐条分开显示。
 *
 * 「备份与日志」的日志页签与「日志管理」窗口共用这一份：两处显示的是同一份
 * 数据，各写一遍迟早会长歪——原先两处各有一份一模一样的模板。
 *
 * 每条日志拆成「时间 / 等级 / 动作 / 细节」四段（见 utils/run_log.ts）：
 * 时间戳单独一列并格式化成日期，等级与动作各自成标签，细节另起一段换行。
 * 一条日志里的文件名可能很长，但边界清楚，逐条阅读不再吃力。
 */
import { computed, ref } from "vue";
import { t } from "../i18n";
import { useAppStore } from "../store";
import { useListKeyboard } from "../composables/useListKeyboard";
import { LOG_LEVEL_KEYS, parseLogLine } from "../utils/run_log";
import type { LogLevel } from "../utils/run_log";

const props = defineProps<{ lines: string[] }>();

const { formatTime } = useAppStore();

/** 键盘导航的挂载点 */
const listHost = ref<HTMLElement | null>(null);
/** 当前选中的日志下标；-1 表示未选中 */
const activeIndex = ref(-1);

/** 等级标签的颜色。用 Arco 标签的内置色板：深浅主题各有一套取值，
 *  因此这里不写死颜色，也不必在全局样式里再配一遍深色。 */
const LEVEL_COLORS: Record<LogLevel, string> = {
  error: "red",
  warn: "orange",
  info: "blue",
  debug: "gray",
};

const entries = computed(() =>
  props.lines.map((line) => {
    const entry = parseLogLine(line);
    return {
      ...entry,
      levelText: entry.level ? t(LOG_LEVEL_KEYS[entry.level]) : "",
      levelColor: entry.level ? LEVEL_COLORS[entry.level] : "",
    };
  }),
);

/** 时间戳解析不出来时如实说明，而不是显示成 1970 年 */
function timeText(timeNs: string): string {
  return timeNs ? formatTime(timeNs) : t("logList.timeUnknown");
}

useListKeyboard({
  host: listHost,
  rowCount: () => entries.value.length,
  activeIndex: () => activeIndex.value,
  onSelect: (i) => {
    activeIndex.value = i;
  },
  // 日志不分页：翻页键退回为「跳到最新 / 最早一条」
  paged: () => false,
  // 这里不是表格，行是 v-for 出来的 div
  rowSelector: ".log-list__row",
});
</script>

<template>
  <div ref="listHost" class="log-list" tabindex="0">
    <div
      v-for="(e, i) in entries"
      :key="i"
      class="log-list__row"
      :class="{ 'log-list__row--active': i === activeIndex }"
    >
      <span class="log-list__time">{{ timeText(e.timeNs) }}</span>
      <div class="log-list__body">
        <a-tag v-if="e.level" class="log-list__level" size="small" :color="e.levelColor">
          {{ e.levelText }}
        </a-tag>
        <span v-if="e.action" class="log-list__action">{{ e.action }}</span>
        <span v-if="e.detail" class="log-list__detail">{{ e.detail }}</span>
      </div>
    </div>
  </div>
</template>
