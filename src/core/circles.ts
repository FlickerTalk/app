import { invoke } from "@tauri-apps/api/core";
import { refreshChats } from "./sync";

export async function sendCircleText(id: string, text: string): Promise<string> {
  return invoke<string>("core_circle_send", { circle: id, text });
}

// nothing travels, a circle tells nobody when you read
export async function markCircleRead(id: string): Promise<void> {
  await invoke("core_circle_mark_read", { circle: id });
}

export async function createCircle(name: string, members: string[], session?: string): Promise<string> {
  const id = await invoke<string>("core_circle_create", { name: name.trim(), members, session });
  await refreshChats();
  return id;
}

export async function inviteToCircle(id: string, contact: string): Promise<void> {
  await invoke("core_circle_invite", { circle: id, contact });
  await refreshChats();
}

export async function removeFromCircle(id: string, contact: string): Promise<void> {
  await invoke("core_circle_remove", { circle: id, contact });
  await refreshChats();
}

export async function setCircleAdmin(id: string, contact: string, admin: boolean): Promise<void> {
  await invoke("core_circle_set_admin", { circle: id, contact, admin });
  await refreshChats();
}

export async function renameCircle(id: string, name: string): Promise<void> {
  await invoke("core_circle_rename", { circle: id, name: name.trim() });
  await refreshChats();
}

export async function setCircleAdminsOnly(id: string, adminsOnly: boolean): Promise<void> {
  await invoke("core_circle_admins_only", { circle: id, adminsOnly });
  await refreshChats();
}

export async function leaveCircle(id: string): Promise<void> {
  await invoke("core_circle_leave", { circle: id });
  await refreshChats();
}

export async function forgetCircle(id: string): Promise<void> {
  await invoke("core_circle_forget", { circle: id });
  await refreshChats();
}
