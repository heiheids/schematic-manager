/**
 * 方块统计表（详情面板「方块」页签的内容）。
 *
 * 列宽拖动、分页图标懒加载、每行的替换入口这三件事都只服务于这张表，
 * 独立成组件后详情面板不用背这些细节。状态在 store/scenePreview.ts，
 * 替换动作在 store/blockReplace.ts（openReplaceFor / onBlockRowClick）。
 *
 * 名字列显示的是「显示名称」：后端同时给出中英两套（取自 Minecraft 语言表），
 * 这里按当前界面语言取一套，缺那套就退另一套并标灰说明。
 *
 * 「区分属性」开关决定这张表按什么口径分组：关闭时按物品合计（材料清单口径，
 * 一个物品一行，没有状态列），开启时每个方块状态一行（替换规则的命中口径，
 * 状态列显示完整状态键）。两种口径的数量含义不同，说明文字随之切换。
 */
<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import type { TableData } from "@arco-design/web-vue";
import { displayName, isFallbackName } from "../i18n";
import { useAppStore } from "../store";
import { useListKeyboard } from "../composables/useListKeyboard";
import { formatStacks } from "../utils/format";
import type { BlockRow } from "../types";

const { t } = useI18n();

const {
  analysis,
  analyzing,
  blockTableKey,
  blockTableColumns,
  blockSplitStates,
  BLOCK_PAGE_SIZE,
  blockPage,
  blockRows,
  blockPageRows,
  activeBlock,
  iconOf,
  iconErrorOf,
  copyBlockTable,
  exportBlockTable,
  resetBlockColumnWidths,
  onBlockColumnResize,
  onBlockRowClick,
  openReplaceFor,
  saving,
} = useAppStore();

/** 键盘导航的挂载点，见 DirSidebar 里同样的用法 */
const tableHost = ref<HTMLElement | null>(null);

/** 当前选中方块在本页里的下标；-1 表示没有选中。 */
function activeRowIndex(): number {
  const cur = activeBlock.value;
  if (!cur) return -1;
  return blockPageRows.value.findIndex((r) => r.key === cur.key);
}

/** 选中行的底色，与备份表 / 实体表同一套观感 */
function rowClass(raw: TableData): string {
  const rec = raw as unknown as BlockRow;
  return rec.key === activeBlock.value?.key ? "block-row--active" : "";
}

useListKeyboard({
  host: tableHost,
  rowCount: () => blockPageRows.value.length,
  activeIndex: activeRowIndex,
  onSelect: (i) => {
    const target = blockPageRows.value[i];
    if (target) onBlockRowClick(target as never);
  },
  // 回车打开选中行的替换窗口：方向键只移动当前行，不替用户做决定
  onActivate: (i) => {
    const target = blockPageRows.value[i];
    if (target && !saving.value) void openReplaceFor(target);
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
        <!-- 区分属性：开启后每种方块状态单独一行并显示状态列。
             替换是按「方块名 + 属性」精确匹配的，需要按状态核对时把开关打开 -->
        <span class="block-split" :title="t('stats.distinguishTitle')">
          <a-switch v-model="blockSplitStates" size="small" />
          <!-- 文字也能点：开关本身很小，只点它容易点偏。
               不能把它包在 label 里——label 关联不了 role=switch 的按钮，
               点文字不会有任何反应 -->
          <span class="block-split__label" @click="blockSplitStates = !blockSplitStates">
            {{ t("stats.distinguish") }}
          </span>
        </span>
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
      <div class="hint-dim" style="margin-bottom: 6px">
        {{ blockSplitStates ? t("stats.hintSplit") : t("stats.hint") }}
      </div>
      <div ref="tableHost" class="block-table-host" tabindex="0">
        <a-table
          :key="blockTableKey"
          class="block-table scroll-table"
          :row-class="rowClass"
          :columns="blockTableColumns"
          :data="blockRows"
          :pagination="{ pageSize: BLOCK_PAGE_SIZE, current: blockPage }"
          row-key="key"
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
          <!-- 方块名。合计了多种方块形态时（火把与墙上的火把）在这里标注：
               状态列在折叠视图下不显示，不标的话用户没法从这一行看出它跨了几个
               方块名，而替换只覆盖其中一个 -->
          <template #name="{ record }">
            <span
              :class="{ 'hint-dim': record.forms.length > 1 }"
              :title="
                record.forms.length > 1
                  ? t('stats.mergedFormsTitle', { forms: record.forms.join(t('common.listSeparator')) })
                  : undefined
              "
            >
              {{ record.name }}
              <template v-if="record.forms.length > 1">
                {{ t("stats.mergedFormsShort", { count: record.forms.length }) }}
              </template>
            </span>
          </template>
          <template #stacks="{ record }">{{ formatStacks(record.count) }}</template>
          <!-- 行内替换入口：点它打开替换窗口，并把这一行设为源方块。
             行点击只负责高亮（表格首先是统计表），因此要阻止冒泡 -->
          <template #act="{ record }">
            <a-link class="block-act" :disabled="saving" @click.stop="openReplaceFor(record)">{{
              t("stats.replaceRow")
            }}</a-link>
          </template>
        </a-table>
      </div>
    </template>
    <a-empty v-else :description="t('stats.noBlocks')" />
  </a-spin>
</template>
