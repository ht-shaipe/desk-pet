<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Refresh, Setting, VideoPause, VideoPlay } from "@element-plus/icons-vue";
import { usePomodoroStore, type PomoMode } from "@/stores/pomodoro";

const { t } = useI18n();
const store = usePomodoroStore();
const modes: PomoMode[] = ["focus", "short", "long"];

function onMinutesChange(m: PomoMode, v: number | undefined) {
  if (typeof v === "number" && v > 0) {
    store.setMinutes(m, v);
  }
}
</script>

<template>
  <div class="view-root">
    <div class="view-toolbar">
      <span class="toolbar-title">{{ t("pomodoro.title") }}</span>
      <span class="text-xs text-dp-tertiary">{{ t("pomodoro.focusCount", { n: store.focusDone }) }}</span>
    </div>
    <div class="flex-1 flex flex-col items-center justify-center gap-[18px]">
      <div class="flex gap-1.5 items-center">
        <button
          v-for="m in modes"
          :key="m"
          class="border bg-dp-card-bg text-dp-muted text-xs py-[5px] px-3.5 rounded-full cursor-pointer transition-all duration-150 ease-[ease]"
          :class="store.mode === m ? 'bg-dp-accent border-dp-accent text-white' : 'border-dp-card-border'"
          @click="store.setMode(m)"
        >
          {{ t(`pomodoro.mode.${m}`) }}
        </button>
        <el-popover placement="bottom" :width="210" trigger="click">
          <template #reference>
            <button
              class="border border-dp-card-border bg-dp-card-bg text-dp-muted w-[26px] h-[26px] rounded-full flex items-center justify-center cursor-pointer transition-all duration-150 ease-[ease] hover:text-dp-primary"
            >
              <el-icon :size="12"><Setting /></el-icon>
            </button>
          </template>
          <div class="flex flex-col gap-2.5">
            <div v-for="m in modes" :key="m" class="flex items-center justify-between gap-2">
              <span class="text-xs text-dp-muted">{{ t(`pomodoro.mode.${m}`) }}</span>
              <el-input-number
                :model-value="store.minutes[m]"
                :min="1"
                :max="120"
                size="small"
                controls-position="right"
                class="w-[110px]"
                @change="(v: number | undefined) => onMinutesChange(m, v)"
              />
            </div>
          </div>
        </el-popover>
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
