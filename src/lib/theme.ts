/** 主题与主题色应用：主窗口（config store）与磁盘分析窗口共用，保证外观一致。 */

export type ThemeMode = "light" | "dark" | "auto";

const darkMedia = window.matchMedia("(prefers-color-scheme: dark)");

let currentTheme: ThemeMode = "auto";

export function normalizeTheme(value: string | null | undefined): ThemeMode {
  return value === "dark" || value === "light" || value === "auto" ? value : "auto";
}

/** 把主题同步到 DOM（html.dark 同时驱动自研变量与 Element Plus 暗色变量）。 */
export function applyTheme(theme: string | null | undefined): ThemeMode {
  currentTheme = normalizeTheme(theme);
  const dark = currentTheme === "auto" ? darkMedia.matches : currentTheme === "dark";
  document.documentElement.classList.toggle("dark", dark);
  return currentTheme;
}

/** auto 模式跟随系统：每个窗口各自的模块实例监听，互不干扰。 */
darkMedia.addEventListener("change", () => {
  if (currentTheme === "auto") {
    document.documentElement.classList.toggle("dark", darkMedia.matches);
  }
});

/** 当前是否暗色（Canvas 绘制取色用）。 */
export function isDark(): boolean {
  return document.documentElement.classList.contains("dark");
}

function hexToRgb(hex: string): [number, number, number] {
  const value = hex.replace("#", "");
  const full =
    value.length === 3
      ? value
          .split("")
          .map((ch) => ch + ch)
          .join("")
      : value;
  return [
    parseInt(full.slice(0, 2), 16),
    parseInt(full.slice(2, 4), 16),
    parseInt(full.slice(4, 6), 16),
  ];
}

function mixHex(base: string, other: string, ratio: number): string {
  const [r1, g1, b1] = hexToRgb(base);
  const [r2, g2, b2] = hexToRgb(other);
  const mix = (a: number, b: number) => Math.round(a * (1 - ratio) + b * ratio);
  const to2 = (v: number) => v.toString(16).padStart(2, "0");
  return `#${to2(mix(r1, r2))}${to2(mix(g1, g2))}${to2(mix(b1, b2))}`;
}

/** 把主题色写入 DOM：覆盖 --dp-accent，并按 Element Plus 规则生成 primary 明暗变体。 */
export function applyAccentColor(color: string | null | undefined) {
  const root = document.documentElement;
  if (!color) {
    const keys = [
      "--dp-accent",
      "--dp-accent-deep",
      "--el-color-primary",
      "--el-color-primary-light-3",
      "--el-color-primary-light-5",
      "--el-color-primary-light-7",
      "--el-color-primary-light-8",
      "--el-color-primary-light-9",
      "--el-color-primary-dark-2",
    ];
    for (const key of keys) {
      root.style.removeProperty(key);
    }
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
