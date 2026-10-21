/**
 * 顶部工具条：目录管理（添加/已加载下拉/移除）与各功能弹窗的入口按钮。
 *
 * 只负责把操作转发给 store 各域；按钮的禁用/加载态也从 store 读。
 *
 * 这里只放「不属于任何单个面板」的操作：目录管理、备份与日志、版本库、
 * 更新、RMC 投影站。作用于某个面板的功能入口放在那个面板自己身上——
 * 搜索与重复筛查在投影列表、方块替换在方块列表、版本转换在投影信息区，
 * 这样入口与被作用的对象在同一区域，不必来回找。
 *
 * 语言切换不在这里：这一行有十来个按钮，实测按内容排下来约需 1650px，
 * 而窗口最小宽度是 1400（默认 1640），放在最右侧会被挤出可视范围。
 * 它改放在标题栏右上角（见 TitleBar.vue），那里始终有空位。
 */
<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";

const { t } = useI18n();

const {
  pickDirectory,
  loadedDirs,
  dirItems,
  isDirEnabled,
  toggleDir,
  removeDir,
  files,
  openBackups,
  setupOnOpen,
  openSetup,
  openUpdate,
} = useAppStore();
</script>

<template>
  <div class="toolbar">
    <a-button size="small" type="primary" @click="pickDirectory">
      {{ t("toolbar.addDir") }}
    </a-button>
    <!-- 已加载目录的下拉：查看当前加载了哪些、并逐个移除。
         不禁用：未加载时点开应看到「尚未加载任何目录」的说明，
         而不是一个点不动的按钮——那样用户不知道是坏了还是没数据。 -->
    <a-dropdown trigger="click">
      <a-button size="small">
        {{ t("toolbar.loadedDirs", { count: loadedDirs.length }) }}
        <icon-down />
      </a-button>
      <template #content>
        <a-doption v-if="loadedDirs.length === 0" disabled>
          {{ t("toolbar.noDirs") }}
        </a-doption>
        <a-doption v-for="d in loadedDirs" :key="d" :value="d">
          <div class="dir-item">
            <!-- 勾选才在下方列表显示 -->
            <a-checkbox
              :model-value="isDirEnabled(d)"
              @change="toggleDir(d, $event as boolean)"
              @click.stop
            />
            <span class="dir-item__name" :title="d">{{ d }}</span>
            <span class="dir-item__count">
              {{ t("toolbar.fileCount", { count: (dirItems[d] ?? []).length }) }}
            </span>
            <!-- 移除按钮：用 @click.stop 阻止冒泡，否则点它会同时选中该项、
                 让下拉以为是「选择目录」而不是「移除」 -->
            <a-button
              size="mini"
              type="text"
              status="danger"
              :title="t('toolbar.removeTitle')"
              @click.stop="removeDir(d)"
            >
              {{ t("toolbar.remove") }}
            </a-button>
          </div>
        </a-doption>
      </template>
    </a-dropdown>
    <a-button size="small" @click="openBackups">{{ t("toolbar.backups") }}</a-button>
    <a-button size="small" :loading="setupOnOpen" @click="openSetup">
      {{ t("toolbar.setup") }}
    </a-button>
    <a-button size="small" @click="openUpdate">{{ t("toolbar.update") }}</a-button>
    <!-- RMC投影站：按用户要求先只放按钮，点击不做任何事，
         这个入口预留给附加程序接入。
         TODO(附加程序接入后)：接上附加程序提供的入口。完成条件是该按钮能
         打开 RMC 投影站（或唤起附加程序）且失败时有明确提示；在此之前
         保持无行为，不显示任何「点不动的按钮」式的伪交互。
         跟踪方式：随附加程序的开发进度一并处理，仓库内暂无对应任务号。 -->
    <a-button size="small" :title="t('toolbar.rmcTitle')">{{ t("toolbar.rmc") }}</a-button>
    <span class="preview__spacer" />
    <span class="hint-dim toolbar__count">
      {{ t("toolbar.totalFiles", { total: files.length }) }}
    </span>
  </div>
</template>
