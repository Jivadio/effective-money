// SPDX-License-Identifier: AGPL-3.0-or-later
// Dates in the UI, always formatted by the user's locale.
//
// Booking dates are calendar days (`YYYY-MM-DD`) with no time zone: they
// are formatted in UTC so a day never shifts. Timestamps (RFC 3339) are
// shown in the browser's time zone.

const DAY_MS = 86_400_000;

function pad(n: number): string {
  return String(n).padStart(2, '0');
}

/** Today's local calendar day as `YYYY-MM-DD`. */
export function todayIso(now: Date = new Date()): string {
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

function dayToUtc(iso: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso);
  if (!match) return null;
  const [, y, m, d] = match;
  const date = new Date(Date.UTC(Number(y), Number(m) - 1, Number(d)));
  date.setUTCFullYear(Number(y));
  return date;
}

function capitalize(text: string, locale: string): string {
  return text.charAt(0).toLocaleUpperCase(locale) + text.slice(1);
}

/**
 * A calendar day: "Today", "Yesterday", then a short date ("Oct 28",
 * "28 oct."), with the year only when it is not the current one.
 */
export function formatDay(
  iso: string,
  locale: string,
  now: Date = new Date(),
): string {
  const day = dayToUtc(iso);
  const today = dayToUtc(todayIso(now));
  if (!day || !today) return iso;
  const diff = Math.round((day.getTime() - today.getTime()) / DAY_MS);
  if (diff === 0 || diff === -1) {
    const relative = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
    return capitalize(relative.format(diff, 'day'), locale);
  }
  return new Intl.DateTimeFormat(locale, {
    day: 'numeric',
    month: 'short',
    year:
      day.getUTCFullYear() === today.getUTCFullYear() ? undefined : 'numeric',
    timeZone: 'UTC',
  }).format(day);
}

/** A moment (RFC 3339) as a medium date and short time. */
export function formatDateTime(timestamp: string, locale: string): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return timestamp;
  return new Intl.DateTimeFormat(locale, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(date);
}
