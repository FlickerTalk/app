import { describe, expect, it } from "vitest";
import { parse } from "vue/compiler-sfc";
import base from "../theme/base.css?raw";

type Node = {
  type: number;
  tag?: string;
  props?: Array<{ type: number; name: string; value?: { content: string } }>;
  children?: Node[];
};

const ELEMENT = 1;
const ATTRIBUTE = 6;
const DIRECTIVE = 7;

const sources = import.meta.glob<string>("/src/**/*.vue", { eager: true, query: "?raw", import: "default" });

const attribute = (node: Node, name: string) => node.props?.find((prop) => prop.type === ATTRIBUTE && prop.name === name)?.value?.content;
const directive = (node: Node, name: string) => node.props?.some((prop) => prop.type === DIRECTIVE && prop.name === name) ?? false;
const elementsIn = (node: Node) => (node.children ?? []).filter((child) => child.type === ELEMENT);

function* descendants(node: Node): Generator<Node> {
  for (const child of elementsIn(node)) {
    yield child;
    yield* descendants(child);
  }
}

const isLargeTitle = (node: Node) => node.tag === "ion-header" && attribute(node, "collapse") === "condense";

function contentWithLargeTitle(source: string): Node | undefined {
  const root = parse(source).descriptor.template?.ast as Node | undefined;
  return root ? [...descendants(root)].find((node) => node.tag === "ion-content" && elementsIn(node).some(isLargeTitle)) : undefined;
}

function blocksUnder(content: Node): Node[][] {
  const blocks: Node[][] = [];
  for (const node of elementsIn(content).slice(1)) {
    if (directive(node, "else-if") || directive(node, "else")) blocks.at(-1)?.push(node);
    else blocks.push([node]);
  }
  return blocks;
}

const pages = Object.entries(sources)
  .map(([path, source]) => [path.replace(/^\/src\//, ""), contentWithLargeTitle(source)] as const)
  .filter((page): page is readonly [string, Node] => page[1] !== undefined);

describe("pages with an iOS large title", () => {
  it("are Chats, Calls and Settings", () => {
    expect(pages.map(([path]) => path).sort()).toEqual(["views/CallsPage.vue", "views/ChatsPage.vue", "views/SettingsPage.vue"]);
  });

  it.each(pages)("%s starts its content with the large title and puts everything else in one block", (_, content) => {
    expect(isLargeTitle(elementsIn(content)[0])).toBe(true);
    const blocks = blocksUnder(content);
    expect(blocks.map((block) => block.map((node) => node.tag))).toHaveLength(1);
    expect(blocks[0].some((node) => directive(node, "for"))).toBe(false);
  });

  it("fill more than the screen under the large title on iOS, their own padding on top, so the title can always scroll wholly past the bar's edge", () => {
    const rule = /ion-content\.ios\s*>\s*ion-header\.header-collapse-condense\s*\+\s*\*\s*\{([^}]*)\}/.exec(base)?.[1] ?? "";
    expect(rule).toMatch(/min-height:\s*calc\(100% \+ 1px\);/);
    expect(rule).toMatch(/box-sizing:\s*content-box;/);
  });
});
