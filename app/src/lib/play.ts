// Play status: names, colours, and what counts as done.
import type { PlayInfo, PlayStatus } from "./types";

export const PLAY: { id: PlayStatus; label: string; short: string; tone: string }[] = [
  { id: "want", label: "Want to play", short: "Want", tone: "info" },
  { id: "playing", label: "Playing", short: "Playing", tone: "accent" },
  { id: "finished", label: "Finished", short: "Finished", tone: "ok" },
  { id: "completed", label: "100% completed", short: "100%", tone: "ok" },
  { id: "onhold", label: "On hold", short: "On hold", tone: "warn" },
  { id: "dropped", label: "Dropped", short: "Dropped", tone: "bad" },
  { id: "skip", label: "Skip", short: "Skip", tone: "plain" },
];
export const playOf = (s: PlayStatus | null | undefined) => PLAY.find((p) => p.id === s) ?? null;
export const isDone = (s: PlayStatus | null | undefined) => s === "finished" || s === "completed";
export const isOut = (s: PlayStatus | null | undefined) => s === "skip" || s === "dropped";
export const emptyPlay = (): PlayInfo => ({ status: null, score: null, notes: null, started: null, finished: null, hours: null, favorite: false });
