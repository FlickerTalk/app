// Builds the Ionic the app lends to every plugin and game frame (2026-10-09, route B of the Ionic
// plan): `src-tauri/resources/ionic/ionic.js` (every component of the app's own `@ionic/core`,
// custom elements, one ES module with nothing loaded later) and `ionic.css` (Ionic's global styles
// without normalize.css and structure.css). The Rust side embeds both (`plugins.rs`) and its tests
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

// The URL router has nothing to route in a frame with one address; everything else is lent.
const NOT_LENT = new Set(["ion-router", "ion-route", "ion-route-redirect", "ion-router-link", "ion-router-outlet"]);
const components = readdirSync(join(modules, "@ionic/core/components"))
  .filter((file) => /^ion-[a-z-]+\.js$/.test(file))
  .map((file) => file.slice(0, -3))
  .filter((name) => !NOT_LENT.has(name))
  .sort();

const entry = `
import { initialize, actionSheetController, alertController, loadingController, modalController, popoverController, toastController, menuController, createAnimation, createGesture, getMode, isPlatform, getPlatforms } from "@ionic/core/components";
import { addIcons } from "ionicons/components";
${components.map((name, at) => `import { defineCustomElement as c${at} } from "@ionic/core/components/${name}.js";`).join("\n")}
initialize();
${components.map((_, at) => `c${at}();`).join("\n")}
globalThis.ftIonic = Object.freeze({
  version: ${JSON.stringify(ionic)},
  ionicons: ${JSON.stringify(ionicons)},
  components: Object.freeze(${JSON.stringify(components)}),
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

const CSS = ["core", "typography", "display", "padding", "float-elements", "text-alignment", "text-transformation", "flex-utils"];
const css = [
  banner.replace("/*!", "/*"),
  ...CSS.map((name) => readFileSync(join(modules, "@ionic/core/css", `${name}.css`), "utf8").replace(/\/\*# sourceMappingURL=[^*]*\*\/\s*$/, "").trim()),
].join("\n");

mkdirSync(out, { recursive: true });
writeFileSync(join(out, "ionic.js"), script);
writeFileSync(join(out, "ionic.css"), `${css}\n`);
rmSync(scratch, { recursive: true, force: true });
console.log(`ionic.js ${script.length} bytes (${components.length} components), ionic.css ${css.length} bytes, @ionic/core ${ionic}, ionicons ${ionicons}`);
