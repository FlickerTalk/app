import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke, convertFileSrc: (path: string) => path }));

import {
  createCircle,
  forgetCircle,
  inviteToCircle,
  leaveCircle,
  markCircleRead,
  removeFromCircle,
  renameCircle,
  sendCircleText,
  setCircleAdmin,
  setCircleAdminsOnly,
} from "./circles";

const commands = () => tauri.invoke.mock.calls.map(([command]) => command);

describe("circles", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "core_conversations" ? [] : command === "core_circle_create" ? "circle2" : undefined),
    );
  });

  it("creates a circle with a trimmed name, in a session or the main list, and then reloads the lists", async () => {
    expect(await createCircle(" Friends ", ["ft_bob"], "s1")).toBe("circle2");
    expect(tauri.invoke.mock.calls[0]).toEqual(["core_circle_create", { name: "Friends", members: ["ft_bob"], session: "s1" }]);
    expect(commands()[1]).toBe("core_conversations");
    tauri.invoke.mockClear();
    await createCircle("Family", ["ft_carol"]);
    expect(tauri.invoke.mock.calls[0]).toEqual(["core_circle_create", { name: "Family", members: ["ft_carol"], session: undefined }]);
  });

  it("changes members, admins, name and who may write through the core, and reloads the lists after each", async () => {
    const changes: [() => Promise<void>, string, Record<string, unknown>][] = [
      [() => inviteToCircle("circle1", "ft_carol"), "core_circle_invite", { circle: "circle1", contact: "ft_carol" }],
      [() => removeFromCircle("circle1", "ft_carol"), "core_circle_remove", { circle: "circle1", contact: "ft_carol" }],
      [() => setCircleAdmin("circle1", "ft_bob", true), "core_circle_set_admin", { circle: "circle1", contact: "ft_bob", admin: true }],
      [() => renameCircle("circle1", " Family "), "core_circle_rename", { circle: "circle1", name: "Family" }],
      [() => setCircleAdminsOnly("circle1", true), "core_circle_admins_only", { circle: "circle1", adminsOnly: true }],
      [() => leaveCircle("circle1"), "core_circle_leave", { circle: "circle1" }],
      [() => forgetCircle("circle1"), "core_circle_forget", { circle: "circle1" }],
    ];
    for (const [change, command, args] of changes) {
      tauri.invoke.mockClear();
      await change();
      expect(tauri.invoke.mock.calls[0]).toEqual([command, args]);
      expect(commands()[1]).toBe("core_conversations");
    }
  });

  it("sends a text and marks the circle read without reloading the lists", async () => {
    tauri.invoke.mockResolvedValue("c9");
    expect(await sendCircleText("circle1", "yes")).toBe("c9");
    await markCircleRead("circle1");
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_circle_send", { circle: "circle1", text: "yes" }],
      ["core_circle_mark_read", { circle: "circle1" }],
    ]);
  });
});
