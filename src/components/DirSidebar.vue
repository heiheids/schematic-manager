/**
 * 左侧目录栏：文件操作条、重复筛查结果条、投影文件表格、
 * 底部多选操作条，以及行右键菜单。
 *
 * 右键菜单虽然浮在全窗口坐标上，但它只服务于这张表的行，
 * 因此跟着表格放在这里。宽度本身由 App 的分隔条控制（useSidebarResize）。
 * 状态在 store/fileList.ts。
 *
 * 表格的纵向滚动由 Arco Table 自带的滚动条负责（它内部就用 Arco 的
 * Scrollbar），轨道已在全局样式里去掉，不留原生滚动条。
 */
<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";
import { useListKeyboard } from "../composables/useListKeyboard";
import { formatDims } from "../utils/format";

const { t } = useI18n();

const {
  sidebarWidth,
  files,
  filteredFiles,
  selected,
  searchText,
  dupBusy,
  scanDuplicatesInline,
  tableColumns,
  tableKey,
  checkedKeys,
  checkedCount,
  hasChecked,
  rowClass,
  onSelectionChange,
  onColumnResize,
  onRowClick,
  onRowContextMenu,
  fileBusy,
  doDeleteFiles,
  doCopyFiles,
  doCopyFilesTo,
  doOpenFolder,
  resetColumnWidths,
  dupResult,
  dupSuspects,
  dupFileCount,
  suspectFileCount,
  dupNoOf,
  dupColorNo,
  isDup,
  isSuspect,
  clearDuplicateScan,
  checkAll,
  invertChecks,
  clearChecks,
  ctxMenu,
  revealRow,
  sortKey,
  sortOrder,
  setSort,
} = useAppStore();

/** 排序字段清单：顺序就是下拉里的顺序，值对应 store 的 SortKey。
 *  单独列一份是为了下拉能显示「按路径」这一项（它不是排序字段）。 */
const SORT_FIELDS = [
  { key: "fileName", label: "files.sortByFileName" },
  { key: "name", label: "files.sortByName" },
  { key: "mcVersionName", label: "files.sortByVersion" },
  { key: "timeModified", label: "files.sortByTime" },
  { key: "author", label: "files.sortByAuthor" },
] as const;

/** 下拉当前值：未排序时用空串表示「按路径」 */
const sortFieldValue = computed(() => sortKey.value ?? "");

/** 表格容器：键盘导航要绑在它上面，而不是 window——
 *  绑 window 会让整个应用抢方向键，在搜索框里按方向键就动不了光标。 */
const tableHost = ref<HTMLElement | null>(null);

/** 当前选中行在列表里的下标。
 *
 * 这份表不分页，filteredFiles 就是屏幕上的全部行，因此下标可以直接由
 * 选中项的路径在 filteredFiles 里查出来。按路径而不是按对象引用比：
 * 列表重排或重新扫描后对象会被换掉，引用比较会失效。
 */
function activeRowIndex(): number {
  const cur = selected.value;
  if (!cur) return -1;
  return filteredFiles.value.findIndex((f) => f.path === cur.path);
}

useListKeyboard({
  host: tableHost,
  rowCount: () => filteredFiles.value.length,
  activeIndex: activeRowIndex,
  onSelect: (i) => {
    const target = filteredFiles.value[i];
    if (target) void onRowClick(target as never);
  },
  // 这份表不分页，翻页键退回为「跳首/末行」
  paged: () => false,
});

/** 选字段：选中即按该字段升序排；选「按路径」则取消排序。
 *
 * 下拉只负责「选哪个字段」，不负责换方向——重复选同一个字段仍然是升序，
 * 不会莫名其妙变成降序。换方向由旁边的 ↑/↓ 按钮做。 */
function onSortFieldChange(v: unknown) {
  if (v === "" || v === undefined) {
    setSort(null, null);
    return;
  }
  setSort(v as (typeof SORT_FIELDS)[number]["key"], "ascend");
}
</script>

