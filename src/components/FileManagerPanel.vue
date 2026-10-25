<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";
import DirectoryTree from "./DirectoryTree.vue";
import SchematicCards from "./SchematicCards.vue";

const { t } = useI18n();
const {
  searchText,
  dupBusy,
  scanDuplicatesInline,
  dupResult,
  dupFileCount,
  dupSuspects,
  suspectFileCount,
  clearDuplicateScan,
  fileBusy,
  doDeleteFiles,
  doCopyFiles,
  doCopyFilesTo,
  doOpenFolder,
  visibleFiles,
  visibleCheckedFiles,
  visibleCheckedCount,
  checkAllVisible,
  invertVisible,
  clearVisible,
  showDescendants,
  sortKey,
  sortOrder,
  setSort,
  ctxMenu,
  revealRow,
} = useAppStore();

const SORT_FIELDS = [
  { key: "fileName", label: "files.sortByFileName" },
  { key: "name", label: "files.sortByName" },
  { key: "mcVersionName", label: "files.sortByVersion" },
  { key: "timeModified", label: "files.sortByTime" },
  { key: "author", label: "files.sortByAuthor" },
] as const;
const sortFieldValue = computed(() => sortKey.value ?? "");

function onSortFieldChange(value: unknown) {
  if (value === "" || value === undefined) {
    setSort(null, null);
    return;
  }
  setSort(value as (typeof SORT_FIELDS)[number]["key"], "ascend");
}
</script>

<template>
  <div class="file-manager">
    <div class="file-manager__ops">
      <a-input-search
        v-model="searchText"
        size="mini"
        class="file-manager__search"
        :placeholder="t('files.searchPlaceholder')"
        allow-clear
      />
      <a-button size="mini" :loading="dupBusy" @click="scanDuplicatesInline">
        {{ t("files.duplicates") }}
      </a-button>
      <a-button size="mini" status="danger" :disabled="!visibleCheckedCount" :loading="fileBusy" @click="doDeleteFiles(visibleCheckedFiles)">
        {{ t("files.delete") }}
      </a-button>
      <a-button size="mini" :disabled="!visibleCheckedCount" :loading="fileBusy" @click="doCopyFiles(visibleCheckedFiles)">
        {{ t("files.copy") }}
      </a-button>
      <a-button size="mini" :disabled="!visibleCheckedCount" :loading="fileBusy" @click="doCopyFilesTo(visibleCheckedFiles)">
        {{ t("files.copyTo") }}
      </a-button>
      <a-button size="mini" :disabled="!visibleCheckedCount" @click="doOpenFolder(visibleCheckedFiles)">
        {{ t("files.openFolder") }}
      </a-button>
    </div>

    <div v-if="dupResult" class="file-manager__dups">
      <span>
        <template v-if="dupResult.groups.length">
          {{ t("files.dupFound", { groups: dupResult.groups.length, files: dupFileCount }) }}
        </template>
        <template v-else>{{ t("files.dupNotFound") }}</template>
        <template v-if="dupSuspects.length">
          {{ t("files.dupSuspects", { groups: dupSuspects.length, files: suspectFileCount }) }}
        </template>
      </span>
      <a-button size="mini" @click="clearDuplicateScan">
        {{ dupResult.groups.length ? t("files.clearMarks") : t("files.gotIt") }}
      </a-button>
    </div>
    <a-alert
      v-if="dupResult && (dupResult.unreadable.length || dupResult.skippedDirs)"
      type="warning"
      class="file-manager__warning"
    >
      <span v-if="dupResult.unreadable.length">{{ t("files.unreadableFiles", { count: dupResult.unreadable.length }) }}</span>
      <span v-if="dupResult.unreadable.length && dupResult.skippedDirs">{{ t("files.unreadableBothSeparator") }}</span>
      <span v-if="dupResult.skippedDirs">{{ t("files.unreadableDirs", { count: dupResult.skippedDirs }) }}</span>
    </a-alert>

    <div class="file-manager__body">
      <DirectoryTree />
      <div class="file-manager__content">
        <div class="file-manager__content-head">
          <span class="hint-dim">{{ t("fileManager.fileCount", { count: visibleFiles.length }) }}</span>
          <span class="preview__spacer" />
          <span class="file-manager__descendants">
            <a-switch v-model="showDescendants" size="small" />
            <span @click="showDescendants = !showDescendants">{{ t("fileManager.includeDescendants") }}</span>
          </span>
        </div>
        <SchematicCards />
        <div class="file-manager__checks">
          <span class="hint-dim">{{ t("files.checkedCount", { checked: visibleCheckedCount, total: visibleFiles.length }) }}</span>
          <span class="preview__spacer" />
          <a-select :model-value="sortFieldValue" size="mini" style="width: 116px" @change="onSortFieldChange">
            <a-option value="">{{ t("files.sortByPath") }}</a-option>
            <a-option v-for="field in SORT_FIELDS" :key="field.key" :value="field.key">{{ t(field.label) }}</a-option>
          </a-select>
          <a-button size="mini" :disabled="!sortKey" @click="sortKey && setSort(sortKey, sortOrder === 'ascend' ? 'descend' : 'ascend')">
            {{ sortOrder === "descend" ? "↓" : "↑" }}
          </a-button>
          <a-button size="mini" :disabled="!visibleFiles.length" @click="checkAllVisible">{{ t("files.checkAll") }}</a-button>
          <a-button size="mini" :disabled="!visibleFiles.length" @click="invertVisible">{{ t("files.invert") }}</a-button>
          <a-button size="mini" :disabled="!visibleCheckedCount" @click="clearVisible">{{ t("files.clear") }}</a-button>
        </div>
      </div>
    </div>

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
