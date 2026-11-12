// Android 12+ rings on time only with exact alarms, which the user must allow (14 denies them by default)
import { alertController } from "@ionic/vue";
import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";

async function exactAlarmsAllowed(): Promise<boolean> {
  return invoke<boolean>("core_exact_alarms").then(
    (allowed) => allowed !== false,
    () => true,
  );
}

function requestExactAlarms(): void {
  void invoke("core_request_exact_alarms").catch(() => undefined);
}

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
    // the reminder still rings, maybe a little late
  }
}
