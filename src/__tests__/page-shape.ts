import type { VueWrapper } from "@vue/test-utils";

const OVERLAYS = new Set(["ion-modal", "ion-alert", "ion-toast", "ion-loading", "ion-popover", "ion-action-sheet"]);

const tagOf = (element: Element) =>
  element.classList.contains("ion-page") ? "ion-page" : element.tagName.toLowerCase().replace(/-stub$/, "");

export function childTags(element: Element, skip: string[] = []): string[] {
  return Array.from(element.children)
    .map(tagOf)
    .filter((tag) => !OVERLAYS.has(tag) && !skip.includes(tag));
}

export function pageShape(wrapper: VueWrapper, skip: string[] = []): string[] {
  const root = wrapper.element as Element;
  if (tagOf(root) !== "ion-page") throw new Error(`the page's root is <${tagOf(root)}>, not <ion-page>`);
  return childTags(root, skip);
}
