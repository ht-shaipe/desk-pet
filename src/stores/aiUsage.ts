import { defineStore } from "pinia";
import { ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { aiUsageQuery, aiUsageRefresh } from "@/api/aiUsage";

/** token 数简写：1.2K / 3.4M / 5.6B，一位小数 */
export function formatCompactTokens(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}B`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

/** 本地时区今日 00:00 的时间戳（毫秒） */
export function localDayStartMs(d = new Date()): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

export const useAiUsageStore = defineStore("aiUsage", () => {
  /** 今日 token 总量（input+output+cache 读+写），用于宠物徽标 */
  const todayTokens = ref(0);
  /** 今日用量是否已成功加载过（失败时徽标隐藏） */
  const todayLoaded = ref(false);

  let seq = 0;
  let initPromise: Promise<void> | undefined;

  /** 拉取今日用量并更新徽标数据 */
  async function refreshToday(): Promise<void> {
    const cur = ++seq;
    const from = localDayStartMs();
    const to = from + 24 * 3600 * 1000;
    try {
      const res = await aiUsageQuery(from, to);
      if (cur !== seq) return;
      const t = res.totals;
      todayTokens.value = t.input + t.output + t.cache_read + t.cache_write;
      todayLoaded.value = true;
    } catch {
      if (cur === seq) todayLoaded.value = false;
    }
  }

  async function doInit(): Promise<void> {
    try {
      await aiUsageRefresh();
    } catch {
      // 扫描失败（无日志目录等）也继续查询，空态由查询结果表达
    }
    await refreshToday();
    // 监听随 app 生命周期存续（store 为应用级单例），无需保存句柄
    await listen<number>("ai_usage://updated", () => {
      void refreshToday();
    });
  }

  /** 首次加载：扫一遍日志再查询，并监听后台扫描广播；并发调用共享同一次初始化。 */
  function init(): Promise<void> {
    if (!initPromise) initPromise = doInit();
    return initPromise;
  }

  return { todayTokens, todayLoaded, refreshToday, init };
});
