import { onBeforeUnmount, onMounted, watch } from "vue";
import { toastController } from "@ionic/vue";
import { pluginOpen } from "./core";
import { t } from "./i18n";
import { NOTICE_DURATION } from "./plugins";

type Named = { id: string; name: string };
type Marked = Named & { count: number };

export const OPEN_PLUGINS_KEY = "ft-open-plugins";

function isMarked(value: unknown): value is Marked {
  if (!value || typeof value !== "object") return false;
  const { id, name, count } = value as Record<string, unknown>;
  return typeof id === "string" && typeof name === "string" && typeof count === "number" && count > 0;
}

function marked(): Marked[] {
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(OPEN_PLUGINS_KEY) ?? "[]");
    return Array.isArray(stored) ? stored.filter(isMarked) : [];
  } catch {
    return [];
  }
}

function keep(list: Marked[]) {
  if (list.length) localStorage.setItem(OPEN_PLUGINS_KEY, JSON.stringify(list));
  else localStorage.removeItem(OPEN_PLUGINS_KEY);
}

export function markOpen({ id, name }: Named): void {
  const list = marked();
  const found = list.find((one) => one.id === id);
  if (found) found.count += 1;
  else list.push({ id, name, count: 1 });
  keep(list);
}

export function markClosed(id: string): void {
  keep(marked().flatMap((one) => (one.id !== id ? [one] : one.count > 1 ? [{ ...one, count: one.count - 1 }] : [])));
}

export function takeClosedPlugins(): string[] {
  const names = marked().map((one) => one.name);
  localStorage.removeItem(OPEN_PLUGINS_KEY);
  return names;
}

export function followOpenPlugin(plugin: () => Named, hidden: () => boolean): void {
  const tell = (id: string, open: boolean) => void pluginOpen(id, open).catch(() => undefined);
  let kept: string | null = null;
  const keepFor = (now: Named | null) => {
    if (kept) markClosed(kept);
    kept = now && !hidden() ? now.id : null;
    if (now && kept) markOpen(now);
  };
  onMounted(() => keepFor(plugin()));
  watch(
    () => plugin().id,
    (now, before) => {
      tell(before, false);
      tell(now, true);
      keepFor(plugin());
    },
  );
  onBeforeUnmount(() => {
    tell(plugin().id, false);
    keepFor(null);
  });
}

export async function tellClosedPlugins(names: string[]): Promise<void> {
  for (const name of names) {
    const toast = await toastController.create({ message: t("plugins.closed", { name }), duration: NOTICE_DURATION, position: "top" });
    await toast.present();
    await toast.onDidDismiss();
  }
}