<template>
  <div class="sidebar" :style="{ width: sidebarWidth + 'px' }">
    <!-- 文件操作条：在目录栏顶部、靠左。
         作用于勾选的文件；未勾选时禁用，避免误操作到「看起来选中」的行。
         搜索与重复筛查放在这里而不是顶部工具条：它们作用于这张列表，
         入口与被作用的对象应当在同一区域。 -->
    <div class="sidebar__ops">
      <a-input-search
        v-model="searchText"
        size="mini"
        :placeholder="t('files.searchPlaceholder')"
        class="sidebar__search"
        allow-clear
      />
      <a-button size="mini" :loading="dupBusy" @click="scanDuplicatesInline">
        {{ t("files.duplicates") }}
      </a-button>
      <a-button
        size="mini"
        status="danger"
        :disabled="!hasChecked"
        :loading="fileBusy"
        @click="doDeleteFiles"
      >
        {{ t("files.delete") }}
      </a-button>
      <a-button size="mini" :disabled="!hasChecked" :loading="fileBusy" @click="doCopyFiles">
        {{ t("files.copy") }}
      </a-button>
      <a-button size="mini" :disabled="!hasChecked" :loading="fileBusy" @click="doCopyFilesTo">
        {{ t("files.copyTo") }}
      </a-button>
      <a-button size="mini" :disabled="!hasChecked" @click="doOpenFolder">
        {{ t("files.openFolder") }}
      </a-button>
      <a-button size="mini" @click="resetColumnWidths" :title="t('files.resetWidthsTitle')">
        {{ t("files.resetWidths") }}
      </a-button>
    </div>

    <!-- 重复筛查结果条：就地显示在列表上方，不再单开弹窗。
         重复行已排到列表最前并按组上色（见 rowClass 与 #fileName 的组号），
         这里只给汇总与清除入口。 -->
    <div v-if="dupResult" class="sidebar__dups">
      <span class="sidebar__dups-text">
        <template v-if="dupResult.groups.length > 0">
          <i18n-t scope="global" keypath="files.dupFound" tag="span">
            <template #groups><b>{{ dupResult.groups.length }}</b></template>
            <template #files><b>{{ dupFileCount }}</b></template>
          </i18n-t>
        </template>
        <template v-else>{{ t("files.dupNotFound") }}</template>
        <template v-if="dupSuspects.length > 0">
          <i18n-t scope="global" keypath="files.dupSuspects" tag="span">
            <template #groups><b>{{ dupSuspects.length }}</b></template>
            <template #files>{{ suspectFileCount }}</template>
          </i18n-t>
        </template>
      </span>
      <a-button size="mini" @click="clearDuplicateScan">
        {{ dupResult.groups.length > 0 ? t("files.clearMarks") : t("files.gotIt") }}
      </a-button>
    </div>
    <a-alert
      v-if="dupResult && (dupResult.unreadable.length > 0 || dupResult.skippedDirs > 0)"
      type="warning"
      style="margin: 0 0 4px; flex: 0 0 auto"
    >
      <template v-if="dupResult.unreadable.length > 0">
        {{ t("files.unreadableFiles", { count: dupResult.unreadable.length }) }}
      </template>
      <template v-if="dupResult.unreadable.length > 0 && dupResult.skippedDirs > 0">
        {{ t("files.unreadableBothSeparator") }}
      </template>
      <template v-if="dupResult.skippedDirs > 0">
        {{ t("files.unreadableDirs", { count: dupResult.skippedDirs }) }}
      </template>
    </a-alert>

    <!-- 键盘导航的挂载点：事件绑在这个 div 上而不是 a-table 组件上
         （组件实例不是 DOM，拿不到 addEventListener）。tabindex 让它可以
         获得焦点，用户点一下列表再按方向键就会在这里命中。 -->
    <div ref="tableHost" class="sidebar__table-host" tabindex="0">
      <a-table
        :key="tableKey"
        :columns="tableColumns"
      :data="filteredFiles"
      :pagination="false"
      row-key="path"
      size="small"
      :bordered="{ wrapper: true, cell: false }"
      :scroll="{ y: '100%' }"
      column-resizable
      :row-selection="{ type: 'checkbox', showCheckedAll: false, width: 40 }"
      :selected-keys="checkedKeys"
      :row-class="rowClass"
      @selection-change="onSelectionChange"
      @column-resize="onColumnResize"
      @row-click="onRowClick"
      @row-contextmenu="onRowContextMenu"
    >
      <template #fileName="{ record }">
        <span class="dup-name">
          <!-- 组号色块与行底色同色：颜色负责「哪几份是一组」，
               数字负责在颜色循环复用时仍能分辨，两者互为佐证。 -->
          <span
            v-if="isDup(record.path)"
            class="dup-chip"
            :class="`dup-chip-${dupColorNo(record.path)}`"
            :title="t('files.dupGroupTitle', { no: dupNoOf(record.path) })"
          >{{ dupNoOf(record.path) }}</span>
          <span
            v-else-if="isSuspect(record.path)"
            class="dup-chip dup-chip--suspect"
            :title="t('files.suspectTitle')"
          >{{ t("files.suspectChip") }}</span>
          <!-- 名称列关掉了 Arco 的 tooltip（它量不准插槽内容），
               完整文件名只能在这里自己给出来 -->
          <span class="dup-name__text" :title="record.fileName">{{ record.fileName }}</span>
        </span>
      </template>
      <template #dims="{ record }">{{ formatDims(record.enclosing) }}</template>
      <template #empty>
        <a-empty :description="t('files.empty')" />
      </template>
      </a-table>
    </div>

    <!-- 目录栏底部：多选与排序操作 -->
    <div class="sidebar__checks">
      <!-- 搜索时把「共多少、匹配多少」一并给出来：只显示匹配数会让人
           以为文件变少了 -->
      <span class="hint-dim">
        <template v-if="searchText">
          {{
            t("files.checkedCountOfAll", {
              checked: checkedCount,
              total: filteredFiles.length,
              all: files.length,
              matched: filteredFiles.length,
            })
          }}
        </template>
        <template v-else>
          {{ t("files.checkedCount", { checked: checkedCount, total: filteredFiles.length }) }}
        </template>
      </span>
      <span class="preview__spacer" />
      <!-- 排序：字段下拉 + 方向按钮。放在这条而不是列头上，因为要排的
           「名称 / 时间 / 作者」并不是列表列（列表只有文件名/版本/尺寸/方块数），
           为排序把它们加成列会把目录栏撑宽，与布局优化冲突。
           方向按钮只在选了字段后可用：按路径排序无所谓升序降序。 -->
      <a-select
        :model-value="sortFieldValue"
        size="mini"
        :title="t('files.sortToggleTip')"
        style="width: 116px"
        @change="onSortFieldChange"
      >
        <a-option value="">{{ t("files.sortByPath") }}</a-option>
        <a-option v-for="f in SORT_FIELDS" :key="f.key" :value="f.key">
          {{ t(f.label) }}
        </a-option>
      </a-select>
      <a-button
        size="mini"
        :disabled="!sortKey"
        :title="sortOrder === 'descend' ? t('files.sortDesc') : t('files.sortAsc')"
        @click="sortKey && setSort(sortKey, sortOrder === 'ascend' ? 'descend' : 'ascend')"
      >
        {{ sortOrder === "descend" ? "↓" : "↑" }}
      </a-button>
      <a-button size="mini" :disabled="filteredFiles.length === 0" @click="checkAll">
        {{ t("files.checkAll") }}
      </a-button>
      <a-button size="mini" :disabled="filteredFiles.length === 0" @click="invertChecks">
        {{ t("files.invert") }}
      </a-button>
      <a-button size="mini" :disabled="!hasChecked" @click="clearChecks">
        {{ t("files.clear") }}
      </a-button>
    </div>

    <!-- 行的右键菜单：只保留「打开所在目录」 -->
    <div
      v-if="ctxMenu.open"
      class="ctx-menu"
      :style="{ left: ctxMenu.x + 'px', top: ctxMenu.y + 'px' }"
      role="menu"
      @contextmenu.prevent
    >
      <div
        class="ctx-menu__item"
        role="menuitem"
        tabindex="0"
        @click="revealRow({ path: ctxMenu.path, name: ctxMenu.name })"
        @keydown.enter="revealRow({ path: ctxMenu.path, name: ctxMenu.name })"
      >
        {{ t("files.openFolder") }}
      </div>
    </div>
  </div>
</template>
