import { defineStore } from "pinia";
import { ref } from "vue";

export type PanelView =
  | "launcher"
  | "pomodoro"
  | "clipboard"
  | "stash"
  | "toolbox"
  | "chat"
  | "account"
  | "settings";

export const useUiStore = defineStore("ui", () => {
  const expanded = ref(false);
  const activeView = ref<PanelView>("chat");
  /** Settings 页初始/跳转的 tab（如启动检查到更新后跳到 about） */
  const settingsTab = ref("general");
  /** 待打开的工具箱工具 id（AI open_tool 深链用，Toolbox 消费后清空） */
  const toolboxTool = ref<string | null>(null);

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

  /** 深链到任意界面：展开面板 -> 切换视图（AI open_view 用）。 */
  function openView(view: PanelView) {
    activeView.value = view;
    expanded.value = true;
  }

  /** 深链到工具箱某个工具（展开面板 -> 切到工具箱 -> 打开指定工具详情）。 */
  function openToolboxTool(id: string) {
    toolboxTool.value = id;
    activeView.value = "toolbox";
    expanded.value = true;
  }

  return {
    expanded,
    activeView,
    settingsTab,
    toolboxTool,
    toggleExpanded,
    expand,
    collapse,
    setActiveView,
    openView,
    openToolboxTool,
  };
});
