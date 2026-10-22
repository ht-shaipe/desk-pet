import { defineStore } from "pinia";
import { ref } from "vue";

export type PanelView =
  | "launcher"
  | "notes"
  | "pomodoro"
  | "clipboard"
  | "toolbox"
  | "chat"
  | "quick"
  | "account"
  | "settings";

export const useUiStore = defineStore("ui", () => {
  const expanded = ref(false);
  const activeView = ref<PanelView>("chat");

  function toggleExpanded() {
    expanded.value = !expanded.value;
  }

  function collapse() {
    expanded.value = false;
  }

  function setActiveView(view: PanelView) {
    activeView.value = view;
  }

  return { expanded, activeView, toggleExpanded, collapse, setActiveView };
});
