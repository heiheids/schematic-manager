/**
 * 方块统计表（详情面板「方块」页签的内容）。
 *
 * 列宽拖动、分页图标懒加载、点行填入替换规则这三件事都只服务于这张表，
 * 独立成组件后详情面板不用背这些细节。状态在 store/scenePreview.ts，
 * 点行的回填动作在 store/blockReplace.ts（onBlockRowClick）。
 *
 * 名字列显示的是「显示名称」：后端同时给出中英两套（取自 Minecraft 语言表），
 * 这里按当前界面语言取一套，缺那套就退另一套并标灰说明。
 */
<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import type { TableData } from "@arco-design/web-vue";
import { displayName, isFallbackName } from "../i18n";
import { useAppStore } from "../store";
import { useListKeyboard } from "../composables/useListKeyboard";
import { formatStacks } from "../utils/format";

const { t } = useI18n();

const {
  analysis,
  analyzing,
  openReplace,
  blockTableKey,
  blockTableColumns,
  BLOCK_PAGE_SIZE,
  blockPage,
  blockRows,
  blockPageRows,
  activeBlock,
  blockStateText,
  iconOf,
  iconErrorOf,
  copyBlockTable,
  exportBlockTable,
  resetBlockColumnWidths,
  onBlockColumnResize,
  onBlockRowClick,
} = useAppStore();

/** 键盘导航的挂载点，见 DirSidebar 里同样的用法 */
const tableHost = ref<HTMLElement | null>(null);

/** 当前选中方块在本页里的下标。
 *
 * 用方块名比对而不是行对象引用：分页切换后同一方块会重新生成行对象。
 */
function activeRowIndex(): number {
  const cur = activeBlock.value;
  if (!cur) return -1;
  return blockPageRows.value.findIndex(
    (r) => r.state === cur.name || r.state.startsWith(`${cur.name}[`) || r.name === cur.name,
  );
}

/** 选中行的底色，与备份表 / 实体表同一套观感 */
function rowClass(raw: TableData): string {
  const rec = raw as unknown as { state: string; name: string };
  const idx = blockPageRows.value.findIndex((r) => r.state === rec.state);
  return idx === activeRowIndex() ? "block-row--active" : "";
}

useListKeyboard({
  host: tableHost,
  rowCount: () => blockPageRows.value.length,
  activeIndex: activeRowIndex,
  onSelect: (i) => {
    const target = blockPageRows.value[i];
    if (target) onBlockRowClick(target as never);
  },
  // 方块表分页：翻页键切页，不移动选中
  paged: () => true,
  page: () => blockPage.value,
  pageCount: () => Math.max(1, Math.ceil(blockRows.value.length / BLOCK_PAGE_SIZE)),
  onPage: (p) => {
    blockPage.value = p;
  },
});
</script>

<template>
  <a-spin :loading="analyzing" style="width: 100%; height: 100%">
    <template v-if="analysis">
      <div class="block-head">
        <a-space :size="16">
          <!-- 统计数字要加粗，而冒号在两种语言里宽度不同，因此整句走 i18n-t -->
          <i18n-t scope="global" keypath="stats.nonAir" tag="span">
            <template #value><b>{{ analysis.nonAirTotal }}</b></template>
          </i18n-t>
          <i18n-t scope="global" keypath="stats.air" tag="span">
            <template #value><b>{{ analysis.airTotal }}</b></template>
          </i18n-t>
          <i18n-t scope="global" keypath="stats.blockKinds" tag="span">
            <template #value><b>{{ analysis.distinctBlockTypes }}</b></template>
          </i18n-t>
          <i18n-t scope="global" keypath="stats.stateKinds" tag="span">
            <template #value><b>{{ analysis.distinctStates }}</b></template>
          </i18n-t>
        </a-space>
        <span class="preview__spacer" />
        <!-- 方块替换放在方块列表里：它改的正是这张表里的方块，
             入口与被作用的对象在同一区域。 -->
        <a-button size="mini" type="primary" @click="openReplace">
          {{ t("stats.replace") }}
        </a-button>
        <a-button size="mini" @click="copyBlockTable">{{ t("stats.copyTable") }}</a-button>
        <a-button size="mini" @click="exportBlockTable">{{ t("stats.exportTable") }}</a-button>
        <a-button
          size="mini"
          :title="t('stats.resetWidthsTitle')"
          @click="resetBlockColumnWidths"
        >
          {{ t("stats.resetWidths") }}
        </a-button>
      </div>
      <div class="hint-dim" style="margin-bottom: 6px">{{ t("stats.hint") }}</div>
      <div ref="tableHost" class="block-table-host" tabindex="0">
        <a-table
          :key="blockTableKey"
          class="block-table scroll-table"
          :row-class="rowClass"
        :columns="blockTableColumns"
        :data="blockRows"
        :pagination="{ pageSize: BLOCK_PAGE_SIZE, current: blockPage }"
        row-key="state"
        size="small"
        column-resizable
        @column-resize="onBlockColumnResize"
        @page-change="(p: number) => (blockPage = p)"
        @row-click="onBlockRowClick"
        :scroll="{ y: '100%' }"
      >
        <template #icon="{ record }">
          <img
            v-if="iconOf(record.state)"
            class="block-icon"
            :src="iconOf(record.state)"
            :alt="displayName(record) || record.name"
            :title="displayName(record) || record.name"
          />
          <!-- 画不出来时说清楚原因，而不是留一个让人以为是加载慢的空位 -->
          <span
            v-else-if="iconErrorOf(record.state)"
            class="block-icon block-icon--failed"
            :title="iconErrorOf(record.state)"
            >{{ t("stats.noIcon") }}</span
          >
        </template>
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
        <template #stacks="{ record }">{{ formatStacks(record.count) }}</template>
        <!-- 合并显示的行写明合并了多少种状态：那一行的数量是所有状态的总和，
              只写一个代表状态会让人以为「只有这么多是那个状态的」 -->
        <template #state="{ record }">{{ blockStateText(record) }}</template>
        </a-table>
      </div>
    </template>
    <a-empty v-else :description="t('stats.noBlocks')" />
  </a-spin>
</template>
