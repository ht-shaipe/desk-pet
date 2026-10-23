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
    /* 窗口边缘缩放手柄：透明命中区，悬停仅变对应方向光标（不做颜色高亮，否则会误以为是一条 UI 边框） */
    "rs-hint": "fixed pointer-events-auto",
    /* AI 聊天 markdown 排版规范（v-html 生成，代码色用 color-mix(currentColor) 适配深浅主题） */
    "md-body": "min-w-0 [word-break:break-word] [&_p]:my-0 [&_p+p]:mt-[0.45em]"
      + "[&_h1]:text-[14.5px] [&_h1]:font-bold [&_h1]:mt-[0.7em] [&_h1]:mb-[0.3em] "
      + "[&_h2]:text-[14px] [&_h2]:font-bold [&_h2]:mt-[0.6em] [&_h2]:mb-[0.28em] "
      + "[&_h3]:text-[13.5px] [&_h3]:font-semibold [&_h3]:mt-[0.55em] [&_h3]:mb-[0.25em] "
      + "[&_h4]:text-[13px] [&_h4]:font-semibold [&_h4]:mt-[0.5em] [&_h4]:mb-[0.2em] "
      + "[&_h5]:text-[13px] [&_h5]:font-bold [&_h5]:italic [&_h5]:mt-[0.4em] [&_h5]:mb-[0.2em] "
      + "[&_h6]:text-[12.5px] [&_h6]:text-dp-secondary [&_h6]:mt-[0.4em] [&_h6]:mb-[0.2em] "
      + "[&_ul]:list-disc [&_ol]:list-decimal [&_ul]:pl-[1.3em] [&_ol]:pl-[1.3em] "
      + "[&_ul]:my-[0.35em] [&_ol]:my-[0.35em] [&_li+li]:mt-[2px] [&_li>ul]:mt-[2px] [&_li>ol]:mt-[2px] "
      + "[&_blockquote]:my-[0.45em] [&_blockquote]:pl-[9px] [&_blockquote]:border-l-[3px] [&_blockquote]:border-dp-divider "
      + "[&_blockquote]:text-dp-secondary [&_blockquote_p]:my-[0.2em] "
      + "[&_code]:rounded-[4px] [&_code]:px-[4px] [&_code]:py-[0.5px] [&_code]:font-mono [&_code]:text-[11.8px] "
      + "[&_code]:bg-[color-mix(in_srgb,currentColor_12%,transparent)] "
      + "[&_pre]:my-[0.5em] [&_pre]:p-[7px] [&_pre]:rounded-[8px] [&_pre]:overflow-x-auto "
      + "[&_pre]:border [&_pre]:border-dp-divider [&_pre]:bg-[color-mix(in_srgb,currentColor_8%,transparent)] "
      + "[&_pre_code]:px-0 [&_pre_code]:py-0 [&_pre_code]:bg-transparent [&_pre_code]:text-[11.2px] "
      + "[&_a]:text-dp-accent [&_a]:underline [&_a]:underline-offset-2 "
      + "[&_table]:my-[0.5em] [&_table]:w-full [&_table]:border-collapse [&_table]:text-[12px] "
      + "[&_th]:border [&_th]:border-dp-divider [&_th]:px-[6px] [&_th]:py-[3px] [&_th]:text-left "
      + "[&_td]:border [&_td]:border-dp-divider [&_td]:px-[6px] [&_td]:py-[3px] "
      + "[&_hr]:my-[0.7em] [&_hr]:border-dp-divider "
      + "[&_img]:max-w-full [&_img]:rounded-lg",
    /* 流式输出占位气泡：在最后一个块元素后追加闪烁光标 */
    "md-cursor":
      '[&>_:last-child]:after:content-["▍"] [&>_:last-child]:after:ml-[2px] '
      + "[&>_:last-child]:after:animate-blink [&>_:last-child]:after:text-dp-accent",
  },
});
