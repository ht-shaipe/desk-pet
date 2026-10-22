<script setup lang="ts">
import { computed } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useUiStore } from "@/stores/ui";
import { useConfigStore } from "@/stores/config";
import { usePomodoroStore } from "@/stores/pomodoro";
import { usePetDrag } from "@/composables/usePetDrag";

const ui = useUiStore();
const config = useConfigStore();
const pomo = usePomodoroStore();

const dragHandlers = usePetDrag({
  onClick: () => ui.toggleExpanded(),
});

const rootClass = computed(() => [
  config.windowMoving ? "dragging cursor-grabbing" : "",
  ui.expanded ? "translate-y-2.5 scale-105 origin-top-left" : "",
]);

const spriteSize = computed(() => {
  const size = config.settings.sprite_size;
  return typeof size === "number" && size >= 32 && size <= 64 ? size : 48;
});

const avatarSrc = computed(() => {
  const p = config.settings.pet_avatar;
  return p ? convertFileSrc(p) : "/images/avatar.png";
});

/** 收起面板且番茄钟计时中时，在精灵上显示倒计时 */
const showCountdown = computed(() => pomo.running && !ui.expanded);
</script>

<template>
  <div
    class="pet-wrap absolute top-0 left-0 w-[68px] h-[68px] flex items-center justify-center cursor-grab z-20 touch-none transition-transform duration-200 ease-[ease]"
    :class="rootClass"
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
      class="absolute bottom-1 left-1/2 -translate-x-1/2 py-px px-1.5 bg-[rgba(31,41,55,0.8)] text-white text-[10px] font-semibold font-mono leading-[1.5] tracking-[0.5px] rounded-full shadow-[0_1px_4px_rgba(0,0,0,0.28)] pointer-events-none z-[21] select-none"
    >
      {{ pomo.display }}
    </div>
  </div>
</template>

