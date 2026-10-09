// Builds the Ionic the app lends to every plugin and game frame (2026-10-09, route B of the Ionic
// plan): `src-tauri/resources/ionic/ionic.js` (every component of the app's own `@ionic/core`,
// custom elements, one ES module with nothing loaded later, and the icons the core carries) and
// `ionic.css` (ionic.bundle.css but for the body rules of structure.css). The Rust side embeds both (`plugins.rs`) and its tests
// check they are the versions of `package-lock.json`. Run it after updating Ionic:
//   node scripts/build-frame-ionic.mjs
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "vite";

const app = join(dirname(fileURLToPath(import.meta.url)), "..");
const modules = join(app, "node_modules");
const out = join(app, "src-tauri/resources/ionic");
const version = (name) => JSON.parse(readFileSync(join(modules, name, "package.json"), "utf8")).version;
const ionic = version("@ionic/core");
const ionicons = version("ionicons");
const banner = `/*! @ionic/core ${ionic}, ionicons ${ionicons} (MIT, Ionic). Lent by FlickerTalk to its plugin frames; built by scripts/build-frame-ionic.mjs from the app's own packages, do not edit. */`;

// Every component, as the plugins expect (the URL router too, though a frame has one address).
const components = readdirSync(join(modules, "@ionic/core/components"))
  .filter((file) => /^ion-[a-z-]+\.js$/.test(file))
  .map((file) => file.slice(0, -3))
  .sort();

// What `ion-app` starts in an app, without `ion-app` (whose layout containment would stop an old
// plugin from scrolling): the press feedback (ripples, activated states) and the keyboard focus
// ring. Found in ion-app's own code, so a new Ionic that moves them fails here, not in a frame.
const ionApp = readFileSync(join(modules, "@ionic/core/components/ion-app.js"), "utf8");
const found = (pattern, what) => {
  const match = pattern.exec(ionApp);
  if (!match) throw new Error(`ion-app no longer says where ${what} is: look at @ionic/core/components/ion-app.js`);
  return match;
};
const [, coreChunk] = found(/import\{[^}]*\bc as (?:\w+)[^}]*\}from"\.\/(p-[\w-]+\.js)"/, "the config");
const [, configName] = found(/import\{[^}]*\bc as (\w+)[^}]*\}from"\.\/p-[\w-]+\.js"/, "the config");
const [, tapChunk] = found(new RegExp(`import\\("\\.\\/(p-[\\w-]+\\.js)"\\)\\.then\\(\\(\\w+=>\\w+\\.startTapClick\\(${configName}\\)`), "tap-click");
const [, focusChunk] = found(/import\("\.\/(p-[\w-]+\.js)"\)\.then\(\(\w+=>this\.focusVisible=\w+\.startFocusVisible\(\)/, "focus-visible");
const chunk = (file) => JSON.stringify(join(modules, "@ionic/core/components", file));

// The icons the core already lends (`resources/icons`) and the ones a plugin may name for its tile
// (`PLUGIN_ICONS`), registered by name, so `<ion-icon name="trash-outline">` draws without a fetch.
const camel = (name) => name.replace(/-([a-z0-9])/g, (_, letter) => letter.toUpperCase());
const tableSource = readFileSync(join(app, "src/plugins.ts"), "utf8");
const table = tableSource.slice(tableSource.indexOf("export const PLUGIN_ICONS"));
const icons = [
  ...new Set([
    ...readdirSync(join(app, "src-tauri/resources/icons")).filter((file) => file.endsWith(".svg")).map((file) => file.slice(0, -4)),
    ...[...table.slice(0, table.indexOf("}),")).matchAll(/"([a-z0-9-]+)":/g)].map(([, name]) => name),
  ]),
].sort();

const entry = `
import { initialize, actionSheetController, alertController, loadingController, modalController, popoverController, toastController, menuController, createAnimation, createGesture, getMode, isPlatform, getPlatforms } from "@ionic/core/components";
import { addIcons } from "ionicons/components";
import { ${icons.map(camel).join(", ")} } from "ionicons/icons";
import { c as config } from ${chunk(coreChunk)};
import { startTapClick } from ${chunk(tapChunk)};
import { startFocusVisible } from ${chunk(focusChunk)};
${components.map((name, at) => `import { defineCustomElement as c${at} } from "@ionic/core/components/${name}.js";`).join("\n")}
initialize();
${components.map((_, at) => `c${at}();`).join("\n")}
addIcons({ ${icons.map((name) => `${JSON.stringify(name)}: ${camel(name)}`).join(", ")} });
startTapClick(config);
startFocusVisible();
globalThis.ftIonic = Object.freeze({
  version: ${JSON.stringify(ionic)},
  ionicons: ${JSON.stringify(ionicons)},
  components: Object.freeze(${JSON.stringify(components)}),
  icons: Object.freeze(${JSON.stringify(icons)}),
  actionSheetController, alertController, loadingController, modalController, popoverController, toastController, menuController,
  addIcons, createAnimation, createGesture, getMode, isPlatform, getPlatforms,
});
`;

const scratch = join(app, "node_modules/.ft-frame-ionic");
mkdirSync(scratch, { recursive: true });
writeFileSync(join(scratch, "entry.js"), entry);
await build({
  configFile: false,
  root: app,
  logLevel: "warn",
  build: {
    outDir: join(scratch, "out"),
    emptyOutDir: true,
    minify: true,
    chunkSizeWarningLimit: 4096,
    // An app build of one script, not a library build: Vite leaves a library's whitespace alone.
    copyPublicDir: false,
    modulePreload: false,
    rolldownOptions: {
      input: join(scratch, "entry.js"),
      preserveEntrySignatures: false,
      output: { format: "es", entryFileNames: "ionic.js", codeSplitting: false, banner, comments: { legal: false } },
    },
  },
});
let script = readFileSync(join(scratch, "out/ionic.js"), "utf8");
if (!script.startsWith(banner)) script = `${banner}\n${script}`;
for (const banned of ["eval(", "new Function", "import(", "importScripts"]) {
  if (script.includes(banned)) throw new Error(`the bundle has ${banned}: it would not run under the frame's policy`);
}

// ionic.bundle.css without structure.css, which pins the body to the frame (`position: fixed`,
// `overflow: hidden`): a frame sized by its content (the game room) would never grow, and a plugin
// that scrolls its page would not scroll. Its first rule (border-box and no tap highlight) is kept;
// the full height of a tool's frame is the frame page's own (`html[data-fill]`).
const CSS = ["normalize", "core", "typography", "display", "padding", "float-elements", "text-alignment", "text-transformation", "flex-utils"];
const read = (name) => readFileSync(join(modules, "@ionic/core/css", `${name}.css`), "utf8").replace(/\/\*# sourceMappingURL=[^*]*\*\/\s*$/, "").trim();
const structure = read("structure");
const everything = /^\*\{[^}]*\}/.exec(structure);
if (!everything) throw new Error("structure.css no longer starts with its * rule: look at it again");
const css = [banner.replace("/*!", "/*"), ...CSS.map(read), everything[0]].join("\n");

mkdirSync(out, { recursive: true });
writeFileSync(join(out, "ionic.js"), script);
writeFileSync(join(out, "ionic.css"), `${css}\n`);
rmSync(scratch, { recursive: true, force: true });
console.log(`ionic.js ${script.length} bytes (${components.length} components, ${icons.length} icons), ionic.css ${css.length} bytes, @ionic/core ${ionic}, ionicons ${ionicons}`);
