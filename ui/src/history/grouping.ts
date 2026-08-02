/**
 * Date bucketing for the history list.
 *
 * Kept out of the component because it is the only part of History with rules
 * worth reading twice: three different group titles, two nested sort orders,
 * and a calendar comparison that must use the viewer's local midnight rather
 * than a 24-hour window (WIN-006, WIN-007).
 */

import type { TranscriptEntry } from "../lib/ipc";

export interface HistoryGroup {
  /** `Today`, `Yesterday`, or a `MMM d` label such as `Mar 4` — no year. */
  title: string;
  /** Newest first. */
  entries: TranscriptEntry[];
}

/**
 * Locale-aware, but constructed once: `Intl.DateTimeFormat` is expensive
 * enough that building one per row is visible on a long list.
 */
const monthDay = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });

/** Row metadata renders a short local time such as `3:42 PM` (WIN-009). */
export const shortTime = new Intl.DateTimeFormat(undefined, { timeStyle: "short" });

/**
 * Bucket entries by local calendar day, newest group first and newest entry
 * first within each group.
 *
 * `now` is injectable so the Today/Yesterday boundary can be exercised without
 * waiting for midnight.
 */
export function groupByDay(
  entries: readonly TranscriptEntry[],
  now: Date = new Date(),
): HistoryGroup[] {
  // Local midnight, so an entry from 23:50 last night is "Yesterday" even
  // though it is well under 24 hours old.
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const oneDay = 86_400_000;

  const buckets = new Map<number, HistoryGroup>();
  for (const entry of entries) {
    const at = new Date(entry.timestamp * 1000);
    const midnight = new Date(at.getFullYear(), at.getMonth(), at.getDate()).getTime();

    let group = buckets.get(midnight);
    if (group === undefined) {
      const title =
        midnight === today
          ? "Today"
          : midnight === today - oneDay
            ? "Yesterday"
            : monthDay.format(at);
      group = { title, entries: [] };
      buckets.set(midnight, group);
    }
    group.entries.push(entry);
  }

  for (const group of buckets.values()) {
    group.entries.sort((a, b) => b.timestamp - a.timestamp);
  }

  return [...buckets.entries()].sort(([a], [b]) => b - a).map(([, group]) => group);
}
