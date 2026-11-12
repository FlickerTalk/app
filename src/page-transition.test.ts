import { afterEach, describe, expect, it, vi } from "vitest";
import type { Animation } from "@ionic/vue";

import { pageTransition } from "./page-transition";

type Opts = Parameters<typeof pageTransition>[1];

function outlet() {
  document.body.innerHTML = `
    <div class="outlet">
      <div class="ion-page" id="leaving"><ion-header><ion-toolbar><ion-title>Chats</ion-title></ion-toolbar></ion-header><ion-content></ion-content></div>
      <div class="ion-page ion-page-invisible" id="entering"><ion-header><ion-toolbar><ion-buttons slot="start"><ion-back-button></ion-back-button></ion-buttons></ion-toolbar></ion-header><ion-content></ion-content></div>
    </div>`;
  const pick = (id: string) => document.getElementById(id) as HTMLElement;
  return { baseEl: document.querySelector(".outlet") as HTMLElement, enteringEl: pick("entering"), leavingEl: pick("leaving") };
}

function build(direction: "forward" | "back", mode: "ios" | "md" = "ios") {
  const { baseEl, enteringEl, leavingEl } = outlet();
  const animation = pageTransition(baseEl, { enteringEl, leavingEl, direction, mode } as Opts);
  return { animation, enteringEl, leavingEl };
}

function all(animation: Animation): Animation[] {
  return [animation, ...animation.childAnimations.flatMap(all)];
}

function framesOf(animation: Animation, el: HTMLElement) {
  const moving = all(animation).find((one) => one.elements.includes(el));
  expect(moving, `an animation moves #${el.id}`).toBeDefined();
  const frames = moving!.getKeyframes() as Array<Record<string, unknown>>;
  return { transform: [frames[0].transform, frames.at(-1)!.transform], opacity: [frames[0].opacity, frames.at(-1)!.opacity] };
}

afterEach(() => {
  document.documentElement.dir = "ltr";
  document.body.innerHTML = "";
});

describe("the app's page transition", () => {
  it("lasts 300 ms with iOS's curve", () => {
    const { animation } = build("forward");
    expect(animation.getDuration()).toBe(300);
    expect(animation.getEasing()).toBe("cubic-bezier(0.32, 0.72, 0, 1)");
  });

  it("slides the entering page in from the trailing side and the leaving one a quarter out, dimmed", () => {
    const { animation, enteringEl, leavingEl } = build("forward");
    expect(framesOf(animation, enteringEl).transform).toEqual(["translateX(100%)", "translateX(0%)"]);
    expect(framesOf(animation, leavingEl)).toEqual({ transform: ["translateX(0%)", "translateX(-25%)"], opacity: [1, 0.8] });
  });

  it("on the way back, slides the page underneath in from a quarter and the leaving one out whole", () => {
    const { animation, enteringEl, leavingEl } = build("back");
    expect(framesOf(animation, enteringEl)).toEqual({ transform: ["translateX(-25%)", "translateX(0%)"], opacity: [0.8, 1] });
    expect(framesOf(animation, leavingEl).transform).toEqual(["translateX(0%)", "translateX(100%)"]);
  });

  it("mirrors itself in a right-to-left language", () => {
    document.documentElement.dir = "rtl";
    const forward = build("forward");
    expect(framesOf(forward.animation, forward.enteringEl).transform).toEqual(["translateX(-100%)", "translateX(0%)"]);
    expect(framesOf(forward.animation, forward.leavingEl).transform).toEqual(["translateX(0%)", "translateX(25%)"]);
    const back = build("back");
    expect(framesOf(back.animation, back.enteringEl).transform).toEqual(["translateX(25%)", "translateX(0%)"]);
    expect(framesOf(back.animation, back.leavingEl).transform).toEqual(["translateX(0%)", "translateX(-100%)"]);
  });

  it("is the same in Android's Material mode", () => {
    const ios = build("forward", "ios");
    const iosFrames = [framesOf(ios.animation, ios.enteringEl), framesOf(ios.animation, ios.leavingEl)];
    const md = build("forward", "md");
    expect(md.animation.getDuration()).toBe(300);
    expect([framesOf(md.animation, md.enteringEl), framesOf(md.animation, md.leavingEl)]).toEqual(iosFrames);
  });

  it("moves only the two pages, never their header or back button on their own", () => {
    for (const direction of ["forward", "back"] as const) {
      const { animation, enteringEl, leavingEl } = build(direction);
      const moved = all(animation).flatMap((one) => one.elements);
      expect(new Set(moved)).toEqual(new Set([enteringEl, leavingEl]));
    }
  });

  it("makes the entering page visible as it starts", () => {
    const { animation, enteringEl } = build("forward");
    animation.progressStart(true, 0);
    expect(enteringEl.classList.contains("ion-page-invisible")).toBe(false);
    animation.destroy();
  });
});

const settled = async (promise: Promise<void>) => {
  let done = false;
  void promise.then(() => (done = true));
  for (let at = 0; at < 5; at += 1) await Promise.resolve();
  return done;
};

describe("pages moving", () => {
  async function fresh() {
    vi.resetModules();
    const module = await import("./page-transition");
    const start = (direction: "forward" | "back") => {
      const { baseEl, enteringEl, leavingEl } = outlet();
      return module.pageTransition(baseEl, { enteringEl, leavingEl, direction, mode: "ios" } as Opts);
    };
    return { start, done: module.pageTransitionsDone };
  }

  it("are done at once when no page moves", async () => {
    const { done } = await fresh();
    expect(await settled(done())).toBe(true);
  });

  it("are done only once the transition under way has finished", async () => {
    const { start, done } = await fresh();
    const animation = start("forward");
    const waiting = done();
    expect(await settled(waiting)).toBe(false);
    await animation.play();
    expect(await settled(waiting)).toBe(true);
    expect(await settled(done())).toBe(true);
  });

  it("are not waited for past a transition's time and some, should one never end", async () => {
    const { start, done } = await fresh();
    vi.useFakeTimers();
    try {
      start("back");
      const waiting = done();
      expect(await settled(waiting)).toBe(false);
      await vi.advanceTimersByTimeAsync(1000);
      expect(await settled(waiting)).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});
