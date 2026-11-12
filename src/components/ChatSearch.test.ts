import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { IonButton, IonSearchbar } from "@ionic/vue";
import type { ChatMessage } from "../core";
import ChatSearch from "./ChatSearch.vue";

const HITS: ChatMessage[] = [
  { id: "m3", mine: false, text: "Perfect, I'll book the table", time: "09:31", sentAt: 1 },
  { id: "m2", mine: true, text: "Yes", time: "09:30", sentAt: 1 },
  { id: "m4", mine: true, text: "", time: "09:38", sentAt: 1, kind: "file", file: { name: "table.pdf" } as ChatMessage["file"] },
];

const panel = (props: Partial<{ query: string; hits: ChatMessage[]; searched: boolean }> = {}) =>
  mount(ChatSearch, { props: { query: "", hits: [], searched: false, contactName: "Maria López", ...props }, global: { stubs: { IonIcon: true } } });

describe("ChatSearch", () => {
  it("is Ionic's searchbar in this conversation, with a clear button that closes it", async () => {
    const wrapper = panel({ query: "tab" });
    const bar = wrapper.findComponent(IonSearchbar);
    expect(bar.props("modelValue")).toBe("tab");
    expect(bar.attributes("aria-label")).toBe("Search in this conversation");
    const close = wrapper.findComponent(IonButton);
    expect(close.props("fill")).toBe("clear");
    await close.trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
  });

  it("hands up what is typed", () => {
    const wrapper = panel();
    wrapper.findComponent(IonSearchbar).vm.$emit("update:modelValue", "table");
    expect(wrapper.emitted("type")).toEqual([["table"]]);
  });

  it("lists the hits with who wrote them and when, a file by its name, and goes to one", async () => {
    const wrapper = panel({ hits: HITS, searched: true });
    const rows = wrapper.findAll("[data-test^='hit-']");
    expect(rows.map((row) => row.find(".ft-search__who").text())).toEqual(["Maria López · 09:31", "You · 09:30", "You · 09:38"]);
    expect(rows[2].find(".ft-search__text").text()).toBe("table.pdf");
    expect(rows[0].find(".ft-search__text").attributes("dir")).toBe("auto");
    await rows[0].trigger("click");
    expect(wrapper.emitted("visit")).toEqual([["m3"]]);
    expect(wrapper.find("[data-test='search-none']").exists()).toBe(false);
  });

  it("says when nothing was found, and nothing before searching", () => {
    expect(panel({ searched: true }).find("[data-test='search-none']").text()).toBe("Nothing found");
    expect(panel().find("[data-test='search-none']").exists()).toBe(false);
  });

  it("lets its page reach the field to focus it", () => {
    const wrapper = panel();
    expect((wrapper.vm as unknown as { searchbar: { $el: Element } }).searchbar.$el.tagName).toBe("ION-SEARCHBAR");
  });
});
