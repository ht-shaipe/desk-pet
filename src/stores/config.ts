import { acceptHMRUpdate, defineStore } from "pinia";
import { ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { getPlatform, setShowDock as invokeSetShowDock } from "@/api/system";
import { getSettings, saveSettings, setPetAvatar, resetPetAvatar } from "@/api/settings";
import {
  monitorbarToggle,
  monitorMenubarSet,
  monitorSample,
  type MonitorChipSections,
} from "@/api/monitor";
import type { AiModel, AppSettings, McpServerConfig } from "@/types";
import { detectLocale, setAppLocale, type AppLocale } from "@/i18n";
import { initWindowPosition } from "@/lib/windowCtl";
import { applyAccentColor, applyTheme, normalizeTheme } from "@/lib/theme";
import { useUiStore } from "./ui";

const win = getCurrentWindow();

/** 归一化持久化的语言值，非法值回退为系统语言检测。 */
function normalizeLocale(value: string | null): AppLocale {
  if (value === "zh-CN" || value === "en") {
    return value;
  }
  return detectLocale();
}

/** 归一化精灵大小：钳制到 32-96（折叠窗口随尺寸同步放大，见 windowCtl.petWindowSize）。 */
function clampSpriteSize(value: number | null | undefined): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return 48;
  }
  return Math.min(96, Math.max(32, Math.round(value)));
}

/** 校验主题色格式（#rgb / #rrggbb），非法返回 null。 */
function normalizeAccentColor(value: string | null | undefined): string | null {
  if (typeof value !== "string") {
    return null;
  }
  return /^#([0-9a-fA-F]{6}|[0-9a-fA-F]{3})$/.test(value.trim()) ? value.trim() : null;
}

/** 归一化应用名：去空白、限长 24，空串视为 null（用默认名）。 */
export const DEFAULT_APP_NAME = "Desk Pet";

function normalizeAppName(value: string | null | undefined): string {
  if (typeof value !== "string") {
    return DEFAULT_APP_NAME;
  }
  const trimmed = value.trim().slice(0, 24);
  return trimmed.length > 0 ? trimmed : DEFAULT_APP_NAME;
}

/** 归一化番茄钟时长：钳制到 1-120 分钟。 */
function clampPomoMin(value: number | null | undefined): number | null {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return null;
  }
  return Math.min(120, Math.max(1, Math.round(value)));
}

/** 归一化监控刷新间隔：白名单 1000/2000/5000，非法回退 2000。 */
function normalizeRefreshMs(value: number | null | undefined): number {
  return value === 1000 || value === 2000 || value === 5000 ? value : 2000;
}

