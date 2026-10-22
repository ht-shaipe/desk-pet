import type { Component } from "vue";
import { defineAsyncComponent } from "vue";
import { Crop, Delete, Lock, Postcard, ScaleToOriginal } from "@element-plus/icons-vue";

/** 工具箱内一个工具的注册信息。 */
export interface ToolboxTool {
  /** 唯一 ID，同时用作组件渲染 key */
  id: string;
  /** i18n 键：工具名称（首页卡片与详情页标题共用） */
  titleKey: string;
  /** i18n 键：工具简介（首页卡片副文案） */
  descKey: string;
  /** 首页卡片图标 */
  icon: Component;
  /** 工具详情组件（懒加载，进入详情时才载入） */
  component: Component;
}

/**
 * 工具注册表。新增工具：
 * 1. 在 tools/ 下新建组件（根节点撑满高度，滚动区自行管理）；
 * 2. 在下方数组追加一项；
 * 3. i18n 文案放到 `toolbox.tools.<id>` 命名空间（zh-CN / en 同步）。
 */
export const TOOLBOX_TOOLS: ToolboxTool[] = [
  {
    id: "qr",
    titleKey: "toolbox.tools.qr.title",
    descKey: "toolbox.tools.qr.desc",
    icon: Postcard,
    component: defineAsyncComponent(() => import("./tools/QrTool.vue")),
  },
  {
    id: "base64",
    titleKey: "toolbox.tools.base64.title",
    descKey: "toolbox.tools.base64.desc",
    icon: ScaleToOriginal,
    component: defineAsyncComponent(() => import("./tools/Base64Tool.vue")),
  },
  {
    id: "des",
    titleKey: "toolbox.tools.des.title",
    descKey: "toolbox.tools.des.desc",
    icon: Lock,
    component: defineAsyncComponent(() => import("./tools/DesTool.vue")),
  },
  {
    id: "imgcompress",
    titleKey: "toolbox.tools.imgcompress.title",
    descKey: "toolbox.tools.imgcompress.desc",
    icon: Crop,
    component: defineAsyncComponent(() => import("./tools/ImageCompressTool.vue")),
  },
  {
    id: "cleanup",
    titleKey: "toolbox.tools.cleanup.title",
    descKey: "toolbox.tools.cleanup.desc",
    icon: Delete,
    component: defineAsyncComponent(() => import("./tools/CleanupTool.vue")),
  },
];
