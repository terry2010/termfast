import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import zhCN from "./locales/zh-CN.json";
import en from "./locales/en.json";

export type SupportedLanguage = "system" | "zh-CN" | "zh-TW" | "en";

/** Detect system language from navigator */
export function detectSystemLanguage(): string {
  const lang = navigator.language;
  if (lang.startsWith("zh-CN") || lang.startsWith("zh-Hans")) return "zh-CN";
  if (lang.startsWith("zh-TW") || lang.startsWith("zh-Hant")) return "zh-TW";
  if (lang.startsWith("en")) return "en";
  return "en"; // fallback
}

/** Get the actual language to use (resolves "system") */
export function resolveLanguage(pref: SupportedLanguage): string {
  if (pref === "system") return detectSystemLanguage();
  return pref;
}

i18n.use(initReactI18next).init({
  resources: {
    "zh-CN": { translation: zhCN },
    en: { translation: en },
  },
  lng: detectSystemLanguage(),
  fallbackLng: "en",
  returnEmptyString: true,
  interpolation: {
    escapeValue: false,
  },
});

export default i18n;
