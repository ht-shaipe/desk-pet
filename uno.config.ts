import {
  defineConfig,
  presetWind3,
  transformerDirectives,
  transformerVariantGroup,
} from "unocss";

// --dp-* 主题变量映射为命名 token：text-dp-primary / bg-dp-card-bg / border-dp-card-border / shadow-dp-card
const dpColors: Record<string, string> = {
  primary: "var(--dp-text-primary)",
  secondary: "var(--dp-text-secondary)",
  muted: "var(--dp-text-muted)",
  tertiary: "var(--dp-text-tertiary)",
  accent: "var(--dp-accent)",
  "accent-deep": "var(--dp-accent-deep)",
  "panel-bg": "var(--dp-panel-bg)",
  "panel-border": "var(--dp-panel-border)",
  "card-bg": "var(--dp-card-bg)",
  "card-border": "var(--dp-card-border)",
  "card-hover-border": "var(--dp-card-hover-border)",
  "hover-bg": "var(--dp-hover-bg)",
  "btn-bg": "var(--dp-btn-bg)",
  "btn-hover-bg": "var(--dp-btn-hover-bg)",
  divider: "var(--dp-divider)",
  "tabbar-bg": "var(--dp-tabbar-bg)",
  "tabbar-border": "var(--dp-tabbar-border)",
  "tab-hover-bg": "var(--dp-tab-hover-bg)",
  "tag-soft": "var(--dp-tag-soft)",
  "danger-soft": "var(--dp-danger-soft)",
  "note-input-bg": "var(--dp-note-input-bg)",
  "dot-border": "var(--dp-dot-border)",
};

export default defineConfig({
  presets: [presetWind3()],
  transformers: [transformerDirectives(), transformerVariantGroup()],
  theme: {
    colors: {
      dp: dpColors,
    },
    boxShadow: {
      "dp-panel": "var(--dp-panel-shadow)",
      "dp-card": "var(--dp-card-shadow)",
      "dp-card-hover": "var(--dp-card-hover-shadow)",
    },
    animation: {
      durations: {
        "panel-in": "0.18s",
        "pip-pulse": "1.8s",
        "pet-breathe": "3.2s",
        blink: "0.9s",
        "cleanup-slide": "1.1s",
      },
      timingFns: {
        "panel-in": "ease-out",
        "pip-pulse": "ease-in-out",
        "pet-breathe": "ease-in-out",
        blink: "step-end",
        "cleanup-slide": "ease-in-out",
      },
      counts: {
        "pip-pulse": "infinite",
        "pet-breathe": "infinite",
        blink: "infinite",
        "cleanup-slide": "infinite",
      },
      keyframes: {
        "panel-in": "{from{opacity:0;transform:translateY(10px)}to{opacity:1;transform:none}}",
        "pip-pulse": "{0%,100%{opacity:1}50%{opacity:0.4}}",
        "pet-breathe": "{0%,100%{transform:translateY(0) scale(1)}50%{transform:translateY(-3px) scale(1.03)}}",
        blink: "{50%{opacity:0}}",
        "cleanup-slide": "{from{transform:translateX(-110%)}to{transform:translateX(340%)}}",
      },
    },
  },
  shortcuts: {
    "view-root": "flex flex-col h-full min-h-0",
    "view-toolbar": "flex items-center gap-2 min-h-9 shrink-0",
    "toolbar-title": "text-[13px] font-semibold text-dp-secondary mr-auto",
    "view-scroll":
      "flex-1 min-h-0 [&_.el-scrollbar\\_\\_thumb]:bg-[color-mix(in_srgb,var(--dp-text-tertiary)_55%,transparent)] [&_.el-scrollbar\\_\\_thumb]:opacity-60 [&_.el-scrollbar\\_\\_thumb:hover]:bg-dp-accent [&_.el-scrollbar\\_\\_thumb:hover]:opacity-75",
    /* 窗口边缘缩放手柄：透明命中区，悬停高亮提示可拖拽 */
    "rs-hint":
      "fixed pointer-events-auto transition-colors duration-150 ease-[ease] hover:bg-[color-mix(in_srgb,var(--dp-accent)_45%,transparent)]",
  },
});