/** 生成模型配置 id（时间戳 + 随机后缀，足够本地唯一）。 */
function genAiModelId(): string {
  return `m-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/** 归一化模型配置列表：剔除 id/必要字段缺失的脏数据。 */
function normalizeAiModels(value: unknown): AiModel[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const models: AiModel[] = [];
  for (const item of value) {
    if (typeof item !== "object" || item === null) {
      continue;
    }
    const raw = item as Record<string, unknown>;
    if (typeof raw.id !== "string" || !raw.id) {
      continue;
    }
    models.push({
      id: raw.id,
      name: typeof raw.name === "string" ? raw.name : "",
      base_url: typeof raw.base_url === "string" ? raw.base_url : "",
      api_key: typeof raw.api_key === "string" ? raw.api_key : "",
      model: typeof raw.model === "string" ? raw.model : "",
    });
  }
  return models;
}

/** 生成 MCP 服务器配置 id（时间戳 + 随机后缀，足够本地唯一）。 */
function genMcpServerId(): string {
  return `s-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/** 归一化 MCP 服务器配置：剔除缺 id/名称的脏数据。 */
function normalizeMcpServers(value: unknown): McpServerConfig[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const servers: McpServerConfig[] = [];
  for (const item of value) {
    if (typeof item !== "object" || item === null) {
      continue;
    }
    const raw = item as Record<string, unknown>;
    if (typeof raw.id !== "string" || !raw.id || typeof raw.name !== "string" || !raw.name) {
      continue;
    }
    servers.push({
      id: raw.id,
      name: raw.name,
      transport: raw.transport === "http" ? "http" : "stdio",
      command: typeof raw.command === "string" ? raw.command : null,
      args: Array.isArray(raw.args)
        ? raw.args.filter((a): a is string => typeof a === "string")
        : [],
      env:
        typeof raw.env === "object" && raw.env !== null
          ? Object.fromEntries(
              Object.entries(raw.env as Record<string, unknown>).filter(
                (entry): entry is [string, string] => typeof entry[1] === "string"
              )
            )
          : {},
      url: typeof raw.url === "string" ? raw.url : null,
      enabled: raw.enabled == null ? true : Boolean(raw.enabled),
    });
  }
  return servers;
}

export const useConfigStore = defineStore("config", () => {
  const settings = ref<AppSettings>({
    pet_x: null,
    pet_y: null,
    always_on_top: true,
    locale: null,
    theme: "auto",
    sprite_size: 48,
    accent_color: null,
    app_name: DEFAULT_APP_NAME,
    pet_clock: true,
    monitor_bar_open: false,
    monitor_bar_click_through: false,
    monitor_bar_cpu: true,
    monitor_bar_mem: true,
    monitor_bar_net: true,
    monitor_menubar_open: false,
    monitor_menubar_show_icon: true,
    monitor_refresh_ms: 2000,
  });
  const platform = ref("");
  const windowMoving = ref(false);

  let persistTimer: ReturnType<typeof setTimeout> | undefined;
  let movingTimer: ReturnType<typeof setTimeout> | undefined;

  function schedulePersist() {
    if (persistTimer !== undefined) {
      clearTimeout(persistTimer);
    }
    persistTimer = setTimeout(() => {
      saveSettings(settings.value).catch((err) => {
        console.error("[desk-pet] 设置保存失败:", err);
      });
    }, 500);
  }

  /** 窗口移动回调：防抖持久化位置（展开态的位移来自展开动画，不记录）。 */
  function onWindowMoved(payload: { x: number; y: number }) {
    windowMoving.value = true;
    if (movingTimer !== undefined) {
      clearTimeout(movingTimer);
    }
    movingTimer = setTimeout(() => {
      windowMoving.value = false;
    }, 200);

    if (useUiStore().expanded) {
      return;
    }
    settings.value.pet_x = payload.x;
    settings.value.pet_y = payload.y;
    schedulePersist();
  }

  async function setAlwaysOnTop(value: boolean) {
    settings.value.always_on_top = value;
    await win.setAlwaysOnTop(value);
    schedulePersist();
  }

  /** 切换界面语言：更新 i18n、持久化并通知 Rust 重建托盘菜单。 */
  async function setLocale(locale: string) {
    const loc = normalizeLocale(locale);
    settings.value.locale = loc;
    setAppLocale(loc);
    schedulePersist();
    await emit("i18n://locale-changed", loc);
  }

  /** 切换界面主题：更新 DOM 类、持久化。 */
  async function setTheme(value: string) {
    const theme = normalizeTheme(value);
    settings.value.theme = theme;
    applyTheme(theme);
    schedulePersist();
  }

  /** 调整精灵图标大小：更新设置并持久化。 */
  async function setSpriteSize(value: number) {
    settings.value.sprite_size = clampSpriteSize(value);
    schedulePersist();
  }

  /** 保存登录态：token 与账户名写入设置。 */
  async function setAuth(token: string, user: string) {
    settings.value.auth_token = token;
    settings.value.auth_user = user;
    schedulePersist();
  }

  /** 收藏云同步开关。 */
  async function setFavoriteSync(enabled: boolean) {
    settings.value.favorite_sync = enabled;
    schedulePersist();
  }

  /** 收藏增量拉取基线（服务端 serverNow）。 */
  async function setFavoriteLastSync(serverNow: string) {
    settings.value.favorite_last_sync = serverNow;
    schedulePersist();
  }

  /** 剪贴板云同步开关。 */
  async function setClipboardSync(enabled: boolean) {
    settings.value.clipboard_sync = enabled;
    schedulePersist();
  }

  /** 剪贴板增量拉取基线（服务端 serverNow）。 */
  async function setClipboardLastSync(serverNow: string) {
    settings.value.clipboard_last_sync = serverNow;
    schedulePersist();
  }

  /** 退出登录：清除 token 与账户名。 */
  async function logout() {
    settings.value.auth_token = null;
    settings.value.auth_user = null;
    schedulePersist();
  }

  /** 设置主题色：写入 DOM 变量并持久化，空值恢复默认蓝。 */
  async function setAccentColor(value: string | null | undefined) {
    const color = normalizeAccentColor(value);
    settings.value.accent_color = color;
    applyAccentColor(color);
    schedulePersist();
  }

  /** 设置应用名称：界面显示与托盘提示同步更新。 */
  async function setAppName(value: string | null | undefined) {
    const name = normalizeAppName(value);
    settings.value.app_name = name;
    schedulePersist();
    await emit("app://name-changed", name);
  }

  /** 更换精灵头像：拷贝图片到 app data 并记录路径。 */
  async function changePetAvatar(src: string) {
    settings.value.pet_avatar = await setPetAvatar(src);
    schedulePersist();
  }

  /** 恢复默认精灵头像：删除自定义文件并清空配置。 */
  async function resetPetAvatarConfig() {
    await resetPetAvatar();
    settings.value.pet_avatar = null;
    schedulePersist();
  }

  /** 新增 AI 模型配置，返回生成的 id。 */
  async function addAiModel(model: Omit<AiModel, "id">): Promise<string> {
    const id = genAiModelId();
    settings.value.ai_models = [...(settings.value.ai_models ?? []), { ...model, id }];
    // 首个模型自动设为当前使用
    if (!settings.value.ai_active) {
      settings.value.ai_active = id;
    }
    schedulePersist();
    return id;
  }

  /** 更新 AI 模型配置。 */
  async function updateAiModel(model: AiModel) {
    const list = settings.value.ai_models ?? [];
    const idx = list.findIndex((m) => m.id === model.id);
    if (idx === -1) {
      return;
    }
    const next = [...list];
    next[idx] = { ...model };
    settings.value.ai_models = next;
    schedulePersist();
  }

  /** 删除 AI 模型配置；删除当前使用项时切到首个可用模型。 */
  async function removeAiModel(id: string) {
    const list = (settings.value.ai_models ?? []).filter((m) => m.id !== id);
    settings.value.ai_models = list;
    if (settings.value.ai_active === id) {
      settings.value.ai_active = list.length > 0 ? list[0].id : null;
    }
    schedulePersist();
  }

  /** 切换当前使用的 AI 模型。 */
  async function setActiveAiModel(id: string) {
    if (!(settings.value.ai_models ?? []).some((m) => m.id === id)) {
      return;
    }
    settings.value.ai_active = id;
    schedulePersist();
  }

  /** 切换是否在 Dock 中显示（仅 macOS）。 */
  async function setShowDock(visible: boolean) {
    settings.value.show_dock = visible;
    await invokeSetShowDock(visible);
    schedulePersist();
  }

  /** 设置番茄钟时长（分钟），钳制到 1-120。 */
  function setPomoMinutes(mode: "focus" | "short" | "long", value: number) {
    const clamped = Math.min(120, Math.max(1, Math.round(value)));
    if (mode === "focus") settings.value.pomo_focus_min = clamped;
    else if (mode === "short") settings.value.pomo_short_min = clamped;
    else settings.value.pomo_long_min = clamped;
    schedulePersist();
  }

  /** 切换收起态宠物时钟显示。 */
  async function setPetClock(enabled: boolean) {
    settings.value.pet_clock = enabled;
    schedulePersist();
  }

  /** 悬浮监控条开关（窗口开/关由 monitorbar_toggle 执行，这里只持久化并同步 UI）。 */
  async function setMonitorBar(open: boolean) {
    settings.value.monitor_bar_open = open;
    schedulePersist();
  }

  /** 悬浮监控条鼠标穿透：持久化并实时推送给悬浮条窗口。 */
  async function setMonitorBarClickThrough(value: boolean) {
    settings.value.monitor_bar_click_through = value;
    schedulePersist();
    try {
      await emitTo("monitorbar", "monitorbar://click-through", value);
    } catch {
      // 悬浮条未打开时推送失败可忽略
    }
  }

  /** 悬浮监控条分区开关（CPU/内存/网速独立配置，持久化并实时推送悬浮条与菜单栏）。 */
  async function setMonitorBarSections(patch: { cpu?: boolean; mem?: boolean; net?: boolean }) {
    if (patch.cpu !== undefined) settings.value.monitor_bar_cpu = patch.cpu;
    if (patch.mem !== undefined) settings.value.monitor_bar_mem = patch.mem;
    if (patch.net !== undefined) settings.value.monitor_bar_net = patch.net;
    schedulePersist();
    const sections: MonitorChipSections = {
      cpu: settings.value.monitor_bar_cpu === true,
      mem: settings.value.monitor_bar_mem === true,
      net: settings.value.monitor_bar_net === true,
    };
    try {
      await emitTo("monitorbar", "monitorbar://sections", sections);
    } catch {
      // 悬浮条未打开时推送失败可忽略
    }
    // 菜单栏芯片共用同一组分区开关：立即重绘（无需等下一轮采样）
    if (menubarTick !== undefined) {
      void menubarTick();
    }
  }

  /** 菜单栏监控文字开关（macOS：轮询采样并推给状态项）。 */
  async function setMonitorMenubar(open: boolean) {
    settings.value.monitor_menubar_open = open;
    schedulePersist();
  }

  /** 菜单栏监控文字显示期间是否保留托盘图标（下一轮采样即生效）。 */
  async function setMonitorMenubarIcon(show: boolean) {
    settings.value.monitor_menubar_show_icon = show;
    schedulePersist();
  }

  /** 监控刷新间隔（毫秒）：工具页、悬浮条、菜单栏三处共用。
   *  持久化 + 实时推送悬浮条 + 菜单栏轮询即刻按新间隔重启。 */
  async function setMonitorRefreshMs(ms: number) {
    settings.value.monitor_refresh_ms = normalizeRefreshMs(ms);
    schedulePersist();
    try {
      await emitTo("monitorbar", "monitorbar://refresh", settings.value.monitor_refresh_ms);
    } catch {
      // 悬浮条未打开时推送失败可忽略
    }
    if (menubarTimer !== undefined) {
      stopMenubarLoop(false);
      startMenubarLoop();
    }
  }

  /* ---------- 菜单栏监控轮询（仅主窗口 init 内启动） ---------- */

  let menubarTimer: ReturnType<typeof setInterval> | undefined;
  /** 当前轮询的采样函数引用：分区切换时立即重绘菜单栏芯片 */
  let menubarTick: (() => Promise<void>) | undefined;

  function stopMenubarLoop(clear = true) {
    if (menubarTimer !== undefined) {
      clearInterval(menubarTimer);
      menubarTimer = undefined;
    }
    menubarTick = undefined;
    if (clear) {
      void monitorMenubarSet(null, true).catch(() => undefined);
    }
  }

  function startMenubarLoop() {
    if (menubarTimer !== undefined) {
      return;
    }
    const interval = normalizeRefreshMs(settings.value.monitor_refresh_ms);
    const tick = async () => {
      try {
        const s = await monitorSample();
        const sections: MonitorChipSections = {
          cpu: settings.value.monitor_bar_cpu === true,
          mem: settings.value.monitor_bar_mem === true,
          net: settings.value.monitor_bar_net === true,
        };
        const any = sections.cpu || sections.mem || sections.net;
        // 全部分区关闭时清除菜单栏芯片；图标开关实时读取
        await monitorMenubarSet(any ? s : null, settings.value.monitor_menubar_show_icon === true, sections);
      } catch {
        // 静默失败，下轮重试
      }
    };
    menubarTick = tick;
    void tick();
    menubarTimer = setInterval(() => void tick(), interval);
  }

  /** 新增 MCP 服务器配置，返回生成的 id。 */
  async function addMcpServer(server: Omit<McpServerConfig, "id" | "enabled">): Promise<string> {
    const id = genMcpServerId();
    settings.value.mcp_servers = [
      ...(settings.value.mcp_servers ?? []),
      { ...server, id, enabled: true },
    ];
    schedulePersist();
    return id;
  }

  /** 更新 MCP 服务器配置。 */
  async function updateMcpServer(server: McpServerConfig) {
    const list = settings.value.mcp_servers ?? [];
    const idx = list.findIndex((s) => s.id === server.id);
    if (idx === -1) {
      return;
    }
    const next = [...list];
    next[idx] = { ...server };
    settings.value.mcp_servers = next;
    schedulePersist();
  }

  /** 删除 MCP 服务器配置。 */
  async function removeMcpServer(id: string) {
    settings.value.mcp_servers = (settings.value.mcp_servers ?? []).filter((s) => s.id !== id);
    schedulePersist();
  }

  /** 切换 MCP 服务器启用状态（停用由调用方负责断开连接）。 */
  async function setMcpServerEnabled(id: string, enabled: boolean) {
    const list = settings.value.mcp_servers ?? [];
    const idx = list.findIndex((s) => s.id === id);
    if (idx === -1) {
      return;
    }
    const next = [...list];
    next[idx] = { ...list[idx], enabled };
    settings.value.mcp_servers = next;
    schedulePersist();
  }

  async function init() {
    platform.value = await getPlatform();
    const s = await getSettings();
    const locale = normalizeLocale(s.locale);
    const theme = normalizeTheme(s.theme);
    settings.value = {
      pet_x: s.pet_x,
      pet_y: s.pet_y,
      always_on_top: s.always_on_top,
      locale,
      theme,
      sprite_size: clampSpriteSize(s.sprite_size),
      accent_color: normalizeAccentColor(s.accent_color),
      auth_token: s.auth_token ?? null,
      auth_user: s.auth_user ?? null,
      device_id: s.device_id ?? null,
      pet_avatar: typeof s.pet_avatar === "string" ? s.pet_avatar : null,
      app_name: normalizeAppName(s.app_name),
      show_dock: typeof s.show_dock === "boolean" ? s.show_dock : true,
      ai_models: normalizeAiModels(s.ai_models),
      ai_active: typeof s.ai_active === "string" ? s.ai_active : null,
      favorite_sync: s.favorite_sync == null ? false : Boolean(s.favorite_sync),
      favorite_last_sync: s.favorite_last_sync ?? null,
      clipboard_sync: s.clipboard_sync == null ? false : Boolean(s.clipboard_sync),
      clipboard_last_sync: s.clipboard_last_sync ?? null,
      pomo_focus_min: clampPomoMin(s.pomo_focus_min),
      pomo_short_min: clampPomoMin(s.pomo_short_min),
      pomo_long_min: clampPomoMin(s.pomo_long_min),
      pet_clock: s.pet_clock == null ? true : Boolean(s.pet_clock),
      mcp_servers: normalizeMcpServers(s.mcp_servers),
      monitor_bar_open: s.monitor_bar_open == null ? false : Boolean(s.monitor_bar_open),
      monitor_bar_click_through:
        s.monitor_bar_click_through == null ? false : Boolean(s.monitor_bar_click_through),
      monitor_bar_cpu: s.monitor_bar_cpu == null ? true : Boolean(s.monitor_bar_cpu),
      monitor_bar_mem: s.monitor_bar_mem == null ? true : Boolean(s.monitor_bar_mem),
      monitor_bar_net: s.monitor_bar_net == null ? true : Boolean(s.monitor_bar_net),
      monitor_menubar_open: s.monitor_menubar_open == null ? false : Boolean(s.monitor_menubar_open),
      monitor_menubar_show_icon:
        s.monitor_menubar_show_icon == null ? true : Boolean(s.monitor_menubar_show_icon),
      monitor_refresh_ms: normalizeRefreshMs(s.monitor_refresh_ms),
    };
    setAppLocale(locale);
    applyTheme(theme);
    applyAccentColor(settings.value.accent_color);
    // 启动时同步一次语言，托盘菜单跟随系统语言/用户设置
    await emit("i18n://locale-changed", locale);
    await initWindowPosition(
      s.pet_x !== null && s.pet_y !== null,
      s.pet_x,
      s.pet_y,
      settings.value.sprite_size
    );
    await win.setAlwaysOnTop(settings.value.always_on_top);
    // 悬浮监控条状态广播（工具页/托盘/悬浮条自身关闭都走同一事件）
    await listen<boolean>("monitorbar://state", (event) => {
      settings.value.monitor_bar_open = event.payload;
      schedulePersist();
    });
    // 上次退出时悬浮条处于开启状态则自动恢复
    if (settings.value.monitor_bar_open) {
      try {
        await monitorbarToggle();
      } catch {
        // 自动恢复失败不打断启动
      }
    }
    // 菜单栏监控文字：随开关启停轮询（init 仅主窗口调用，避免多窗口重复轮询）
    watch(
      () => settings.value.monitor_menubar_open,
      (open) => {
        if (open) {
          startMenubarLoop();
        } else {
          stopMenubarLoop();
        }
      },
      { immediate: true }
    );
  }

  return {
    settings,
    platform,
    windowMoving,
    init,
    onWindowMoved,
    setAlwaysOnTop,
    setLocale,
    setTheme,
    setSpriteSize,
    setAccentColor,
    setAppName,
    setAuth,
    setFavoriteSync,
    setFavoriteLastSync,
    setClipboardSync,
    setClipboardLastSync,
    logout,
    changePetAvatar,
    resetPetAvatarConfig,
    addAiModel,
    updateAiModel,
    removeAiModel,
    setActiveAiModel,
    setShowDock,
    setPomoMinutes,
    setPetClock,
    setMonitorBar,
    setMonitorBarClickThrough,
    setMonitorBarSections,
    setMonitorMenubar,
    setMonitorMenubarIcon,
    setMonitorRefreshMs,
    addMcpServer,
    updateMcpServer,
    removeMcpServer,
    setMcpServerEnabled,
  };
});

// 开发期热更新：store 修改后热替换已激活实例，避免组件拿到缺少新方法的旧实例
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useConfigStore, import.meta.hot));
}
