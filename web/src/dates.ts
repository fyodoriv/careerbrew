// Friendly day labels for headings (comfort budget, goal 2026-10-22): "Mon Oct 19", never an ISO string.
// A "YYYY-MM-DD" is a calendar day: read it in UTC, so the browser's time zone never shifts it by one.
const at = (day: string) => new Date(`${day}T00:00:00Z`);
const fmt = (day: string, o: Intl.DateTimeFormatOptions) => at(day).toLocaleDateString("en-US", { ...o, timeZone: "UTC" });

export const shortDay = (day: string): string =>
  /^\d{4}-\d{2}-\d{2}$/.test(day) ? fmt(day, { weekday: "short", month: "short", day: "numeric" }).replace(",", "") : day;

// With the year, for a day that may be in another year (a goal's date).
export const fullDay = (day: string): string =>
  /^\d{4}-\d{2}-\d{2}$/.test(day) ? fmt(day, { weekday: "short", month: "short", day: "numeric", year: "numeric" }).replace(/^(\w+),/, "$1") : day;

// A day in body text: "Tue Oct 20" this year, "Sun Jan 17, 2027" in another (the browser's clock decides the year).
export const friendlyDay = (day: string): string => {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(day) || Number.isNaN(at(day).getTime())) return day;  // "2026-13-45" stays as written
  return Number(day.slice(0, 4)) === new Date().getFullYear() ? shortDay(day) : fullDay(day);
};

export const longDay = (day: string): string =>
  /^\d{4}-\d{2}-\d{2}$/.test(day) ? `${fmt(day, { weekday: "long" })}, ${fmt(day, { month: "short", day: "numeric" })}` : day;
