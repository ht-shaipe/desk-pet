import { ElMessage } from "element-plus";
import type { MessageOptions, MessageParams, MessageTypedFn } from "element-plus";
import { isVNode } from "vue";

/**
 * 系统提示统一下移：ElMessage 默认距窗口顶部 16px，在面板里正好压在
 * 宠物图标/标题区上，看不全。这里给每类调用注入更大的默认 offset，
 * 调用点显式传过 offset 时不覆盖。需在 element-plus 引入后执行一次。
 */
const TOAST_OFFSET = 60;

/** string / VNode / 渲染函数 → 规整为 options 并带上默认 offset；对象参数只补缺失的 offset。 */
function withOffset(params: MessageParams | undefined): Omit<MessageOptions, "type"> {
  if (params === undefined) {
    return { offset: TOAST_OFFSET };
  }
  if (isVNode(params) || typeof params === "string" || typeof params === "function") {
    return { message: params, offset: TOAST_OFFSET };
  }
  return { ...params, offset: params.offset ?? TOAST_OFFSET };
}

/** 给 ElMessage 的所有类型化入口注入默认下移偏移（模块单例，全项目调用点同时生效）。 */
export function shiftToastsDown(): void {
  const keys = ["primary", "success", "warning", "info", "error"] as const;
  for (const key of keys) {
    const raw: MessageTypedFn = ElMessage[key];
    const shifted: MessageTypedFn = (options, appContext) =>
      raw(withOffset(options), appContext);
    ElMessage[key] = shifted;
  }
}
