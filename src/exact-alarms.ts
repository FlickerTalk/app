/**
 * Reminders that ring on time (2026-10-13). A plugin's reminder for Friday at 10:00 must ring at
 * 10:00, but Android 12 and up give the app exact alarms only if the user allows them (Android 14
 * denies them by default); without them the phone may ring it minutes late. When the user grants a
 * plugin reminders and the phone does not allow exact alarms, the app says so once and offers the
 * system screen with its switch. Never on each reminder. iOS and a desktop never ask.
 */
import { alertController } from "@ionic/vue";
import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";

/** Whether reminders ring on time; a phone that cannot say is not asked about it. */
async function exactAlarmsAllowed(): Promise<boolean> {
  return invoke<boolean>("core_exact_alarms").then(
    (allowed) => allowed !== false,
    () => true,
  );
}

/** Opens the system screen where the user allows exact alarms. */
function requestExactAlarms(): void {
  void invoke("core_request_exact_alarms").catch(() => undefined);
}

/**
 * Right after the user grants a plugin reminders: if they would ring late, one alert says why and
 * offers the phone's settings. It ends once the user has answered, and never fails.
 */
export async function offerExactAlarms(): Promise<void> {
  if (await exactAlarmsAllowed()) return;
  try {
    const alert = await alertController.create({
      message: t("plugins.exactAlarms.message"),
      buttons: [
        { text: t("plugins.exactAlarms.notNow"), role: "cancel" },
        { text: t("plugins.exactAlarms.openSettings"), handler: requestExactAlarms },
      ],
    });
    await alert.present();
    await alert.onDidDismiss();
  } catch {
    // No alert, no question: the reminder still rings, maybe a little late.
  }
}
