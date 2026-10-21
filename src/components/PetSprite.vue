<script setup lang="ts">
import { computed } from "vue";
import { useUiStore } from "@/stores/ui";
import { useConfigStore } from "@/stores/config";
import { usePetDrag } from "@/utils/drag";

const ui = useUiStore();
const config = useConfigStore();

const dragHandlers = usePetDrag({
  onClick: () => ui.toggleExpanded(),
});

const rootClass = computed(() => ({
  dragging: config.windowMoving,
  expanded: ui.expanded,
}));
</script>

<template>
  <div class="pet-wrap" :class="rootClass" v-bind="dragHandlers">
    <div class="pet-body">
      <div class="pet-eye left"></div>
      <div class="pet-eye right"></div>
      <div class="pet-mouth"></div>
      <div class="pet-cheek left"></div>
      <div class="pet-cheek right"></div>
    </div>
  </div>
</template>

<style scoped>
.pet-wrap {
  position: absolute;
  top: 0;
  left: 0;
  width: 120px;
  height: 120px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: grab;
  z-index: 20;
  touch-action: none;
  transition: transform 0.2s ease;
}

.pet-wrap.expanded {
  transform: scale(0.82);
  transform-origin: top left;
}

.pet-wrap.dragging {
  cursor: grabbing;
}

.pet-body {
  position: relative;
  width: 92px;
  height: 92px;
  border-radius: 50%;
  background: radial-gradient(circle at 34% 28%, #ffe9a8, #ffc751 52%, #f5a623 100%);
  box-shadow:
    0 10px 24px rgba(245, 166, 35, 0.35),
    inset 0 -6px 12px rgba(217, 119, 6, 0.25);
  transition: transform 0.25s ease, box-shadow 0.25s ease;
  animation: pet-breathe 3.2s ease-in-out infinite;
}

.pet-wrap:hover .pet-body {
  box-shadow:
    0 14px 30px rgba(245, 166, 35, 0.5),
    inset 0 -6px 12px rgba(217, 119, 6, 0.25);
}

.pet-wrap.dragging .pet-body {
  animation: none;
  transform: scale(0.94) rotate(-8deg);
}

.pet-eye {
  position: absolute;
  top: 34px;
  width: 12px;
  height: 14px;
  background: #4a2c0a;
  border-radius: 50%;
  animation: pet-blink 4.6s infinite;
}

.pet-eye.left {
  left: 26px;
}

.pet-eye.right {
  right: 26px;
}

.pet-mouth {
  position: absolute;
  top: 52px;
  left: 50%;
  width: 18px;
  height: 9px;
  margin-left: -9px;
  border-bottom: 3px solid #4a2c0a;
  border-radius: 0 0 18px 18px;
}

.pet-cheek {
  position: absolute;
  top: 46px;
  width: 10px;
  height: 6px;
  background: rgba(255, 120, 120, 0.45);
  border-radius: 50%;
}

.pet-cheek.left {
  left: 12px;
}

.pet-cheek.right {
  right: 12px;
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

@keyframes pet-blink {
  0%,
  91%,
  100% {
    transform: scaleY(1);
  }
  94% {
    transform: scaleY(0.12);
  }
}
</style>
