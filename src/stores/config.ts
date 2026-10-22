import { defineStore } from "pinia";
import { ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { emit } from "@tauri-apps/api/event";
import { getPlatform, setShowDock as invokeSetShowDock } from "@/api/system";
import { getSettings, saveSettings, setPetAvatar, resetPetAvatar } from "@/api/settings";
import type { AiModel, AppSettings, ChatMessage, ThemeMode } from "@/types";
import { detectLocale, setAppLocale, type AppLocale } from "@/i18n";
import { initWindowPosition } from "@/lib/windowCtl";
import { useUiStore } from "./ui";

const win = getCurrentWindow();

/** 归一化持久化的语言值，非法值回退为系统语言检测。 */
function normalizeLocale(value: string | null): AppLocale {
  if (value === "zh-CN" || value === "en") {
    return value;
  }
  return detectLocale();
}

/** 归一化主题值：light | dark | auto，缺省/非法回退为 auto（跟随系统）。 */
function normalizeTheme(value: string | null | undefined): ThemeMode {
  if (value === "light" || value === "dark" || value === "auto") {
    return value;
  }
  return "auto";
}

/** 归一化精灵大小：钳制到 32-64（呼吸动画峰值不超出 72px 窗口）。 */
function clampSpriteSize(value: number | null | undefined): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return 48;
  }
  return Math.min(64, Math.max(32, Math.round(value)));
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

/** 归一化聊天历史：只保留 user/assistant 角色的字符串内容。 */
function normalizeChatMessages(value: unknown): ChatMessage[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const messages: ChatMessage[] = [];
  for (const item of value) {
    if (typeof item !== "object" || item === null) {
      continue;
    }
    const raw = item as Record<string, unknown>;
    if (typeof raw.role !== "string" || typeof raw.content !== "string") {
      continue;
    }
    if (raw.role !== "user" && raw.role !== "assistant") {
      continue;
    }
    messages.push({ role: raw.role, content: raw.content });
  }
  return messages;
}

function hexToRgb(hex: string): [number, number, number] {
  let h = hex.slice(1);
  if (h.length === 3) {
    h = h
      .split("")
      .map((c) => c + c)
      .join("");
  }
  const n = parseInt(h, 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

function mixHex(base: string, other: string, ratio: number): string {
  const [r1, g1, b1] = hexToRgb(base);
  const [r2, g2, b2] = hexToRgb(other);
  const mix = (a: number, b: number) => Math.round(a * (1 - ratio) + b * ratio);
  const to2 = (v: number) => v.toString(16).padStart(2, "0");
  return `#${to2(mix(r1, r2))}${to2(mix(g1, g2))}${to2(mix(b1, b2))}`;
}

/** 把主题色写入 DOM：覆盖 --dp-accent，并按 Element Plus 规则生成 primary 明暗变体。 */
function applyAccentColor(color: string | null | undefined) {
  const root = document.documentElement;
  if (!color) {
    root.style.removeProperty("--dp-accent");
    root.style.removeProperty("--dp-accent-deep");
    root.style.removeProperty("--el-color-primary");
    root.style.removeProperty("--el-color-primary-light-3");
    root.style.removeProperty("--el-color-primary-light-5");
    root.style.removeProperty("--el-color-primary-light-7");
    root.style.removeProperty("--el-color-primary-light-8");
    root.style.removeProperty("--el-color-primary-light-9");
    root.style.removeProperty("--el-color-primary-dark-2");
    return;
  }
  const full = color.length === 4 ? mixHex(color, color, 0) : color;
  root.style.setProperty("--dp-accent", full);
  root.style.setProperty("--dp-accent-deep", mixHex(full, "#000000", 0.18));
  root.style.setProperty("--el-color-primary", full);
  root.style.setProperty("--el-color-primary-light-3", mixHex(full, "#ffffff", 0.3));
  root.style.setProperty("--el-color-primary-light-5", mixHex(full, "#ffffff", 0.5));
  root.style.setProperty("--el-color-primary-light-7", mixHex(full, "#ffffff", 0.7));
  root.style.setProperty("--el-color-primary-light-8", mixHex(full, "#ffffff", 0.8));
  root.style.setProperty("--el-color-primary-light-9", mixHex(full, "#ffffff", 0.9));
  root.style.setProperty("--el-color-primary-dark-2", mixHex(full, "#000000", 0.2));
}

/** 把主题同步到 DOM（html.dark 同时驱动自研变量与 Element Plus 暗色变量）。auto 跟随系统。 */
let currentTheme: ThemeMode = "auto";
const darkMedia = window.matchMedia("(prefers-color-scheme: dark)");

function applyTheme(theme: ThemeMode) {
  currentTheme = theme;
  const dark = theme === "auto" ? darkMedia.matches : theme === "dark";
  document.documentElement.classList.toggle("dark", dark);
}

darkMedia.addEventListener("change", () => {
  if (currentTheme === "auto") {
    document.documentElement.classList.toggle("dark", darkMedia.matches);
  }
});

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

  /** 保存聊天历史（流式输出中高频调用，走防抖持久化）。 */
  async function setAiMessages(messages: ChatMessage[]) {
    settings.value.ai_messages = messages;
    schedulePersist();
  }

  /** 切换是否在 Dock 中显示（仅 macOS）。 */
  async function setShowDock(visible: boolean) {
    settings.value.show_dock = visible;
    await invokeSetShowDock(visible);
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
      pet_avatar: typeof s.pet_avatar === "string" ? s.pet_avatar : null,
      app_name: normalizeAppName(s.app_name),
      show_dock: typeof s.show_dock === "boolean" ? s.show_dock : true,
      ai_models: normalizeAiModels(s.ai_models),
      ai_active: typeof s.ai_active === "string" ? s.ai_active : null,
      ai_messages: normalizeChatMessages(s.ai_messages),
    };
    setAppLocale(locale);
    applyTheme(theme);
    applyAccentColor(settings.value.accent_color);
    // 启动时同步一次语言，托盘菜单跟随系统语言/用户设置
    await emit("i18n://locale-changed", locale);
    await initWindowPosition(s.pet_x !== null && s.pet_y !== null, s.pet_x, s.pet_y);
    await win.setAlwaysOnTop(settings.value.always_on_top);
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
    logout,
    changePetAvatar,
    resetPetAvatarConfig,
    addAiModel,
    updateAiModel,
    removeAiModel,
    setActiveAiModel,
    setAiMessages,
    setShowDock,
  };
});
