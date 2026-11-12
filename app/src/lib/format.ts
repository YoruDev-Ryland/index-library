export function bytes(n: number): string {
  if (n < 1e3) return `${n} B`;
  const u = ["KB", "MB", "GB", "TB"];
  let v = n / 1e3, i = 0;
  while (v >= 1000 && i < u.length - 1) { v /= 1000; i++; }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${u[i]}`;
}
export function day(ts: number): string {
  return new Date(ts * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}
export function relDays(ts: number): string {
  const d = Math.round((ts * 1000 - Date.now()) / 86_400_000);
  if (d === 0) return "today";
  return d > 0 ? `in ${d} day${d === 1 ? "" : "s"}` : `${-d} day${d === -1 ? "" : "s"} ago`;
}
export function basename(p: string): string { return p.split(/[\\/]/).pop() ?? p; }
export function dirname(p: string): string { const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\")); return i < 0 ? "" : p.slice(0, i); }
export function errText(e: unknown): string { return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e); }

/** Maker colour for a system id — the coloured tab on each cartridge spine. */
export function makerOf(id: string): { maker: string; color: string } {
  if (["n3ds", "nds", "gb", "gba", "gbc", "gamecube", "nes", "snes", "switch", "virtualboy", "gbahacks"].includes(id)) return { maker: "Nintendo", color: "var(--maker-nintendo)" };
  if (["ps1", "psp", "psvita"].includes(id)) return { maker: "Sony", color: "var(--maker-sony)" };
  if (["sega32x", "gamegear", "genesis", "sms", "sg1000"].includes(id)) return { maker: "Sega", color: "var(--maker-sega)" };
  if (id.startsWith("atari") || id === "lynx") return { maker: "Atari", color: "var(--maker-atari)" };
  if (id === "neogeo" || id === "ngpc") return { maker: "SNK", color: "var(--maker-snk)" };
  if (id === "pce") return { maker: "NEC", color: "var(--maker-nec)" };
  return { maker: "Other", color: "var(--maker-other)" };
}
