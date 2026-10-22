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

const rootClass = computed(() => ({
  dragging: config.windowMoving,
  expanded: ui.expanded,
}));

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
  <div class="pet-wrap" :class="rootClass" v-bind="dragHandlers">
    <img
      class="pet-img"
      :style="{ width: `${spriteSize}px`, height: `${spriteSize}px` }"
      :src="avatarSrc"
      alt="pet"
      draggable="false"
    />
    <div v-if="showCountdown" class="pomo-badge">{{ pomo.display }}</div>
  </div>
</template>

<style scoped>
.pet-wrap {
  position: absolute;
  top: 0;
  left: 0;
  width: 68px;
  height: 68px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: grab;
  z-index: 20;
  touch-action: none;
  transition: transform 0.2s ease;
}

.pet-wrap.expanded {
  transform: translateY(10px) scale(1.05);
  transform-origin: top left;
}

.pet-wrap.dragging {
  cursor: grabbing;
}

.pet-img {
  width: 48px;
  height: 48px;
  flex-shrink: 0;
  object-fit: cover;
  border-radius: 50%;
  box-shadow: 0 2px 6px rgba(31, 41, 55, 0.12);
  transition: transform 0.25s ease, box-shadow 0.25s ease;
  animation: pet-breathe 3.2s ease-in-out infinite;
  user-select: none;
  -webkit-user-drag: none;
  pointer-events: none;
}

.pet-wrap:hover .pet-img {
  box-shadow: 0 4px 10px rgba(31, 41, 55, 0.16);
}

.pet-wrap.dragging .pet-img {
  animation: none;
  transform: scale(0.94);
}

@keyframes pet-breathe {
  0%,
  100% {
    transform: translateY(0) scale(1);
  }
  50% {
    transform: translateY(-3px) scale(1.03);
  }
}

.pomo-badge {
  position: absolute;
  bottom: 4px;
  left: 50%;
  transform: translateX(-50%);
  padding: 1px 6px;
  background: rgba(31, 41, 55, 0.8);
  color: #ffffff;
  font-size: 10px;
  font-weight: 600;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  line-height: 1.5;
  letter-spacing: 0.5px;
  border-radius: 999px;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.28);
  pointer-events: none;
  z-index: 21;
  user-select: none;
}

</style>
