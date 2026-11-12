# Collection files

Index can export a collection as a small JSON file and import one back. The format
is simple enough to write by hand, and to ask an AI to write for you ("make me a collection of
the Atelier games in release order, skipping the weak ones").

## Format

```json
{
  "format": "index-collection",
  "version": 1,
  "name": "Atelier, in release order",
  "notes": "Arland first, then Dusk, then the Mysterious trilogy.",
  "games": [
    { "title": "Atelier Rorona: The Alchemist of Arland DX", "system": "Switch", "note": "Start here" },
    { "title": "Atelier Totori: The Adventurer of Arland DX", "system": "Switch" },
    { "title": "Atelier Meruru: The Apprentice of Arland DX", "system": "Switch" },
    { "title": "Atelier Ayesha: The Alchemist of Dusk DX", "system": "Switch", "note": "Dusk trilogy begins" },
    { "title": "Atelier Shallie: Alchemists of the Dusk Sea DX", "system": "Switch", "note": "Optional — weaker reviews" }
  ]
}
```

| Field | Required | Meaning |
|---|---|---|
| `name` | yes | The collection's name. |
| `notes` | no | Anything about the collection as a whole. |
| `games` | yes | The games, **in play order**. |
| `games[].title` | yes | The game's name. Punctuation, accents and ™ don't matter. |
| `games[].system` | recommended | The console: the app's id (`n3ds`), its name (`Nintendo 3DS`) or a common short name (`3DS`, `GBA`, `Switch`, `PS1`, `Mega Drive`…). |
| `games[].note` | no | Why it's here or where it fits ("skip — weak reviews"). |
| `games[].title_id` | no | A Switch/3DS title id, PSP/Vita content id or disc serial, if known. Matches exactly. |
| `games[].region` | no | Preferred release (`USA`, `Europe`, `Japan`…) when the library has several. |

`format` and `version` are written on export and optional on import. Text around the JSON (such
as an AI's explanation, or a ```json fence) is ignored.

## Importing

Library → Collections → **＋ → Import collection…**: paste the JSON or open a file. Before anything
is created, the preview shows how each game matched your library:

- **Exact** — by title id or serial.
- **Same title** — the same name on that system (your preferred region first; other regions are offered).
- **Close title** — a near-identical name; check it, or pick another.
- **Not in your library** — kept in the collection by name, and found automatically once the game is added.

## Asking an AI

The import window's **Copy AI prompt** copies a ready-made prompt with this format and the list of
systems. Paste it into any AI, add what you want ("…all the mainline Castlevania games on GBA and
DS, in story order"), and paste its reply back into the import window.
