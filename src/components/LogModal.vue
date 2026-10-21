/**
 * 日志管理窗口：查看全局运行日志，切换记录等级，并提供清空与定位日志文件两个管理动作。
 *
 * 与「备份与日志」里的运行日志页签是同一份数据（store/backups.ts 的 logs 与
 * logLevel），这里只是把它做成可以单独打开的窗口——入口就在那个页签里，
 * 便于在窗口里完成改等级、清空与定位日志文件。
 *
 * 清空是不可撤销的，因此走 a-popconfirm 二次确认；日志文件本身保留
 * （见后端 clear_run_log），所以「打开日志目录」在清空后仍然可用。
 */
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { runLogDir } from "../api";
import { useAppStore } from "../store";
import { isLogLevel, LOG_LEVEL_KEYS, LOG_LEVELS } from "../utils/run_log";
import RunLogList from "./RunLogList.vue";

const { t } = useI18n();

const {
  logOpen,
  logs,
  logLoading,
  logLevel,
  logMaxLines,
  refreshLogs,
  clearLogs,
  openLogDir,
  setLogLevel,
} = useAppStore();

/** 日志所在目录，由后端算（run_log_dir）。界面自己拼应用数据目录就成了
 *  两处规则：这个项目的 identifier 改过一次，拼出来的位置会指向不存在的地方。
 *  取不到时保持 null，说明文案里就不显示位置——不显示猜的路径。 */
const logDir = ref<string | null>(null);

onMounted(async () => {
  try {
    logDir.value = await runLogDir();
  } catch (e) {
    // run_log_dir 的失败已由 logged 出口记进运行日志，这里只留控制台一份；
    // 路径显示不出来不影响看日志
    console.warn("读取运行日志目录失败：", e);
  }
});

const levelOptions = computed(() =>
  LOG_LEVELS.map((level) => ({ value: level, label: t(LOG_LEVEL_KEYS[level]) })),
);

/** 选择器只该给出约定里的四档；这里把 Arco 的宽松取值收窄后再交给 store */
function onLevelChange(value: unknown) {
  if (isLogLevel(value)) setLogLevel(value);
}
</script>

<template>
  <a-modal
    v-model:visible="logOpen"
    :title="t('log.title')"
    :width="760"
    :footer="false"
    :body-style="{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }"
    unmount-on-close
  >
    <!-- 说清楚每条日志的等级含义、文件什么时候滚动、日志在哪：
         用户要照着这段话判断「去哪找完整日志」和「该开哪个等级」。 -->
    <div class="hint-dim" style="margin-bottom: 8px">
      <div>{{ t("log.hint", { count: logMaxLines }) }}</div>
      <div v-if="logDir">{{ t("log.dir", { dir: logDir }) }}</div>
    </div>

    <a-space style="margin-bottom: 8px">
      <span class="hint-dim">{{ t("log.level") }}</span>
      <a-select
        :model-value="logLevel"
        :options="levelOptions"
        size="small"
        style="width: 100px"
        @change="onLevelChange"
      />
      <a-button size="small" :loading="logLoading" @click="refreshLogs">
        {{ t("log.refresh") }}
      </a-button>
      <a-button size="small" :title="t('log.openDirTitle')" @click="openLogDir">
        {{ t("log.openDir") }}
      </a-button>
      <a-popconfirm
        :content="t('log.clearConfirm')"
        :ok-text="t('log.clearOk')"
        @ok="clearLogs"
      >
        <a-button size="small" status="danger" :disabled="logs.length === 0">
          {{ t("log.clear") }}
        </a-button>
      </a-popconfirm>
      <span class="hint-dim">{{ t("log.count", { count: logs.length }) }}</span>
    </a-space>

    <!-- 日志可以很长：内容区自己滚（Arco 无轨道滚动条），标题与按钮留在视野里 -->
    <a-scrollbar type="embed" class="scroll-fill__inner" outer-class="scroll-fill">
      <a-spin :loading="logLoading" style="width: 100%">
        <a-empty v-if="logs.length === 0" :description="t('log.empty')" />
        <RunLogList v-else :lines="logs" />
      </a-spin>
    </a-scrollbar>
  </a-modal>
</template>
