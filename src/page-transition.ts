import { createAnimation, getIonPageElement, type Animation, type TransitionOptions } from "@ionic/vue";

// ms. Ionic's own are 540 on iOS, 280/200 on Android
export const DURATION = 300;
// iOS curve, Ionic's swipe back assumes it
export const EASING = "cubic-bezier(0.32, 0.72, 0, 1)";
const UNDER = "25%";
const DIM = 0.8;

// unlike Ionic's iOS transition nothing inside the page moves on its own, so the back button is solid from the first frame
export function pageTransition(baseEl: HTMLElement, opts: TransitionOptions): Animation {
  const root = build(baseEl, opts);
  follow(root);
  return root;
}

const moving = new Set<() => void>();
let waiting: Array<() => void> = [];

// Ionic sometimes drops an animation unplayed
function follow(root: Animation): void {
  const over = () => {
    clearTimeout(cap);
    if (!moving.delete(over) || moving.size) return;
    const woken = waiting;
    waiting = [];
    woken.forEach((wake) => wake());
  };
  const cap = setTimeout(over, (root.getDuration() || DURATION) * 3);
  moving.add(over);
  root.onFinish(over, { oneTimeCallback: true });
}

// going back while a push was still animating left the page hidden when the push ended
export function pageTransitionsDone(): Promise<void> {
  if (!moving.size) return Promise.resolve();
  return new Promise((resolve) => waiting.push(resolve));
}

function build(baseEl: HTMLElement, opts: TransitionOptions): Animation {
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
