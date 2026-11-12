import { i18n } from "../i18n";

export function hueOf(id: string): number {
  let hash = 2166136261;
  for (const char of id) {
    hash = Math.imul(hash ^ char.charCodeAt(0), 16777619);
  }
  return (hash >>> 0) % 360;
}

export function clock(ms: number): string {
  if (!ms) return "";
  const date = new Date(ms);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function formatSize(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const digits = unit === 0 || value >= 10 ? 0 : 1;
  const shown = new Intl.NumberFormat(i18n.global.locale.value, { maximumFractionDigits: digits }).format(value);
  return `${shown} ${units[unit]}`;
}
