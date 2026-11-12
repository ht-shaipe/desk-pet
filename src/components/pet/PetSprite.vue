<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useUiStore } from "@/stores/ui";
import { useConfigStore } from "@/stores/config";
import { usePomodoroStore } from "@/stores/pomodoro";
import { useAiUsageStore, formatCompactTokens } from "@/stores/aiUsage";
import { usePetDrag } from "@/composables/usePetDrag";
import { petWrapSize, spriteSize as normalizeSpriteSize } from "@/lib/windowCtl";

const ui = useUiStore();
const config = useConfigStore();
const pomo = usePomodoroStore();
const aiUsage = useAiUsageStore();

const dragHandlers = usePetDrag({
  onClick: () => ui.toggleExpanded(),
});

const rootClass = computed(() => [
  config.windowMoving ? "dragging cursor-grabbing" : "",
  ui.expanded ? "translate-y-4 scale-105 origin-top-left" : "",
]);

const spriteSize = computed(() => normalizeSpriteSize(config.settings.sprite_size));

/** 拖拽命中区/呼吸容器边长：精灵 ≤64 保持既有 68px 手感，更大时跟随放大。 */
const wrapSize = computed(() => petWrapSize(spriteSize.value));

const avatarSrc = computed(() => {
  const p = config.settings.pet_avatar;
  return p ? convertFileSrc(p) : "/images/avatar.png";
});

/** 收起面板且番茄钟计时中时，在精灵上显示倒计时 */
const showCountdown = computed(() => pomo.running && !ui.expanded);

/** 收起面板且开启时钟时显示当前时间；与倒计时同槽位，计时中让位给倒计时 */
const showClock = computed(() => config.settings.pet_clock === true && !ui.expanded && !showCountdown.value);

/** 收起面板且开启徽标时显示今日 AI token 总量；最低优先级，仅当前两个槽位都空时显示 */
const showAiUsage = computed(
  () =>
    config.settings.pet_ai_usage === true &&
    !ui.expanded &&
    !showCountdown.value &&
    !showClock.value &&
    aiUsage.todayLoaded &&
    aiUsage.todayTokens > 0,
);

const clockText = ref("");

function tickClock() {
  const d = new Date();
  const text = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  if (text !== clockText.value) {
    clockText.value = text;
  }
}

let clockTimer: ReturnType<typeof setInterval> | undefined;

onMounted(() => {
  tickClock();
  clockTimer = setInterval(tickClock, 1000);
});

// 徽标开启时才拉取今日用量（扫描 + 查询 + 监听后台广播）；运行时切换开关即时生效
watch(
  () => config.settings.pet_ai_usage === true,
  (on) => {
    if (on) void aiUsage.init();
  },
  { immediate: true },
);

onUnmounted(() => {
  if (clockTimer !== undefined) {
    clearInterval(clockTimer);
  }
});
</script>

<template>
  <div
    class="pet-wrap absolute top-0 left-0 flex items-center justify-center cursor-grab z-20 touch-none transition-transform duration-200 ease-[ease]"
    :class="rootClass"
    :style="{ width: `${wrapSize}px`, height: `${wrapSize}px` }"
    v-bind="dragHandlers"
  >
    <img
      class="w-12 h-12 shrink-0 object-cover rounded-full shadow-[0_2px_6px_rgba(31,41,55,0.06)] transition-[transform,box-shadow] duration-250 ease-[ease] animate-pet-breathe select-none pointer-events-none [-webkit-user-drag:none] [.pet-wrap:hover_&]:shadow-[0_4px_10px_rgba(31,41,55,0.08)] [.pet-wrap.dragging_&]:animate-none [.pet-wrap.dragging_&]:scale-[0.94]"
      :style="{ width: `${spriteSize}px`, height: `${spriteSize}px` }"
      :src="avatarSrc"
      alt="pet"
      draggable="false"
    />
    <div
      v-if="showCountdown"
      class="absolute bottom-0 left-1/2 -translate-x-1/2 py-px px-1.5 bg-[rgba(31,41,55,0.8)] text-white text-[10px] font-semibold font-mono leading-[1.5] tracking-[0.5px] rounded-full shadow-[0_1px_4px_rgba(0,0,0,0.28)] pointer-events-none z-[21] select-none"
    >
      {{ pomo.display }}
    </div>
    <div
      v-else-if="showClock"
      class="absolute bottom-0 left-1/2 -translate-x-1/2 py-0 px-1 bg-[rgba(31,41,55,0.6)] text-white text-[11px] font-semibold font-mono leading-[1.5] tracking-[0.5px] rounded-full shadow-[0_1px_3px_rgba(0,0,0,0.16)] pointer-events-none z-[21] select-none"
    >
      {{ clockText }}
    </div>
    <div
      v-else-if="showAiUsage"
      class="absolute bottom-0 left-1/2 -translate-x-1/2 py-px px-1.5 bg-[rgba(31,41,55,0.8)] text-white text-[10px] font-semibold font-mono leading-[1.5] tracking-[0.5px] rounded-full shadow-[0_1px_4px_rgba(0,0,0,0.28)] pointer-events-none z-[21] select-none"
    >
      AI {{ formatCompactTokens(aiUsage.todayTokens) }}
    </div>
  </div>
</template>

