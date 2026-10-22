/**
 * 实体统计表（详情面板「实体」页签的内容）。
 *
 * 状态在 store/scenePreview.ts；实体坐标只如实汇报数量，
 * 预览渲染留待后续功能。
 *
 * 名字列与方块表同理：后端给出中英两套显示名称，按当前界面语言取一套。
 */
<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import type { TableData } from "@arco-design/web-vue";
import { displayName, isFallbackName } from "../i18n";
import { useAppStore } from "../store";
import { useListKeyboard } from "../composables/useListKeyboard";

const { t } = useI18n();

const { analysis, entityColumns } = useAppStore();

/** 当前选中的实体行下标；-1 表示未选中。
 *
 * 这张表原本没有「选中」概念（行不可点），方向键也就无从落脚。
 * 加上选中态让键盘能逐行浏览实体清单，也让「当前看的是哪一行」有反馈。
 */
const activeIndex = ref(-1);
/** 键盘导航的挂载点 */
const tableHost = ref<HTMLElement | null>(null);

useListKeyboard({
  host: tableHost,
  rowCount: () => analysis.value?.entities.length ?? 0,
  activeIndex: () => activeIndex.value,
  onSelect: (i) => {
    activeIndex.value = i;
  },
  // 实体表不分页
  paged: () => false,
});

/** 选中行的高亮 */
function rowClass(raw: TableData): string {
  const rows = analysis.value?.entities ?? [];
  const idx = rows.findIndex((r) => r.id === (raw as unknown as { id: string }).id);
  return idx === activeIndex.value ? "entity-row--active" : "";
}
</script>

<template>
  <template v-if="analysis">
    <div ref="tableHost" class="entity-table-host" tabindex="0">
      <a-table
        :columns="entityColumns"
        :data="analysis.entities"
        :pagination="false"
        :row-class="rowClass"
        row-key="id"
        size="small"
      >
      <template #display="{ record }">
        <span
          v-if="displayName(record)"
          :class="{ 'hint-dim': isFallbackName(record) }"
          :title="isFallbackName(record) ? t('displayName.missing') : undefined"
        >
          {{ displayName(record) }}
        </span>
        <span v-else class="hint-dim" :title="t('displayName.missing')">—</span>
      </template>
      </a-table>
    </div>
    <!-- 坐标已随分析一起读出并存入，供以后做实体预览。
         这里如实说明读到了多少个，而不是让用户以为只有统计数字。 -->
    <div v-if="analysis.entities.length > 0" class="hint-dim" style="margin-top: 6px">
      {{ t("stats.entityInstances", { count: analysis.entityInstances.length }) }}
    </div>
    <a-empty
      v-if="analysis.entities.length === 0"
      :description="t('stats.noEntities')"
    />
  </template>
  <a-empty v-else :description="t('stats.noEntitiesSelected')" />
</template>
