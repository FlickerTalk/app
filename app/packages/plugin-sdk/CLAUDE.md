# app/packages/plugin-sdk

SDK para autores de plugins (`§95`): la superficie de la **FlickerTalk Plugin API** que un
plugin puede usar desde JavaScript. Fase 6, fuera del MVP.

## Reglas

- El SDK solo expone llamadas que pasan por el chequeo de permisos del core
  (`app/crates/ft-plugins`); nunca envuelve `invoke` de Tauri directamente (`§58`).
- No da acceso a claves, push token, red arbitraria ni APIs nativas (`§52–55`).
- El formato de paquete, la firma y `minCoreVersion` los define `app/crates/ft-plugins`; el SDK se
  mantiene alineado con él.
- Los plugins viven fuera de la app (`plugins/`, o en repos propios) y consumen este SDK como
  dependencia versionada: un cambio incompatible rompe plugins publicados.
