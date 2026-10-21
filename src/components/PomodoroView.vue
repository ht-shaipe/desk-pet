<script setup lang="ts">
import { Refresh, VideoPause, VideoPlay } from "@element-plus/icons-vue";
import { MODE_LABELS, usePomodoroStore, type PomoMode } from "@/stores/pomodoro";

const store = usePomodoroStore();
const modes: PomoMode[] = ["focus", "short", "long"];
</script>

<template>
  <div class="pomo-view">
    <div class="view-toolbar">
      <span class="toolbar-title">番茄钟</span>
      <span class="pomo-count">已完成专注 {{ store.focusDone }} 次</span>
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
          {{ MODE_LABELS[m] }}
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
          开始
        </el-button>
        <el-button v-else type="warning" round size="small" :icon="VideoPause" @click="store.pause()">
          暂停
        </el-button>
        <el-button round size="small" :icon="Refresh" @click="store.reset()">重置</el-button>
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
  color: #9ca3af;
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
  border: 1px solid #e0e3e8;
  background: #ffffff;
  color: #6b7280;
  font-size: 12px;
  padding: 5px 14px;
  border-radius: 999px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.mode-btn.active {
  background: #409eff;
  border-color: #409eff;
  color: #ffffff;
}

.pomo-time {
  font-size: 56px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: #1f2937;
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
