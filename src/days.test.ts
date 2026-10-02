import { describe, expect, it } from "vitest";

// A thread used to show one fixed «Today» above every message, so a conversation from 29 September
// read «Today» on 2 October (seen on real phones, 2026-10-02). Each local calendar day now gets its
// own separator. Times are built in local time, so the tests hold in any time zone.
import { dayLabels } from "./days";

const at = (month: number, day: number, hours: number, minutes = 0, year = 2026) =>
  new Date(year, month - 1, day, hours, minutes).getTime();

const NOW = at(10, 2, 12);

describe("day separators in a thread", () => {
  it("shows none in an empty thread", () => {
    expect(dayLabels([], NOW, "en", "Today")).toEqual([]);
  });

  it("puts one «Today» before the first message of today", () => {
    expect(dayLabels([at(10, 2, 9), at(10, 2, 9, 5), at(10, 2, 11)], NOW, "en", "Today")).toEqual([
      "Today",
      undefined,
      undefined,
    ]);
  });

  it("names an earlier day by its date and today as today", () => {
    expect(dayLabels([at(9, 29, 21, 24), at(9, 29, 21, 30), at(10, 2, 8)], NOW, "en", "Today")).toEqual([
      "September 29",
      undefined,
      "Today",
    ]);
  });

  it("writes the date in the language of the app", () => {
    expect(dayLabels([at(9, 29, 21, 24), at(10, 2, 8)], NOW, "es", "Hoy")).toEqual(["29 de septiembre", "Hoy"]);
  });

  it("adds the year only to a day of another year", () => {
    expect(dayLabels([at(12, 31, 23, 0, 2025), at(1, 5, 10)], NOW, "en", "Today")).toEqual([
      "December 31, 2025",
      "January 5",
    ]);
  });

  it("starts a new day at local midnight", () => {
    expect(dayLabels([at(9, 30, 23, 59), at(10, 1, 0, 1)], NOW, "en", "Today")).toEqual(["September 30", "October 1"]);
  });

  it("follows the thread's order when a day comes back", () => {
    expect(dayLabels([at(9, 29, 10), at(10, 2, 8), at(9, 29, 11)], NOW, "en", "Today")).toEqual([
      "September 29",
      "Today",
      "September 29",
    ]);
  });

  // `clock` shows no time for 0: a message without a known time joins the day before it.
  it("gives a message with no known time no separator of its own", () => {
    expect(dayLabels([at(10, 2, 9), 0, at(10, 2, 10)], NOW, "en", "Today")).toEqual(["Today", undefined, undefined]);
  });
});
