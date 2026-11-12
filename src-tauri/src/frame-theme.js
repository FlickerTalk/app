// Without these a dark app shows white calendars and borderless alerts inside its plugins.

const root = document.documentElement;
const style = document.createElement("style");
style.id = "ft-ionic-theme";

const clamp = (channel) => Math.max(0, Math.min(255, channel));

// Takes the same formats frame.js's COLOUR check lets through (hex, rgb(a), hsl(a)).
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

const mix = (base, other, amount) => base.map((channel, at) => clamp(Math.round(channel + (other[at] - channel) * amount)));
const rgb = (colour) => `rgb(${colour.join(", ")})`;
const numbers = (colour) => colour.join(", ");
const BLACK = [0, 0, 0];
const WHITE = [255, 255, 255];

// YIQ, the way Ionic's colour generator picks a contrast colour.
const contrastOf = (colour) => ((colour[0] * 299 + colour[1] * 587 + colour[2] * 114) / 1000 >= 128 ? "#000000" : "#ffffff");

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
    // Ionic 8 and 9 read the first two names, Ionic 7 components the last one.
    for (let step = 50; step <= 950; step += 50) {
      const towardsText = rgb(mix(page, text, step / 1000));
      out[`--ion-background-color-step-${step}`] = towardsText;
      out[`--ion-text-color-step-${step}`] = rgb(mix(text, page, step / 1000));
      out[`--ion-color-step-${step}`] = towardsText;
    }
    out["--ion-toolbar-background"] = rgb(page);
    out["--ion-toolbar-color"] = rgb(text);
  }

  for (const name of ["primary", "success", "danger", "medium"]) {
    const colour = parse(read(`--ion-color-${name}`));
    if (!colour) continue;
    const given = name === "primary" && parse(read("--ion-color-primary-contrast")) ? read("--ion-color-primary-contrast") : undefined;
    complete(out, name, colour, given ? { contrast: given, given: true } : {});
  }

  const light = surface ?? (page && text ? mix(page, text, 0.1) : null);
  if (light && text) complete(out, "light", light, { contrast: rgb(text), own: false });
  if (text && page) complete(out, "dark", text, { contrast: rgb(page), own: false });

  if (surface) out["--ion-card-background"] = rgb(surface);
  if (border) out["--ion-item-border-color"] = border;

  // In the dark an overlay sits on the surface colour with a heavier backdrop, like the app's own
  // sheets. On a black page it would have no edge at all.
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
  // Last in the head so these win over Ionic's own defaults.
  if (document.head.lastElementChild !== style) document.head.append(style);
}

const observer = new MutationObserver(write);
observer.observe(root, { attributes: true, attributeFilter: ["style", "data-dark"] });
write();
