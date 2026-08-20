// shadcn/ui utility — class name combiner
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * Decode a base64 string to a UTF-8 string.
 * Use this instead of `atob()` when the decoded content is a UTF-8 string
 * (e.g. JSON with non-ASCII characters). `atob()` returns a binary string
 * where each char is one byte, which mangles multi-byte UTF-8 sequences
 * (e.g. Chinese characters become Latin-1 garbage like "ç»ç«¯").
 */
export function decodeBase64Utf8(b64: string): string {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return new TextDecoder("utf-8").decode(bytes);
}

/**
 * Decode a base64 string and parse as JSON (UTF-8 safe).
 * Use this instead of `JSON.parse(atob(...))` when the JSON payload
 * may contain non-ASCII characters (e.g. terminal names in Chinese).
 */
export function decodeBase64Json<T = unknown>(b64: string): T {
  return JSON.parse(decodeBase64Utf8(b64)) as T;
}
