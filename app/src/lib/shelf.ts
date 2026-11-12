import type { Condition, CopyFields, ShelfCopy } from "./types";

export const CONDITIONS: { id: Condition; label: string }[] = [
  { id: "mint", label: "Mint" }, { id: "near_mint", label: "Near mint" }, { id: "very_good", label: "Very good" },
  { id: "good", label: "Good" }, { id: "fair", label: "Fair" }, { id: "poor", label: "Poor" },
];
export const conditionLabel = (c: Condition | null) => CONDITIONS.find((x) => x.id === c)?.label ?? null;

export const EDITIONS = ["Player's Choice", "Greatest Hits", "Platinum", "Nintendo Selects", "Classics", "Black label", "Limited Edition", "Collector's Edition", "Launch edition", "Bundle copy", "Rental"];

/** What a copy is, by its parts: Sealed, CIB, Boxed, Loose, Box only, Manual only. */
export function partsLabel(c: Pick<CopyFields, "sealed" | "cart" | "box" | "manual">): string {
  if (c.sealed) return "Sealed";
  if (c.cart && c.box && c.manual) return "CIB";
  if (c.cart && c.box) return "Boxed";
  if (c.cart) return "Loose";
  if (c.box) return "Box only";
  if (c.manual) return "Manual only";
  return "Loose";
}

export const blankCopy = (): CopyFields => ({ edition: null, cart: true, box: false, manual: false, inserts: false, sealed: false, condition: null, location: null, price_cents: null, bought: null, notes: null });
export const fieldsOf = (c: ShelfCopy): CopyFields => ({ edition: c.edition, cart: c.cart, box: c.box, manual: c.manual, inserts: c.inserts, sealed: c.sealed, condition: c.condition, location: c.location, price_cents: c.price_cents, bought: c.bought, notes: c.notes });

export function money(cents: number | null, currency: string | undefined): string {
  if (cents == null) return "";
  try { return new Intl.NumberFormat(undefined, { style: "currency", currency: currency || "USD" }).format(cents / 100); }
  catch { return `${(cents / 100).toFixed(2)} ${currency ?? ""}`.trim(); }
}
