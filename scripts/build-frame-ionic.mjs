// Rerun after updating Ionic, the Rust tests check the versions.
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

const components = readdirSync(join(modules, "@ionic/core/components"))
  .filter((file) => /^ion-[a-z-]+\.js$/.test(file))
  .map((file) => file.slice(0, -3))
  .sort();

// ion-app's layout containment stops old plugins from scrolling, so start its tap and focus helpers without it.
// Their chunks are found in ion-app's code, so an Ionic update that moves them fails here.
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

// registered by name so <ion-icon name="..."> never fetches
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
    // app build, not lib, Vite leaves a library's whitespace alone
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

// no structure.css, its fixed body would stop frames from growing or scrolling
const CSS = ["normalize", "core", "typography", "display", "padding", "float-elements", "text-alignment", "text-transformation", "flex-utils"];
const read = (name) => readFileSync(join(modules, "@ionic/core/css", `${name}.css`), "utf8").replace(/\/\*# sourceMappingURL=[^*]*\*\/\s*$/, "").trim();
const structure = read("structure");
const everything = /^\*\{[^}]*\}/.exec(structure);
if (!everything) throw new Error("structure.css no longer starts with its * rule: look at it again");
const css = [banner.replace("/*!", "/*"), ...CSS.map(read), everything[0]].join("\n");

mkdirSync(out, { recursive: true });
writeFileSync(join(out, "ionic.js"), script);
writeFileSync(join(out, "ionic.css"), `${css}\n`);
// both are MIT, the notices go with the copy
const notice = (name, at) => `${name} ${at}\n\n${readFileSync(join(modules, name, "LICENSE"), "utf8").trimEnd()}\n`;
writeFileSync(join(out, "LICENSE"), [notice("@ionic/core", ionic), notice("ionicons", ionicons)].join("\n"));
rmSync(scratch, { recursive: true, force: true });
console.log(`ionic.js ${script.length} bytes (${components.length} components, ${icons.length} icons), ionic.css ${css.length} bytes, @ionic/core ${ionic}, ionicons ${ionicons}`);
