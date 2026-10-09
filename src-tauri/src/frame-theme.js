// The theme Ionic reads inside a plugin's frame (2026-10-09, route B of the Ionic plan). The app
// hands the frame nine colours and whether it is dark (`frame.js` puts them on the root); Ionic's
// components read many more: a colour as numbers (`-rgb`), the steps between the page and the
// text, the shade, tint and contrast of each colour, light and dark, the overlays. Without them a
// dark app shows white calendars and borderless alerts in its plugins. They are derived here, from
// those nine only, in a style of their own that follows the root whenever it changes. Nothing of
// the app reaches the plugin this way that the nine colours did not already say.

const root = document.documentElement;
const style = document.createElement("style");
style.id = "ft-ionic-theme";

const clamp = (channel) => Math.max(0, Math.min(255, channel));

/** A colour as [r, g, b] (0–255), or null: hex, rgb()/rgba() and hsl()/hsla(), as `frame.js` lets through. */
function parse(text) {
  const value = String(text ?? "").trim().toLowerCase();
  const hex = /^#([0-9a-f]{3,8})$/.exec(value);
  if (hex) {
    const digits = hex[1];
    if (digits.length === 3 || digits.length === 4) return [0, 1, 2].map((at) => parseInt(digits[at] + digits[at], 16));
    if (digits.length === 6 || digits.length === 8) return [0, 2, 4].map((at) => parseInt(digits.slice(at, at + 2), 16));
    return null;
  }
  const call = /^(rgba?|hsla?)\(([^)]*)\)$/.exec(value);
  if (!call) return null;
  const parts = call[2].split(/[\s,/]+/).filter(Boolean);
  if (parts.length < 3) return null;
  if (call[1].startsWith("rgb")) {
    const channels = parts.slice(0, 3).map((part) => (part.endsWith("%") ? (parseFloat(part) * 255) / 100 : parseFloat(part)));
    return channels.every(Number.isFinite) ? channels.map((one) => clamp(Math.round(one))) : null;
  }
  const hue = parseFloat(parts[0]);
  const saturation = parseFloat(parts[1]) / 100;
  const lightness = parseFloat(parts[2]) / 100;
  if (![hue, saturation, lightness].every(Number.isFinite)) return null;
  const chroma = (1 - Math.abs(2 * lightness - 1)) * saturation;
  const at = (n) => {
    const k = (n + hue / 30) % 12;
    return lightness - (chroma / 2) * Math.max(-1, Math.min(k - 3, 9 - k, 1));
  };
  return [at(0), at(8), at(4)].map((one) => clamp(Math.round(one * 255)));
}

/** `amount` (0–1) of `other` into `base`, as `color-mix(in srgb, other amount, base)`. */
const mix = (base, other, amount) => base.map((channel, at) => clamp(Math.round(channel + (other[at] - channel) * amount)));
const rgb = (colour) => `rgb(${colour.join(", ")})`;
const numbers = (colour) => colour.join(", ");
const BLACK = [0, 0, 0];
const WHITE = [255, 255, 255];

/** Dark text on a bright colour and white on a deep one: Ionic's colour generator (YIQ). */
const contrastOf = (colour) => ((colour[0] * 299 + colour[1] * 587 + colour[2] * 114) / 1000 >= 128 ? "#000000" : "#ffffff");

/**
 * What Ionic reads for one named colour: its numbers, shade, tint and contrast. `contrast` is the
 * one the app gave or chose (written only if it was not given); `own` says the colour itself is
 * the app's, already on the root.
 */
function complete(out, name, colour, { contrast, given = false, own = true }) {
  if (!own) out[`--ion-color-${name}`] = rgb(colour);
  out[`--ion-color-${name}-rgb`] = numbers(colour);
  out[`--ion-color-${name}-shade`] = rgb(mix(colour, BLACK, 0.12));
  out[`--ion-color-${name}-tint`] = rgb(mix(colour, WHITE, 0.1));
  const chosen = contrast ?? contrastOf(colour);
  if (!given) out[`--ion-color-${name}-contrast`] = chosen;
  const parsed = parse(chosen);
  if (parsed) out[`--ion-color-${name}-contrast-rgb`] = numbers(parsed);
}

/** Everything derived from what the root says now. */
function derive() {
  const read = (name) => root.style.getPropertyValue(name).trim();
  const dark = root.dataset.dark === "1";
  const page = parse(read("--ion-background-color"));
  const text = parse(read("--ion-text-color"));
  const surface = parse(read("--ion-item-background"));
  const border = read("--ion-border-color");
  const out = {};

  if (page) out["--ion-background-color-rgb"] = numbers(page);
  if (text) out["--ion-text-color-rgb"] = numbers(text);
  if (page && text) {
    // Ionic 8 and 9 read the steps by the first two names; components of Ionic 7 by the last.
    for (let step = 50; step <= 950; step += 50) {
      const towardsText = rgb(mix(page, text, step / 1000));
      out[`--ion-background-color-step-${step}`] = towardsText;
      out[`--ion-text-color-step-${step}`] = rgb(mix(text, page, step / 1000));
      out[`--ion-color-step-${step}`] = towardsText;
    }
    // The bars as the app's: on the page, in its text.
    out["--ion-toolbar-background"] = rgb(page);
    out["--ion-toolbar-color"] = rgb(text);
  }

  for (const name of ["primary", "success", "danger", "medium"]) {
    const colour = parse(read(`--ion-color-${name}`));
    if (!colour) continue;
    const given = name === "primary" && parse(read("--ion-color-primary-contrast")) ? read("--ion-color-primary-contrast") : undefined;
    complete(out, name, colour, given ? { contrast: given, given: true } : {});
  }

  // Light is the surface of a card (as in the app), dark the text, each with the other as contrast.
  const light = surface ?? (page && text ? mix(page, text, 0.1) : null);
  if (light && text) complete(out, "light", light, { contrast: rgb(text), own: false });
  if (text && page) complete(out, "dark", text, { contrast: rgb(page), own: false });

  if (surface) out["--ion-card-background"] = rgb(surface);
  if (border) out["--ion-item-border-color"] = border;

  // In the dark an overlay is lifted onto the surface and dims more behind it, as the app's own
  // sheets do (`variables.css`): on a black page it would have no edge at all.
  if (dark && light) {
    out["--ion-overlay-background-color"] = rgb(light);
    out["--ion-backdrop-opacity"] = "0.6";
  }
  return out;
}

let written;
function write() {
  const css = `:root{${Object.entries(derive())
    .map(([name, value]) => `${name}: ${value}`)
    .join(";")}}`;
  if (css !== written) {
    written = css;
    style.textContent = css;
  }
  // Last in the head, after Ionic's own defaults, so these win over them.
  if (document.head.lastElementChild !== style) document.head.append(style);
}

const observer = new MutationObserver(write);
observer.observe(root, { attributes: true, attributeFilter: ["style", "data-dark"] });
write();
