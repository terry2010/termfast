// i18n config tests — FP-7.2
import { describe, it, expect, vi } from "vitest";
import { detectSystemLanguage, resolveLanguage } from "@/i18n/config";

describe("i18n", () => {
  it("detectSystemLanguage returns a valid language", () => {
    const lang = detectSystemLanguage();
    expect(["zh-CN", "zh-TW", "en"]).toContain(lang);
  });

  it("resolveLanguage returns the preference when not 'system'", () => {
    expect(resolveLanguage("zh-CN")).toBe("zh-CN");
    expect(resolveLanguage("en")).toBe("en");
  });

  it("resolveLanguage resolves 'system' to detected language", () => {
    const result = resolveLanguage("system");
    expect(["zh-CN", "zh-TW", "en"]).toContain(result);
  });
});
