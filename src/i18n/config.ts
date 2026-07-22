import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import zhCN from "./locales/zh-CN.json";
import en from "./locales/en.json";

export type SupportedLanguage = "system" | "zh-CN" | "zh-TW" | "en";

/** Detect system language from navigator (sync, webview-based).
 *  Used as initial guess; asyncDetectSystemLanguage overrides with
 *  OS-level locale (more reliable in Tauri webview).
 */
export function detectSystemLanguage(): string {
  const lang = navigator.language || "";
  const lower = lang.toLowerCase();
  if (lower.startsWith("zh")) return "zh-CN";
  if (lower.startsWith("en")) return "en";
  return "en";
}

/** Detect system language from OS (async, Tauri command).
 *  More reliable than navigator.language in Tauri webview,
 *  which may default to en-US regardless of system language.
 *  Falls back to navigator.language if Tauri command fails.
 */
export async function asyncDetectSystemLanguage(): Promise<string> {
  try {
    const locale = await import("@tauri-apps/api/core")
      .then((m) => m.invoke<string>("ipc_get_system_locale"))
      .catch(() => null);
    if (locale) {
      const lower = locale.toLowerCase();
      if (lower.startsWith("zh")) return "zh-CN";
      if (lower.startsWith("en")) return "en";
    }
  } catch {
    // Not in Tauri (e.g. browser dev mode) — fall through
  }
  return detectSystemLanguage();
}

/** Get the actual language to use (resolves "system") */
export function resolveLanguage(pref: SupportedLanguage): string {
  if (pref === "system") return detectSystemLanguage();
  return pref;
}

/** Async version of resolveLanguage — uses OS-level locale for "system" */
export async function asyncResolveLanguage(pref: SupportedLanguage): Promise<string> {
  if (pref === "system") return asyncDetectSystemLanguage();
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
