/**
 * The separator to show before each message of a thread, given when each was sent (ms, 0 when
 * unknown), in thread order: the name of its local calendar day when it starts a new day,
 * `undefined` otherwise. Today is called `today`; any other day is its date in `locale`, with the
 * year only when it is not this year's.
 */
export function dayLabels(sentAt: readonly number[], now: number, locale: string, today: string): Array<string | undefined> {
  const thisDay = new Date(now).toDateString();
  const thisYear = new Date(now).getFullYear();
  let previous = "";
  return sentAt.map((ms) => {
    if (!ms) return undefined;
    const date = new Date(ms);
    const day = date.toDateString();
    if (day === previous) return undefined;
    previous = day;
    if (day === thisDay) return today;
    const year = date.getFullYear() === thisYear ? undefined : "numeric";
    return new Intl.DateTimeFormat(locale, { day: "numeric", month: "long", year }).format(date);
  });
}
