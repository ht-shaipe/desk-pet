import { createI18n } from "vue-i18n";
import zhCN from "./zh-CN";
import en from "./en";

export const LOCALE_ZH = "zh-CN";
export const LOCALE_EN = "en";

export type AppLocale = typeof LOCALE_ZH | typeof LOCALE_EN;

/** 按浏览器语言自动选择默认语言。 */
export function detectLocale(): AppLocale {
  return navigator.language.toLowerCase().startsWith("zh") ? LOCALE_ZH : LOCALE_EN;
}

export const i18n = createI18n({
  legacy: false,
  locale: LOCALE_ZH,
  fallbackLocale: LOCALE_EN,
  messages: {
    [LOCALE_ZH]: zhCN,
    [LOCALE_EN]: en,
  },
});

/** 切换应用语言（含 <html lang> 标记）。 */
export function setAppLocale(locale: AppLocale): void {
  i18n.global.locale.value = locale;
  document.documentElement.setAttribute("lang", locale);
}
