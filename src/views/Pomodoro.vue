<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Refresh, VideoPause, VideoPlay } from "@element-plus/icons-vue";
import { usePomodoroStore, type PomoMode } from "@/stores/pomodoro";

const { t } = useI18n();
const store = usePomodoroStore();
const modes: PomoMode[] = ["focus", "short", "long"];
</script>

<template>
  <div class="view-root">
    <div class="view-toolbar">
      <span class="toolbar-title">{{ t("pomodoro.title") }}</span>
      <span class="text-xs text-dp-tertiary">{{ t("pomodoro.focusCount", { n: store.focusDone }) }}</span>
    </div>
    <div class="flex-1 flex flex-col items-center justify-center gap-[18px]">
      <div class="flex gap-1.5">
        <button
          v-for="m in modes"
          :key="m"
          class="border bg-dp-card-bg text-dp-muted text-xs py-[5px] px-3.5 rounded-full cursor-pointer transition-all duration-150 ease-[ease]"
          :class="store.mode === m ? 'bg-dp-accent border-dp-accent text-white' : 'border-dp-card-border'"
          @click="store.setMode(m)"
        >
          {{ t(`pomodoro.mode.${m}`) }}
        </button>
      </div>
      <div
        class="text-[56px] font-bold tabular-nums tracking-[2px] leading-none"
        :class="store.running ? 'text-[#e67e22]' : 'text-dp-primary'"
      >
        {{ store.display }}
      </div>
      <div class="flex gap-2.5">
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
