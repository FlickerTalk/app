import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";

const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

import { IonButton, IonIcon } from "@ionic/vue";
import MessageBubble from "./MessageBubble.vue";
import { installed, offered } from "../plugins";
import type { OfferedPlugin, PluginView } from "../core";
import { setLocale } from "../i18n";

const base = { id: "m1", text: "Hi", time: "10:02", mine: true };

describe("MessageBubble", () => {
  it.each([
    ["pending", "Waiting for device"],
    ["sent", "Sent"],
    ["delivered", "Delivered"],
    ["read", "Read"],
    ["unsent", "Not sent"],
  ])("labels the %s state for screen readers", (status, label) => {
    const wrapper = mount(MessageBubble, { props: { message: { ...base, status } }, shallow: true });
    expect(wrapper.find(`[aria-label="${label}"]`).exists()).toBe(true);
  });

  // 2026-10-05: an answer shows what it answers over the text; a tap goes there.
  describe("an answer", () => {
    it("quotes the message it answers and goes to it on a tap", async () => {
      const quote = { id: "m0", text: "dinner on friday?", mine: false, kind: "text" as const };
      const wrapper = mount(MessageBubble, { props: { message: { ...base, text: "yes!", quote } }, shallow: true });
      const shown = wrapper.find("[data-test='quote']");
      expect(shown.text()).toContain("dinner on friday?");
      expect(shown.text()).toContain("Them");
      await shown.trigger("click");
      expect(wrapper.emitted("jump")).toEqual([["m0"]]);
      expect(wrapper.attributes("data-message")).toBe("m1");
    });

    it("names me when it quotes my own message, and the sender in a circle", () => {
      const mine = mount(MessageBubble, { props: { message: { ...base, quote: { id: "m0", text: "ok", mine: true, kind: "text" } } }, shallow: true });
      expect(mine.find("[data-test='quote']").text()).toContain("You");
      const circle = mount(MessageBubble, {
        props: { message: { ...base, mine: false, quote: { id: "m0", text: "ok", mine: false, kind: "text" } }, sender: "Bob" },
        shallow: true,
      });
      expect(circle.find("[data-test='quote']").text()).toContain("Bob");
    });

    it("says so when the quoted message is no longer here, and names a quoted file", () => {
      const gone = mount(MessageBubble, { props: { message: { ...base, quote: { id: "m0", text: "", mine: false, kind: "gone" } } }, shallow: true });
      expect(gone.find("[data-test='quote']").text()).toBe("Message no longer here");
      const file = mount(MessageBubble, { props: { message: { ...base, quote: { id: "m0", text: "holiday.jpg", mine: false, kind: "file" } } }, shallow: true });
      expect(file.find("[data-test='quote']").text()).toContain("holiday.jpg");
    });

    it("has no quote on a plain message", () => {
      expect(mount(MessageBubble, { props: { message: base }, shallow: true }).find("[data-test='quote']").exists()).toBe(false);
    });
  });

  // 2026-10-05: a message taken back shows only its mark; an edited one says so by its time.
  it("shows a message taken back as deleted, and an edited one as edited", () => {
    const gone = mount(MessageBubble, { props: { message: { ...base, text: "", deleted: true } }, shallow: true });
    expect(gone.find("[data-test='deleted']").text()).toBe("Message deleted");
    expect(gone.find(".ft-bubble__text:not(.ft-bubble__gone)").exists()).toBe(false);
    const edited = mount(MessageBubble, { props: { message: { ...base, edited: true } }, shallow: true });
    expect(edited.find("[data-test='edited']").text()).toBe("edited");
    expect(edited.text()).toContain("Hi");
    expect(mount(MessageBubble, { props: { message: base }, shallow: true }).find("[data-test='edited']").exists()).toBe(false);
  });

  // 2026-10-05: a pinned message carries a pin by its time.
  it("marks a pinned message", () => {
    expect(mount(MessageBubble, { props: { message: { ...base, pinned: true } }, shallow: true }).find("[data-test='pinned']").exists()).toBe(true);
    expect(mount(MessageBubble, { props: { message: base }, shallow: true }).find("[data-test='pinned']").exists()).toBe(false);
  });

  // 2026-10-05: the emoji each side put on a message show under it; none, nothing.
  it("shows the emoji each side put on the message", () => {
    const both = mount(MessageBubble, { props: { message: { ...base, reactions: { mine: "👍", theirs: "❤️" } } }, shallow: true });
    expect(both.find("[data-test='reaction-mine']").text()).toBe("👍");
    expect(both.find("[data-test='reaction-theirs']").text()).toBe("❤️");
    const theirs = mount(MessageBubble, { props: { message: { ...base, reactions: { theirs: "❤️" } } }, shallow: true });
    expect(theirs.find("[data-test='reaction-mine']").exists()).toBe(false);
    expect(theirs.find("[data-test='reaction-theirs']").text()).toBe("❤️");
    expect(mount(MessageBubble, { props: { message: base }, shallow: true }).find("[data-test='reactions']").exists()).toBe(false);
  });

  // §84: a message the router refused says so, and can be sent again from where it is.
  it("offers to send again a message that was not sent", async () => {
    const wrapper = mount(MessageBubble, { props: { message: { ...base, status: "unsent" } }, shallow: true });
    const resend = wrapper.find("[data-test='resend']");
    expect(resend.attributes("aria-label")).toBe("Send again");
    await resend.trigger("click");
    expect(wrapper.emitted("resend")).toEqual([["m1"]]);
    expect(wrapper.emitted("actions")).toBeUndefined();
  });

  it.each(["pending", "sent", "delivered", "read"])("offers nothing to send again for a %s message", (status) => {
    const wrapper = mount(MessageBubble, { props: { message: { ...base, status } }, shallow: true });
    expect(wrapper.find("[data-test='resend']").exists()).toBe(false);
  });

  it("shows no delivery state on incoming messages", () => {
    const wrapper = mount(MessageBubble, {
      props: { message: { ...base, mine: false, status: "read" } },
      shallow: true,
    });
    expect(wrapper.find("[aria-label='Read']").exists()).toBe(false);
  });

  // A4: a file bigger than the phone downloads on its own waits for a tap, and says its size.
  it("shows a waiting file with its size and a way to download it", async () => {
    const message = {
      ...base,
      mine: false,
      kind: "file",
      file: { name: "photos.zip", size: "48 MB", progress: 0, state: "waiting" },
    };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    expect(wrapper.text()).toContain("Tap to download");
    expect(wrapper.text()).toContain("48 MB");
    expect(wrapper.find("[role='progressbar']").exists()).toBe(false);
    await wrapper.find("[data-test='download']").trigger("click");
    expect(wrapper.emitted("download")).toEqual([["m1"]]);
    expect(wrapper.emitted("open")).toBeUndefined();
  });

  it("shows a waiting picture as a way to download it, not as a picture", async () => {
    const message = {
      ...base,
      mine: false,
      kind: "file",
      file: { name: "beach.jpg", size: "12 MB", progress: 0, state: "waiting", mime: "image/jpeg" },
    };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    expect(wrapper.find("img").exists()).toBe(false);
    expect(wrapper.find("[data-test='download']").text()).toContain("12 MB");
    await wrapper.find("[data-test='download']").trigger("click");
    expect(wrapper.emitted("download")).toEqual([["m1"]]);
  });

  it("shows the transfer progress of a file", () => {
    const message = {
      ...base,
      kind: "file",
      status: "sent",
      file: { name: "photos.zip", size: "48 MB", progress: 0.62, state: "sending" },
    };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    expect(wrapper.text()).toContain("photos.zip");
    expect(wrapper.find("[role='progressbar']").attributes("aria-valuenow")).toBe("62");
  });

  it("shows a received image", () => {
    const message = {
      ...base,
      mine: false,
      kind: "file",
      file: { name: "beach.jpg", size: "1.2 MB", progress: 1, state: "done", mime: "image/jpeg", url: "asset://localhost/beach.jpg" },
    };
    const image = mount(MessageBubble, { props: { message }, shallow: true }).find("img");
    expect(image.attributes("src")).toBe("asset://localhost/beach.jpg");
    expect(image.attributes("alt")).toBe("beach.jpg");
  });

  it("plays a voice message", () => {
    const message = {
      ...base,
      mine: false,
      kind: "file",
      file: { name: "voice-20260922-161500.m4a", size: "24 KB", progress: 1, state: "done", mime: "audio/mp4", url: "asset://localhost/v.m4a" },
    };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    expect(wrapper.find("audio").attributes("src")).toBe("asset://localhost/v.m4a");
    expect(wrapper.text()).not.toContain("voice-20260922");
  });

  // Media bubbles carry nothing but the medium (Ioan, 2026-09-23): no card, no name, no size.
  it("shows an image with nothing but the picture", () => {
    const message = {
      ...base,
      mine: false,
      kind: "file",
      file: { name: "beach.jpg", size: "1.2 MB", progress: 1, state: "done", mime: "image/jpeg", url: "asset://localhost/beach.jpg" },
    };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    expect(wrapper.find("img").exists()).toBe(true);
    expect(wrapper.find("[data-test='file']").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("beach.jpg");
    expect(wrapper.text()).not.toContain("1.2 MB");
    expect(wrapper.find("[aria-label='Save to Downloads']").exists()).toBe(true);
  });

  it("shows a video with nothing but the video", () => {
    const message = {
      ...base,
      mine: false,
      kind: "file",
      file: { name: "clip.mp4", size: "8 MB", progress: 1, state: "done", mime: "video/mp4", url: "asset://localhost/clip.mp4" },
    };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    expect(wrapper.find("video").exists()).toBe(true);
    expect(wrapper.find("[data-test='file']").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("clip.mp4");
    expect(wrapper.text()).not.toContain("8 MB");
  });

  const voice = {
    ...base,
    mine: false,
    kind: "file",
    file: { name: "voice-20260922-161500.m4a", size: "24 KB", progress: 1, state: "done", mime: "audio/mp4", url: "asset://localhost/v.m4a" },
  };

  it("shows a voice message as a player alone", () => {
    const wrapper = mount(MessageBubble, { props: { message: voice }, shallow: true });
    expect(wrapper.text()).not.toContain("Voice message");
    expect(wrapper.text()).not.toContain("24 KB");
    expect(wrapper.find("[data-test='file']").exists()).toBe(false);
    expect(wrapper.find("[aria-label='Play']").exists()).toBe(true);
    expect(wrapper.find("[aria-label='Save to Downloads']").exists()).toBe(true);
  });

  it("plays and pauses a voice message from its own button", async () => {
    const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
    const pause = vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => undefined);
    const wrapper = mount(MessageBubble, { props: { message: voice }, shallow: true });
    await wrapper.find("[aria-label='Play']").trigger("click");
    expect(play).toHaveBeenCalledTimes(1);
    await wrapper.find("audio").trigger("play");
    expect(wrapper.find("[aria-label='Pause']").exists()).toBe(true);
    await wrapper.find("[aria-label='Pause']").trigger("click");
    expect(pause).toHaveBeenCalledTimes(1);
    expect(wrapper.emitted("open")).toBeUndefined();
    play.mockRestore();
    pause.mockRestore();
  });

  it("shows how long a voice message lasts", async () => {
    const wrapper = mount(MessageBubble, { props: { message: voice }, shallow: true });
    const audio = wrapper.find("audio").element as HTMLAudioElement;
    Object.defineProperty(audio, "duration", { value: 63, configurable: true });
    await wrapper.find("audio").trigger("loadedmetadata");
    expect(wrapper.text()).toContain("1:03");
  });

  // The player draws a waveform (the sketch Ioan approved, 2026-09-23): the bars up to where the
  // message has played are lit. The bars come from the message itself, the same every time.
  it("lights the bars of a voice message as far as it has played", async () => {
    const wrapper = mount(MessageBubble, { props: { message: voice }, shallow: true });
    const bars = wrapper.findAll("[data-test='bar']");
    expect(bars).toHaveLength(18);
    expect(bars.filter((bar) => bar.classes("is-on"))).toHaveLength(0);
    const audio = wrapper.find("audio").element as HTMLAudioElement;
    Object.defineProperty(audio, "duration", { value: 10, configurable: true });
    Object.defineProperty(audio, "currentTime", { value: 5, configurable: true });
    await wrapper.find("audio").trigger("loadedmetadata");
    await wrapper.find("audio").trigger("timeupdate");
    expect(bars.filter((bar) => bar.classes("is-on"))).toHaveLength(Math.round(bars.length / 2));
    const again = mount(MessageBubble, { props: { message: voice }, shallow: true });
    expect(again.findAll("[data-test='bar']").map((bar) => bar.attributes("style"))).toEqual(bars.map((bar) => bar.attributes("style")));
  });

  // A waveform, not noise: two bars next to each other never jump the whole height, so the shape
  // reads as a voice and not as static (Ioan, 2026-09-23).
  // Every message, not just a lucky one: the ids the app really makes are UUIDv7.
  it.each(["m1", "01a0cf49-a448-71fe-87a4-f43f3344df08", "01a0cf8b-1395-7263-b947-d600372d4113", "01a0cf69-f758-745e-91b4-7a59978b9e12"])(
    "draws a waveform that rises and falls gently for %s",
    (id) => {
      const wrapper = mount(MessageBubble, { props: { message: { ...voice, id } }, shallow: true });
      const heights = wrapper.findAll("[data-test='bar']").map((bar) => Number(/height: ([\d.]+)%/.exec(bar.attributes("style") ?? "")?.[1]));
      expect(heights.every((height) => height >= 20 && height <= 100)).toBe(true);
      const jumps = heights.slice(1).map((height, index) => Math.abs(height - heights[index]));
      expect(Math.max(...jumps)).toBeLessThanOrEqual(30);
      // A shape, not a flat line: it neither flattens out nor sticks to the edges.
      expect(Math.max(...heights) - Math.min(...heights)).toBeGreaterThan(30);
      const runs = heights.reduce<number[]>((all, height, index) => {
        if (index > 0 && height === heights[index - 1]) all[all.length - 1] += 1;
        else all.push(1);
        return all;
      }, []);
      expect(Math.max(...runs)).toBeLessThanOrEqual(2);
    },
  );

  it("keeps the name of a document but not its size", () => {
    const wrapper = mount(MessageBubble, { props: { message: arrived }, shallow: true });
    expect(wrapper.text()).toContain("menu.pdf");
    expect(wrapper.text()).not.toContain("1.2 MB");
  });

  // The state stays in sight and honest (§84), over the medium itself.
  it("shows how much of an image has arrived", () => {
    const message = {
      ...base,
      mine: false,
      kind: "file",
      file: { name: "beach.jpg", size: "1.2 MB", progress: 0.62, state: "receiving", mime: "image/jpeg" },
    };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    expect(wrapper.find("[role='progressbar']").attributes("aria-valuenow")).toBe("62");
    expect(wrapper.text()).toContain("62%");
    expect(wrapper.find("[aria-label='Save to Downloads']").exists()).toBe(false);
  });

  it("says when a voice message failed to arrive", () => {
    const message = { ...voice, file: { ...voice.file, progress: 0, state: "failed", url: undefined } };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    expect(wrapper.text()).toContain("Failed");
    expect(wrapper.find("[aria-label='Play']").exists()).toBe(false);
  });

  it("says when a transfer failed", () => {
    const message = { ...base, mine: false, kind: "file", file: { name: "a.bin", size: "10 B", progress: 0, state: "failed" } };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    expect(wrapper.text()).toContain("Failed");
    expect(wrapper.find("[role='progressbar']").exists()).toBe(false);
  });

  const arrived = {
    ...base,
    mine: false,
    kind: "file",
    file: { name: "menu.pdf", size: "1.2 MB", progress: 1, state: "done", mime: "application/pdf" },
  };

  it("opens an arrived file when tapped", async () => {
    const wrapper = mount(MessageBubble, { props: { message: arrived }, shallow: true });
    await wrapper.find("[data-test='file']").trigger("click");
    expect(wrapper.emitted("open")).toEqual([["m1"]]);
  });

  it("offers to save an arrived file to Downloads", async () => {
    const wrapper = mount(MessageBubble, { props: { message: arrived }, shallow: true });
    await wrapper.find("[aria-label='Save to Downloads']").trigger("click");
    expect(wrapper.emitted("save")).toEqual([["m1"]]);
    expect(wrapper.emitted("open")).toBeUndefined();
  });

  it("shows when the file has been saved", () => {
    const wrapper = mount(MessageBubble, { props: { message: arrived, saved: true }, shallow: true });
    expect(wrapper.find("[aria-label='Saved to Downloads']").exists()).toBe(true);
  });

  it("cannot open or save a file still on its way", async () => {
    const message = { ...arrived, file: { ...arrived.file, progress: 0.4, state: "receiving" } };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    await wrapper.find("[data-test='file']").trigger("click");
    expect(wrapper.emitted("open")).toBeUndefined();
    expect(wrapper.find("[aria-label='Save to Downloads']").exists()).toBe(false);
  });

  // A message written with fences is code: monospace, as it was written, with its language.
  it("shows a fenced message as a block of code", () => {
    const wrapper = mount(MessageBubble, {
      props: { message: { ...base, text: "```python\nif x:\n    go()\n```" } },
      shallow: true,
    });
    const code = wrapper.find("[data-test='code']");
    expect(code.exists()).toBe(true);
    expect(code.text()).toContain("if x:");
    expect(code.text()).toContain("    go()");
    expect(wrapper.text()).toContain("python");
    expect(wrapper.find("[data-test='code'] script").exists()).toBe(false);
  });

  it("leaves a message that is not code as it is", () => {
    const wrapper = mount(MessageBubble, { props: { message: { ...base, text: "just text" } }, shallow: true });
    expect(wrapper.find("[data-test='code']").exists()).toBe(false);
    expect(wrapper.text()).toContain("just text");
  });

  // A video that arrived plays in the chat, like an image or a voice message.
  it("plays a received video", () => {
    const message = {
      ...base,
      kind: "file",
      file: { name: "clip.mp4", size: "8 MB", mime: "video/mp4", state: "ready", progress: 1, url: "asset://localhost/files/clip.mp4" },
    };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    const video = wrapper.find("video");
    expect(video.exists()).toBe(true);
    expect(video.attributes("src")).toBe("asset://localhost/files/clip.mp4");
    expect(video.attributes("controls")).toBeDefined();
  });

  // Addresses are marked so they can be opened, and nothing is ever fetched to preview them.
  it("marks the links of a message", async () => {
    const wrapper = mount(MessageBubble, {
      props: { message: { ...base, text: "mira https://flickertalk.com y escribe a info@flickertalk.com" } },
      shallow: true,
    });
    const links = wrapper.findAll("[data-test='link']");
    expect(links.map((link) => link.text())).toEqual(["https://flickertalk.com", "info@flickertalk.com"]);
    await links[1].trigger("click");
    expect(opener.openUrl).toHaveBeenCalledWith("mailto:info@flickertalk.com");
    expect(wrapper.find("img[src^='http']").exists()).toBe(false);
  });

  // 2026-10-02: a place the location plugin wrote is a card, never a map: 📍, "Location" and how
  // far off it may be. Nothing is fetched until the tap, which opens the phone's maps app.
  it("shows a place as a card that opens the phone's maps app", async () => {
    opener.openUrl.mockReset();
    const wrapper = mount(MessageBubble, {
      props: { message: { ...base, mine: false, text: "geo:40.41680,-3.70380;u=35" } },
      shallow: true,
    });
    const place = wrapper.find("[data-test='place']");
    expect(place.exists()).toBe(true);
    expect(place.text()).toContain("📍");
    expect(place.text()).toContain("Location");
    expect(place.text()).toContain("±35 m");
    expect(place.text()).not.toContain("geo:");
    expect(place.attributes("aria-label")).toBe("Location ±35 m: open in maps");
    expect(wrapper.find("img").exists()).toBe(false);
    expect(opener.openUrl).not.toHaveBeenCalled();
    await place.trigger("click");
    expect(opener.openUrl).toHaveBeenCalledWith("https://maps.apple.com/?ll=40.4168,-3.7038&q=40.4168,-3.7038");
  });

  it("shows a place without an accuracy, and leaves a malformed one as text", () => {
    const plain = mount(MessageBubble, { props: { message: { ...base, text: "geo:48.2010,16.3695" } }, shallow: true });
    expect(plain.find("[data-test='place']").text()).not.toContain("±");
    const broken = mount(MessageBubble, { props: { message: { ...base, text: "geo:91.0,10.0" } }, shallow: true });
    expect(broken.find("[data-test='place']").exists()).toBe(false);
    expect(broken.text()).toContain("geo:91.0,10.0");
  });

  // A long press is how a message is acted on (Ioan, 2026-09-23); a tap is not.
  it("asks for the actions of a message after a long press, not a tap", async () => {
    vi.useFakeTimers();
    const wrapper = mount(MessageBubble, { props: { message: base }, shallow: true });
    const bubble = wrapper.find("[data-test='bubble']");

    await bubble.trigger("pointerdown");
    await bubble.trigger("pointerup");
    vi.advanceTimersByTime(1000);
    expect(wrapper.emitted("actions")).toBeUndefined();

    await bubble.trigger("pointerdown");
    vi.advanceTimersByTime(600);
    expect(wrapper.emitted("actions")).toEqual([["m1"]]);
    vi.useRealTimers();
  });

  // Folded, a long message takes a few lines instead of the whole screen; it is still there.
  it("folds a message that was folded, and says how to unfold it", () => {
    const long = { ...base, text: "line\n".repeat(40) };
    const open = mount(MessageBubble, { props: { message: long }, shallow: true });
    expect(open.find("[data-test='folded']").exists()).toBe(false);

    const folded = mount(MessageBubble, { props: { message: long, folded: true }, shallow: true });
    expect(folded.find("[data-test='folded']").exists()).toBe(true);
    expect(folded.text()).toContain("line");
  });

  // Plan 10.6: an invitation is a text with the exact link of a game's page. A game of the signed
  // catalogue, or already here, gets a way to play under it; anything that looks like it, nothing.
  describe("an invitation to a game", () => {
    const CHESS: PluginView = {
      id: "com.flickertalk.game.chess",
      name: "Chess",
      version: "1.0.0",
      kind: "game",
      asks: { network: [], messages: false, send: "propose", live: true },
      granted: { network: [], messages: false, send: "propose", live: true },
      installedAt: 1,
    };
    const GO: OfferedPlugin = {
      id: "com.flickertalk.game.go",
      name: "Go",
      version: "1.0.0",
      summary: "Play go.",
      size: 1_200_000,
      installed: false,
      carried: false,
      kind: "game",
    };
    const invite = (text: string, games = true) =>
      mount(MessageBubble, { props: { message: { ...base, mine: false, text }, games }, shallow: true });

    beforeEach(() => {
      installed.value = [CHESS];
      offered.value = [GO];
    });
    afterEach(() => {
      installed.value = [];
      offered.value = [];
    });

    it("offers to play an installed game in this conversation", async () => {
      const wrapper = invite("🎮 Shall we play Chess? https://flickertalk.com/games/chess");
      const play = wrapper.find("[data-test='play-game']");
      expect(play.text()).toContain("Play");
      expect(play.text()).not.toContain("MB");
      // Ioan, 2026-10-02: Ionic's own button, and an icon, never an emoji, for the interface.
      const button = wrapper.findAllComponents(IonButton).find((one) => one.attributes("data-test") === "play-game");
      expect(button?.findComponent(IonIcon).exists()).toBe(true);
      expect(play.html()).not.toContain("🎮");
      // The address stays a link like any other.
      expect(wrapper.find("[data-test='link']").attributes("href")).toBe("https://flickertalk.com/games/chess");
      await play.trigger("click");
      expect(wrapper.emitted("play")).toEqual([[CHESS.id]]);
      expect(wrapper.emitted("actions")).toBeUndefined();
    });

    // 2026-10-02 (plan of the catalogue's translations): the button names the game in the
    // receiver's language, whatever language the invitation was written in.
    it("names the game in the phone's language", async () => {
      await setLocale("es");
      try {
        const fourInARow = { ...GO, id: "com.flickertalk.game.fourinarow", name: "Four in a Row", locales: { es: { name: "Cuatro en raya" } } };
        offered.value = [fourInARow];
        const play = invite("🎮 Four in a Row · Shall we play? https://flickertalk.com/games/fourinarow").find("[data-test='play-game']");
        expect(play.text()).toContain("Cuatro en raya");
        expect(play.text()).not.toContain("Four in a Row");
      } finally {
        await setLocale("en");
      }
    });

    it("says what a game of the catalogue is and weighs before installing it", async () => {
      const wrapper = invite("https://flickertalk.com/games/go");
      const play = wrapper.find("[data-test='play-game']");
      expect(play.text()).toContain("Go");
      expect(play.text()).toContain("1.2 MB");
      await play.trigger("click");
      expect(wrapper.emitted("play")).toEqual([[GO.id]]);
    });

    it.each([
      ["a game that is not in the catalogue", "https://flickertalk.com/games/poker"],
      ["another host", "https://flickertalk.org/games/chess"],
      ["a host that only starts like ours", "https://flickertalk.com.evil.com/games/chess"],
      ["plain http", "http://flickertalk.com/games/chess"],
      ["more path", "https://flickertalk.com/games/chess/extra"],
      ["an upper-case host", "https://FLICKERTALK.COM/games/chess"],
      ["an upper-case game", "https://flickertalk.com/games/CHESS"],
      ["a look-alike letter", "https://flickertаlk.com/games/chess"],
    ])("offers nothing for %s", (_case, text) => {
      expect(invite(text).find("[data-test='play-game']").exists()).toBe(false);
    });

    // A tool that happens to have a game's id is not a game.
    it("offers nothing for a plugin that is not a game", () => {
      installed.value = [{ ...CHESS, kind: "tool" }];
      offered.value = [{ ...GO, id: CHESS.id, kind: undefined }];
      expect(invite("https://flickertalk.com/games/chess").find("[data-test='play-game']").exists()).toBe(false);
    });

    // A circle (or a phone without games) does not play: no button there.
    it("offers nothing where games are not played", () => {
      expect(invite("https://flickertalk.com/games/chess", false).find("[data-test='play-game']").exists()).toBe(false);
    });
  });

  // Seen in Arabic (2026-10-02): what someone wrote takes its direction from itself, not from the
  // app's language, or «¿Probamos?» reads «?Probamos¿» with its punctuation moved to the other end.
  describe("what someone wrote, in its own direction", () => {
    it("a text", () => {
      const wrapper = mount(MessageBubble, { props: { message: { ...base, text: "¿Probamos la hoja de apps?" } }, shallow: true });
      expect(wrapper.find(".ft-bubble__text").attributes("dir")).toBe("auto");
    });

    it("a block of code", () => {
      const wrapper = mount(MessageBubble, { props: { message: { ...base, text: "```\ngo();\n```" } }, shallow: true });
      expect(wrapper.find(".ft-code__body").attributes("dir")).toBe("auto");
    });

    it("the name of a file", () => {
      const message = { ...base, mine: false, kind: "file", file: { name: "menu (1).pdf", size: "1.2 MB", progress: 1, state: "done", mime: "application/pdf" } };
      const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
      expect(wrapper.find(".ft-file__name").attributes("dir")).toBe("auto");
    });

    it("who said it, in a circle", () => {
      const wrapper = mount(MessageBubble, { props: { message: { ...base, mine: false }, sender: "Ana (work)" }, shallow: true });
      expect(wrapper.find("[data-test='sender']").attributes("dir")).toBe("auto");
    });
  });
});
