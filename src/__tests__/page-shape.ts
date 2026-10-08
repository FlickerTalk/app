import type { VueWrapper } from "@vue/test-utils";

// Ionic's page transitions look for a page's header, content and footer among the page's own
// children (`:scope > ion-header`, `:scope > ion-content`, `:scope > ion-tabs`…): anything of ours
// in between and the page does not slide, or its title moves over a page that stays still.

/** Overlays sit anywhere in a page: Ionic presents them over it and its transitions skip them. */
const OVERLAYS = new Set(["ion-modal", "ion-alert", "ion-toast", "ion-loading", "ion-popover", "ion-action-sheet"]);

/** The tag a shallow mount gives a component's stub, without the `-stub`. */
const tagOf = (element: Element) => element.tagName.toLowerCase().replace(/-stub$/, "");

/** The children of an element, by tag, overlays and the components named in `skip` left out. */
export function childTags(element: Element, skip: string[] = []): string[] {
  return Array.from(element.children)
    .map(tagOf)
    .filter((tag) => !OVERLAYS.has(tag) && !skip.includes(tag));
}

/**
 * The shape of a page mounted shallow: its root must be the `ion-page`, and what it holds, by tag,
 * comes back (overlays and the components in `skip`, which present sheets, left out).
 */
export function pageShape(wrapper: VueWrapper, skip: string[] = []): string[] {
  const root = wrapper.element as Element;
  if (tagOf(root) !== "ion-page") throw new Error(`the page's root is <${tagOf(root)}>, not <ion-page>`);
  return childTags(root, skip);
}
