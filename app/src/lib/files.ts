// Display helpers for lists of library files.

/** "…/a.part2.rar" and "…/a.part1.rar" are one archive set. */
export function setKey(path: string): string { return path.replace(/\.part\d+\.rar$/i, ".rar"); }

/** The path below the system folder: "Fire Emblem [id]/x.nsp", or "x.nsp" for a loose file. */
export function inSystem(path: string, roots: string[]): string {
  for (const r of roots) {
    const root = r.replace(/[\\/]+$/, "");
    if (path.startsWith(root + "/") || path.startsWith(root + "\\")) {
      const rest = path.slice(root.length + 1);
      const i = rest.search(/[\\/]/);
      return i < 0 ? rest : rest.slice(i + 1);
    }
  }
  return path;
}

/** The folder part of `inSystem`, or "" for a file loose in the system folder. */
export function folderIn(path: string, roots: string[]): string {
  const rel = inSystem(path, roots);
  const i = Math.max(rel.lastIndexOf("/"), rel.lastIndexOf("\\"));
  return i < 0 ? "" : rel.slice(0, i);
}

export interface Row<T> { key: string; name: string; parts: number; size: number; items: T[] }

/** Collapse the parts of multi-part archives into one row each, keeping order. */
export function collapse<T>(items: T[], path: (t: T) => string, size: (t: T) => number): Row<T>[] {
  const rows: Row<T>[] = [];
  const at = new Map<string, Row<T>>();
  for (const it of items) {
    const p = path(it);
    const k = setKey(p);
    const r = at.get(k);
    if (r) { r.parts++; r.size += size(it); r.items.push(it); continue; }
    const name = (p.split(/[\\/]/).pop() ?? p);
    const row = { key: k, name, parts: 1, size: size(it), items: [it] };
    at.set(k, row);
    rows.push(row);
  }
  for (const r of rows) if (r.parts > 1) r.name = (setKey(r.items[0] ? path(r.items[0]) : r.name).split(/[\\/]/).pop() ?? r.name);
  return rows;
}

/** The system folder a path is in ("Sony - PSP"), or "" outside the library. */
export function systemFolder(path: string, roots: string[]): string {
  for (const r of roots) {
    const root = r.replace(/[\\/]+$/, "");
    if (path.startsWith(root + "/") || path.startsWith(root + "\\")) return path.slice(root.length + 1).split(/[\\/]/)[0] ?? "";
  }
  return "";
}

/** Where a file or folder sits, short: "Sony - PSP › Games", or "Sony - PSP" when loose. */
export function placeOf(path: string, roots: string[]): string {
  const sys = systemFolder(path, roots);
  if (!sys) { const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")); return i < 0 ? "" : path.slice(0, i); }
  const folder = folderIn(path, roots);
  return folder ? `${sys} › ${folder.split(/[\\/]/).join(" › ")}` : sys;
}
