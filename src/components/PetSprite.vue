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
    <img class="pet-img" src="/images/avatar.png" alt="pet" draggable="false" />
  </div>
</template>

<style scoped>
.pet-wrap {
  position: absolute;
  top: 0;
  left: 0;
  width: 80px;
  height: 80px;
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

.pet-img {
  width: 62px;
  height: 62px;
  object-fit: cover;
  border-radius: 50%;
  box-shadow: 0 0 0 2px rgba(255, 255, 255, 0.85);
  filter: drop-shadow(0 6px 14px rgba(31, 41, 55, 0.3));
  transition: transform 0.25s ease, filter 0.25s ease;
  animation: pet-breathe 3.2s ease-in-out infinite;
  user-select: none;
  -webkit-user-drag: none;
  pointer-events: none;
}

.pet-wrap:hover .pet-img {
  filter: drop-shadow(0 12px 22px rgba(31, 41, 55, 0.4));
}

.pet-wrap.dragging .pet-img {
  animation: none;
  transform: scale(0.94) rotate(-8deg);
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
</style>
