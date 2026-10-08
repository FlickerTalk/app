import { createAnimation, getIonPageElement, type Animation, type TransitionOptions } from "@ionic/vue";

/** How long a page takes to come in or go (ms). Ionic's own: 540 on iOS, 280/200 on Android. */
export const DURATION = 300;
/** iOS's curve: quick out of the tap, gentle at the end. Ionic's swipe back assumes it too. */
export const EASING = "cubic-bezier(0.32, 0.72, 0, 1)";
/** How far the page underneath moves, and how dim it gets, while the other slides over it. */
const UNDER = "25%";
const DIM = 0.8;

/**
 * The app's page transition (2026-10-09), the same on iOS and Android: the entering page slides in
 * from the side, whole, over the one it covers, which moves a quarter away and dims; going back is
 * the reverse. Unlike Ionic's iOS transition, nothing inside a page (header, title, back button)
 * moves or fades on its own, so the back button is on screen, opaque, from the first frame.
 * Right-to-left languages mirror it. Ionic plays it for every push and pop of an outlet; a swipe
 * back (iOS) drives it step by step; `root` navigations and tab switches are not animated.
 */
export function pageTransition(baseEl: HTMLElement, opts: TransitionOptions): Animation {
  const rtl = (baseEl?.ownerDocument ?? document).documentElement.dir === "rtl";
  const side = (percent: string) => (rtl ? `translateX(-${percent})` : `translateX(${percent})`);
  const under = (percent: string) => (rtl ? `translateX(${percent})` : `translateX(-${percent})`);
  const back = opts.direction === "back";

  const root = createAnimation()
    .duration(opts.duration || DURATION)
    .easing(EASING)
    .fill("both");

  const entering = createAnimation()
    .addElement(getIonPageElement(opts.enteringEl))
    .beforeRemoveClass("ion-page-invisible");
  if (back) entering.beforeClearStyles(["opacity"]).fromTo("transform", under(UNDER), "translateX(0%)").fromTo("opacity", DIM, 1);
  else entering.fromTo("transform", side("100%"), "translateX(0%)");
  root.addAnimation(entering);

  if (opts.leavingEl) {
    const leavingPage = getIonPageElement(opts.leavingEl) as HTMLElement;
    const leaving = createAnimation().addElement(leavingPage);
    if (back) {
      leaving.beforeClearStyles(["opacity"]).fromTo("transform", "translateX(0%)", side("100%"));
      // A page left behind stays out of the way once gone, as Ionic's own transitions leave it.
      root.afterAddWrite(() => {
        if (root.getDirection() === "normal") leavingPage.style.setProperty("display", "none");
      });
    } else {
      leaving.fromTo("transform", "translateX(0%)", under(UNDER)).fromTo("opacity", 1, DIM);
    }
    root.addAnimation(leaving);
  }
  return root;
}
