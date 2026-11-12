// What a match's console verdict means, in words.
import type { MetaCandidate } from "./types";

export function consoleNote(c: Pick<MetaCandidate, "console" | "platforms">, systemName: string): { text: string; tone: string } {
  if (c.console === "match") return { text: systemName, tone: "ok" };
  const p = c.platforms ?? [];
  if (c.console === "other") return { text: `Released on ${p.slice(0, 3).join(", ")}${p.length > 3 ? "…" : ""} — not ${systemName}`, tone: "bad" };
  return { text: "Console not checked", tone: "" };
}
