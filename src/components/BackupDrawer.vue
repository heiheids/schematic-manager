/**
 * 备份与日志抽屉：备份清单（还原/删除）与全局运行日志两个页签。
 *
 * 状态与动作都在 store/backups.ts，这里只做展示与触发。
 *
 * 抽屉上下各留 16px（见 global.scss 的 .arco-drawer）：贴满整屏高度会像页面被
 * 切掉一块，与其它弹出层的处理也不一致。
 * 需要滚动的是内容区：运行日志可以很长，放在 Arco 的无轨道滚动条里。
 */
<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import type { TableData } from "@arco-design/web-vue";
import { useAppStore } from "../store";
import { useListKeyboard } from "../composables/useListKeyboard";
import RunLogList from "./RunLogList.vue";
import { formatSize } from "../utils/format";

const { t } = useI18n();

const {
  selected,
  backupOpen,
  backups,
  backupLoading,
  logs,
  logLoading,
  formatTime,
  refreshBackups,
  refreshLogs,
  openLogs,
  onRestore,
  onDelete,
} = useAppStore();

/** 当前选中的备份行下标；-1 表示未选中。
 *
 * 这张表原本没有「选中」概念（每行各自带两个操作链接），方向键也就无处落脚。
 * 加上选中态既让键盘导航有反馈，也让「当前在看哪一行」在长列表里更清楚。
 */
const backupActive = ref(-1);
const BACKUP_PAGE_SIZE = 12;
const backupPage = ref(1);
/** 键盘导航的挂载点 */
const backupTableHost = ref<HTMLElement | null>(null);

/** 当前页可见的行：导航的行数与下标都按这一份算 */
const backupPageRows = computed(() => {
  const start = (backupPage.value - 1) * BACKUP_PAGE_SIZE;
  return backups.value.slice(start, start + BACKUP_PAGE_SIZE);
});

/** 翻页或刷新后下标可能越界，落到第一行才能继续用方向键 */
function clampBackupActive() {
  if (backupActive.value >= backupPageRows.value.length) {
    backupActive.value = backupPageRows.value.length > 0 ? 0 : -1;
  }
}

useListKeyboard({
  host: backupTableHost,
  rowCount: () => backupPageRows.value.length,
  activeIndex: () => backupActive.value,
  onSelect: (i) => {
    backupActive.value = i;
  },
  paged: () => true,
  page: () => backupPage.value,
  pageCount: () => Math.max(1, Math.ceil(backups.value.length / BACKUP_PAGE_SIZE)),
  onPage: (p) => {
    backupPage.value = p;
    clampBackupActive();
  },
});

/** 选中行的底色，与投影列表的选中样式保持一致的做法 */
function backupRowClass(raw: TableData): string {
  const idx = backupPageRows.value.findIndex(
    (b) => b.path === (raw as unknown as { path: string }).path,
  );
  return idx === backupActive.value ? "backup-row--active" : "";
}

/** 列定义随语言变化，写在这里而不是常量里 */
const columns = computed(() => [
  { title: t("backup.colTime"), slotName: "time", width: 170 },
  { title: t("backup.colOp"), dataIndex: "op", width: 110 },
  { title: t("backup.colSize"), slotName: "size", width: 90 },
  { title: t("backup.colAct"), slotName: "act", width: 150 },
]);
</script>

<template>
  <a-drawer
    v-model:visible="backupOpen"
    :title="t('backup.title')"
    :width="620"
    :footer="false"
    unmount-on-close
  >
    <a-tabs default-active-key="backups">
      <a-tab-pane key="backups" :title="t('backup.tabBackups', { count: backups.length })">
        <a-space style="margin-bottom: 8px">
          <a-button size="small" @click="refreshBackups">{{ t("backup.refresh") }}</a-button>
          <span v-if="selected" class="hint-dim">
            {{ t("backup.onlyFor", { name: selected.fileName }) }}
          </span>
        </a-space>
        <div ref="backupTableHost" class="backup-table-host" tabindex="0">
        <a-table
          :columns="columns"
          :data="backups"
          :loading="backupLoading"
          :pagination="{
            pageSize: BACKUP_PAGE_SIZE,
            current: backupPage,
            total: backups.length,
          }"
          :row-class="backupRowClass"
          row-key="path"
          size="small"
          @page-change="(p: number) => { backupPage = p; clampBackupActive(); }"
        >
          <template #time="{ record }">{{ formatTime(record.createdNs) }}</template>
          <template #size="{ record }">{{ formatSize(record.size) }}</template>
          <template #act="{ record }">
            <a-space>
              <a-link :disabled="!selected" @click="onRestore(record)">
                {{ t("backup.restore") }}
              </a-link>
              <a-popconfirm :content="t('backup.deleteConfirm')" @ok="onDelete(record)">
                <a-link status="danger">{{ t("backup.delete") }}</a-link>
              </a-popconfirm>
            </a-space>
          </template>
          <template #empty>
            <a-empty :description="selected ? t('backup.noBackups') : t('backup.pickFile')" />
          </template>
        </a-table>
        </div>
      </a-tab-pane>

      <a-tab-pane key="logs" :title="t('backup.tabLogs')">
        <a-space style="margin-bottom: 8px">
          <a-button size="small" @click="refreshLogs">{{ t("backup.refresh") }}</a-button>
          <!-- 日志管理窗口（清空、打开日志目录）从这里进：它原先挂在资源包
               管理窗口上，那个窗口已移除；日志属于备份与运维这条线，放在
               同一条运行日志页签里最顺手 -->
          <a-button size="small" @click="openLogs">{{ t("backup.logManage") }}</a-button>
        </a-space>
        <!-- 日志可能很长：内容区自己滚（Arco 无轨道滚动条）。
             滚动容器要有**确定**高度才滚得动（只给 max-height 不行）：这里套一层
             确定高度的纵向 flex 容器，滚动区在里面 flex 伸缩。
             短日志下方留些空白可以接受，比「列表被截断且滚不动」好。 -->
        <div class="log-scroll-host">
          <a-scrollbar type="embed" class="scroll-fill__inner" outer-class="scroll-fill">
            <a-spin :loading="logLoading" style="width: 100%">
              <a-empty v-if="logs.length === 0" :description="t('log.empty')" />
              <RunLogList v-else :lines="logs" />
            </a-spin>
          </a-scrollbar>
        </div>
      </a-tab-pane>
    </a-tabs>
  </a-drawer>
</template>
