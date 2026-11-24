// Game lists as plain text, for the clipboard: one title per line, so you can paste
// what you have anywhere (an AI chat, a note, a message).
import type { CollectionDetail, GameRow, PlayStatus } from "./types";
import { playOf } from "./play";

const byTitle = (a: string, b: string) => a.localeCompare(b, undefined, { sensitivity: "base", numeric: true });

/** What you've marked about a game, e.g. " (Finished, 9/10, ★)"; empty when nothing is */
export function marks(status: PlayStatus | null, score: number | null, favorite: boolean): string {
  const m = [playOf(status)?.label, score != null ? `${score}/10` : null, favorite ? "★" : null].filter(Boolean);
  return m.length ? ` (${m.join(", ")})` : "";
}

/** Titles A–Z; grouped under a console heading when there's more than one console */
export function gamesText(rows: GameRow[], systemName: (id: string) => string): string {
  const groups = new Map<string, GameRow[]>();
  for (const r of rows) (groups.get(r.system_id) ?? groups.set(r.system_id, []).get(r.system_id)!).push(r);
  const line = (r: GameRow) => r.title + marks(r.play, r.score, r.favorite);
  const sorted = (rs: GameRow[]) => [...rs].sort((a, b) => byTitle(a.title, b.title)).map(line);
  if (groups.size <= 1) return sorted(rows).join("\n");
  return [...groups]
    .map(([id, rs]) => [systemName(id), rs] as const)
    .sort((a, b) => byTitle(a[0], b[0]))
    .map(([name, rs]) => [name, ...sorted(rs)].join("\n"))
    .join("\n\n");
}

/** The collection's name, then its games in play order, with the console when it spans several, and your notes */
export function collectionText(c: CollectionDetail, systemName: (id: string) => string): string {
  const mixed = new Set(c.items.map((i) => i.system)).size > 1;
  const lines = c.items.map((i) =>
    i.title + (mixed ? ` — ${systemName(i.system)}` : "") +
    marks(i.play?.status ?? null, i.play?.score ?? null, i.play?.favorite ?? false) +
    (i.note ? ` — ${i.note}` : ""));
  return [c.name, "", ...lines].join("\n");
}
