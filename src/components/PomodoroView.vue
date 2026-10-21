<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Refresh, VideoPause, VideoPlay } from "@element-plus/icons-vue";
import { usePomodoroStore, type PomoMode } from "@/stores/pomodoro";

const { t } = useI18n();
const store = usePomodoroStore();
const modes: PomoMode[] = ["focus", "short", "long"];
</script>

<template>
  <div class="pomo-view">
    <div class="view-toolbar">
      <span class="toolbar-title">{{ t("pomodoro.title") }}</span>
      <span class="pomo-count">{{ t("pomodoro.focusCount", { n: store.focusDone }) }}</span>
    </div>
    <div class="pomo-main">
      <div class="pomo-mode">
        <button
          v-for="m in modes"
          :key="m"
          class="mode-btn"
          :class="{ active: store.mode === m }"
          @click="store.setMode(m)"
        >
          {{ t(`pomodoro.mode.${m}`) }}
        </button>
      </div>
      <div class="pomo-time" :class="{ running: store.running }">{{ store.display }}</div>
      <div class="pomo-controls">
        <el-button
          v-if="!store.running"
          type="primary"
          round
          size="small"
          :icon="VideoPlay"
          @click="store.start()"
        >
          {{ t("pomodoro.start") }}
        </el-button>
        <el-button v-else type="warning" round size="small" :icon="VideoPause" @click="store.pause()">
          {{ t("pomodoro.pause") }}
        </el-button>
        <el-button round size="small" :icon="Refresh" @click="store.reset()">
          {{ t("pomodoro.reset") }}
        </el-button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.pomo-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.pomo-count {
  font-size: 12px;
  color: var(--dp-text-tertiary);
}

.pomo-main {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 18px;
}

.pomo-mode {
  display: flex;
  gap: 6px;
}

.mode-btn {
  border: 1px solid var(--dp-card-border);
  background: var(--dp-card-bg);
  color: var(--dp-text-muted);
  font-size: 12px;
  padding: 5px 14px;
  border-radius: 999px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.mode-btn.active {
  background: var(--dp-accent);
  border-color: var(--dp-accent);
  color: #ffffff;
}

.pomo-time {
  font-size: 56px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: var(--dp-text-primary);
  letter-spacing: 2px;
  line-height: 1;
}

.pomo-time.running {
  color: #e67e22;
}

.pomo-controls {
  display: flex;
  gap: 10px;
}
</style>
