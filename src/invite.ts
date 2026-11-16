import { isPlatform } from "@ionic/vue";
import { invite, pickContact, store, type PickedContact } from "./core";
import { t } from "./i18n";

export function canInvite(): boolean {
  return isPlatform("ios") || isPlatform("android");
}

export function inviteText(name: string, me: string, link: string): string {
  const friend = name.trim();
  const mine = me.trim();
  if (friend && mine) return t("addContact.inviteMessageToFrom", { name: friend, me: mine, link });
  if (friend) return t("addContact.inviteMessageTo", { name: friend, link });
  if (mine) return t("addContact.inviteMessageFrom", { me: mine, link });
  return t("addContact.inviteMessage", { link });
}

const NOBODY_PICKED: PickedContact = { name: "", whatsapp: null };

// the picked name and number are used for this message only, never kept
export async function inviteFriend(link: string): Promise<void> {
  const friend = await pickContact().catch(() => NOBODY_PICKED);
  if (!friend) return;
  await invite(friend.whatsapp, inviteText(friend.name, store.me.name, link)).catch(() => undefined);
}
