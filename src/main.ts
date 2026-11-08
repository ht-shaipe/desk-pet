import { createApp } from "vue";
import { createPinia } from "pinia";
import ElementPlus from "element-plus";
import { i18n } from "./i18n";
import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "virtual:uno.css";
import App from "./App.vue";
import { shiftToastsDown } from "./lib/toast";
import "./styles/main.css";

// 禁用 WebView 默认右键菜单（刷新、检查元素、系统编辑菜单等）
// 托盘图标的右键菜单由 Rust 侧提供，不受影响
document.addEventListener("contextmenu", (event) => event.preventDefault(), { capture: true });

// ElMessage 默认贴顶会压住宠物/标题区，统一注入下移偏移（详见 lib/toast.ts）
shiftToastsDown();

// 分析/更新/资料库/接口请求窗口复用主入口（index.html?view=analyzer|updater|images|docs|http），
// 条件挂载独立根组件，不初始化宠物/剪贴板/托盘逻辑；
// 也不新增 HTML 入口，避免 uno 与 EP 的 CSS 顺序被破坏
const view = new URLSearchParams(location.search).get("view");

async function bootstrap() {
  const Root =
    view === "analyzer"
      ? (await import("./AnalyzerApp.vue")).default
      : view === "updater"
        ? (await import("./UpdaterWindow.vue")).default
        : view === "images"
          ? (await import("./ImagesApp.vue")).default
          : view === "docs"
            ? (await import("./DocsApp.vue")).default
            : view === "http"
              ? (await import("./HttpApp.vue")).default
              : view === "monitorbar"
                ? (await import("./FloatingMonitorApp.vue")).default
                : App;
  createApp(Root).use(createPinia()).use(i18n).use(ElementPlus).mount("#app");
}

void bootstrap();
