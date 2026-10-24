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
  /** Settings 页初始/跳转的 tab（如启动检查到更新后跳到 about） */
  const settingsTab = ref("general");

  function toggleExpanded() {
    expanded.value = !expanded.value;
  }

  function expand() {
    expanded.value = true;
  }

  function collapse() {
    expanded.value = false;
  }

  function setActiveView(view: PanelView) {
    activeView.value = view;
  }

  return { expanded, activeView, settingsTab, toggleExpanded, expand, collapse, setActiveView };
});
