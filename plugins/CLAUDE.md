# plugins/

Plugins web de FlickerTalk (`Plan.md §48–57`). Se **desarrollan aquí, fuera de la app**, y se
**instalan desde la app** como paquetes `.ftplugin` firmados. Fase 6, fuera del MVP (`§86`,
`§95`).

## Organización

- Un plugin = una carpeta `plugins/<nombre>/` **autocontenida** (su propio `package.json`, build
  y `module.json`). Cualquier plugin puede pasar a su propio repo si hace falta, así que ninguno
  importa por ruta relativa nada de `app/` ni de otro plugin.
- El contrato con la app es `app/packages/plugin-sdk`, consumido como dependencia versionada,
  junto con el formato de paquete y `minCoreVersion` que define `app/crates/ft-plugins`.
- Primeros candidatos (`§57`): Markdown Renderer, Poll, Stickers, Themes, Syntax Highlighting,
  Translator, Code Snippets.
- El catálogo `plugins.flickertalk.com` (`§56`: casi estático, `index.json` + paquetes, sin
  cuentas, HTTPS + paquete firmado) aún no tiene carpeta asignada.

## Paquete `.ftplugin` (`§49–50`)

`module.json` (`id`, `name`, `version`, `minCoreVersion`, `components`), `dist/index.js`,
`dist/style.css`, `assets/`, `signature`. La app lo instala así: descarga → BLAKE3 → verificar
firma Ed25519 → validar manifest → instalar → registrar el Web Component.

## Reglas

- Solo HTML/CSS/JS/Web Components; nunca código nativo (`§48`).
- Sin red por defecto, sin secretos y con permisos mínimos declarados en el manifest
  (`§53–55`).
- Deben funcionar dentro del sandbox limitado de iOS (`§52`): renderers, temas, presentación de
  mensajes, transformaciones locales y comandos sobre datos dados explícitamente.
- Si un plugin necesita una capacidad nativa nueva, se añade al Core y se sube `minCoreVersion`;
  nunca se mete en el plugin (`§51`).
