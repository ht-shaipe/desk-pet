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

createApp(App).use(createPinia()).use(i18n).use(ElementPlus).mount("#app");
