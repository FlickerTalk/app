import { computed, ref, watch, type Ref } from "vue";
import { useRouter } from "vue-router";
import { grantPlugin, installPlugin, PREMIUM_PAGE, type ChatMessage, type PluginView } from "./core";
import { installed, isLocked, offered, offeredOnce, pluginIcon, pluginImage, pluginName, refreshPlugins, refreshPremiumLock, tools } from "./plugins";
import { gameGrant, gameIdFromText, gameUrl, isGame, needsGameGrant } from "./games";
import { t } from "./i18n";
import type { Opening, OpenPlugin } from "./pluginWindow";

export type AppsTab = "tools" | "games";

export function useChatApps(options: {
  messages: () => ChatMessage[];
  play: () => string | undefined;
  draft: Ref<string>;
  error: Ref<string>;
  plugin: Ref<OpenPlugin | null>;
  openPlugin: (chosen: Opening) => void;
}) {
  const router = useRouter();
  const showApps = ref(false);

  function toPremium() {
    void router.push(PREMIUM_PAGE);
  }

  function useApp(id: string) {
    const chosen = installed.value.find((one) => one.id === id);
    if (!chosen) return;
    showApps.value = false;
    if (isLocked(chosen)) return toPremium();
    options.openPlugin({ id: chosen.id, name: pluginName(chosen), sending: chosen.granted.send, live: Boolean(chosen.granted.live) });
  }

  const appsTab = ref<AppsTab>("tools");
  const asking = ref<{ id: string; name: string; size?: number; icon?: string; image?: string } | null>(null);

  function openApps() {
    void Promise.all([refreshPlugins(), refreshPremiumLock()]);
    appsTab.value = tools.value.length ? "tools" : "games";
    showApps.value = true;
  }

  function openGame(game: PluginView) {
    options.openPlugin({ id: game.id, name: pluginName(game), sending: game.granted.send, live: Boolean(game.granted.live), game: true });
  }

  function inviteToPlaying() {
    const game = installed.value.find((one) => one.id === options.plugin.value?.id);
    if (game) invite(game);
  }

  // only games the signed catalogue lists, never a tool
  function playGame(id: string) {
    showApps.value = false;
    options.error.value = "";
    const here = installed.value.find((one) => one.id === id && isGame(one));
    if (here) {
      if (needsGameGrant(here)) asking.value = { id, name: pluginName(here), icon: pluginIcon(here), image: pluginImage(here) };
      else openGame(here);
      return;
    }
    const listed = offered.value.find((one) => one.id === id && isGame(one));
    if (listed) asking.value = { id, name: pluginName(listed), size: listed.size, icon: pluginIcon(listed), image: pluginImage(listed) };
  }

  async function allowGame() {
    const ask = asking.value;
    asking.value = null;
    if (!ask) return;
    if (ask.size !== undefined) {
      try {
        await installPlugin(ask.id);
      } catch {
        options.error.value = t("games.installFailed");
        return;
      }
    }
    await refreshPlugins();
    let game = installed.value.find((one) => one.id === ask.id && isGame(one));
    if (game && needsGameGrant(game)) {
      await grantPlugin(game.id, gameGrant(game)).catch(() => undefined);
      await refreshPlugins();
      game = installed.value.find((one) => one.id === ask.id && isGame(one));
    }
    if (game && !needsGameGrant(game)) openGame(game);
  }

  function invite(game: PluginView) {
    const url = gameUrl(game.id);
    showApps.value = false;
    if (!url) return;
    // the receiver's Play button names the game in their own language
    const text = t("games.inviteText", { game: pluginName(game), url });
    options.draft.value = options.draft.value.trim() ? `${options.draft.value.trimEnd()} ${text}` : text;
  }

  function moreGames() {
    showApps.value = false;
    void router.push("/tabs/apps?show=games");
  }

  // opening a chat alone fetches nothing from the catalogue
  const pluginsLoaded = ref(false);
  const invitedToUnknown = computed(
    () =>
      pluginsLoaded.value &&
      options.messages().some((message) => {
        if (message.kind === "file") return false;
        const id = gameIdFromText(message.text ?? "");
        return Boolean(id) && !installed.value.some((one) => one.id === id && isGame(one));
      }),
  );
  watch(invitedToUnknown, (now) => {
    if (now) void offeredOnce();
  });

  // the page drops ?play= from the address as soon as it hands it over
  let toPlay = options.play();
  function loaded() {
    pluginsLoaded.value = true;
    if (toPlay) playGame(toPlay);
    toPlay = undefined;
  }
  watch(options.play, (id) => {
    if (!id) return;
    if (pluginsLoaded.value) playGame(id);
    else toPlay = id;
  });

  return {
    showApps,
    appsTab,
    asking,
    toPremium,
    useApp,
    openApps,
    inviteToPlaying,
    playGame,
    allowGame,
    invite,
    moreGames,
    loaded,
  };
}
