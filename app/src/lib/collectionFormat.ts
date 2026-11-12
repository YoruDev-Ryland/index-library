// The collection file format, and a prompt that teaches an AI to write it (see docs/collection-format.md).
export const EXAMPLE = {
  format: "index-collection",
  version: 1,
  name: "Atelier, in release order",
  notes: "Arland first, then Dusk, then the Mysterious trilogy.",
  games: [
    { title: "Atelier Rorona: The Alchemist of Arland DX", system: "Switch", note: "Start here" },
    { title: "Atelier Totori: The Adventurer of Arland DX", system: "Switch" },
    { title: "Atelier Ayesha: The Alchemist of Dusk DX", system: "Switch", note: "Dusk trilogy begins" },
    { title: "Atelier Shallie: Alchemists of the Dusk Sea DX", system: "Switch", note: "Optional — weaker reviews" },
  ],
};

export function aiPrompt(systems: [string, string][], request = ""): string {
  return `I keep my game library in an app called Index. Please make me a game collection as JSON in exactly this format, and reply with only the JSON:

${JSON.stringify(EXAMPLE, null, 2)}

Rules:
- "games" is in the order I should play them.
- "title" is the game's official English name as released on that system.
- "system" is one of: ${systems.map(([id, name]) => `${name} (${id})`).join(", ")}.
- "note" is optional: a few words on why it's in the list, where it fits, or why to skip it.
- Add "title_id" only if you are sure of the exact Switch/3DS title id or disc serial.
- Add "region" (USA, Europe, Japan…) only when a specific release matters.
- Don't invent games. If unsure whether a game exists on a system, leave it out.

The collection I want: ${request || "[describe it here: a series, a genre, a system, a mood, a reason…]"}`;
}
