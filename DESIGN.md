# Design

The app is called **Index**.

The visual world is **Braun, by way of Dieter Rams**: less, but better. The app is a working tool (Operate mode); the world lends type, palette, density and one signature move, and every control stays a standard one.

## Palette (tokens in `app/src/app.css`)

- **Ground and panels:** warm housing white and grey in light mode (`--plastic`, `--panel`, `--panel-2`), Braun black in dark mode (T 1000, ET 66). The theme follows the system setting.
- **Ink:** near-black (light) / warm off-white (dark), with `--ink-soft` and `--ink-faint` for secondary text; all text meets 4.5:1.
- **Signal orange (`--accent`)** — the ET 66's "=" key. Used only for: the one action that does the job in an area (`.btn.primary`, the drawer's Open folder), the current selection, focus rings, text selection, and the live "Playing" lamp. Never decoration, never more than one primary key per area.
- **`--signal` (ochre)** — your own marks: favourite stars and the chosen score.
- **State colours** (`--ok`, `--warn`, `--bad`, `--info`) appear as indicator lamps and short state text, not as filled backgrounds.
- **Maker colours** only as small dots beside system names.

## Signature move: indicator lamps

Status is a small round lamp beside a plain label (`.chip`, `.gpill`), like the pilot lights on Braun radios and hi-fi. `.chip.plain` is a bordered label tag without a lamp. Pressed-in keys with a lit lamp show a chosen state (play status in the drawer).

## Type

One family: **IBM Plex Sans** (400/500/600, italic for quoted reviews), self-hosted via `@fontsource/ibm-plex-sans`. Base 14px; headings 22 / 17 / 14.5; tabular numerals everywhere. Section labels are sentence case, 13–13.5px, semibold, `--ink-soft` — no uppercase eyebrows.

## Controls

Buttons are keys: flat face, hairline border, a lower edge one gentle step darker than the border (`--key-edge`, never a black line), 4px radius, a 1px press on click. Inputs share the same border and radius. Icons are drawn (`components/Icon.svelte`: 16px grid, 1.6px round stroke), never text glyphs.

## Layout and density

Lists and cards are dense and aligned: one card per group, one row per file (lamp, name, one line of facts, optional note). No nested cards, no coloured side borders, no gradient text, no decorative shadows; the only shadow is `--shadow-pop` on popovers and dialogs. Photography (system photos, game art) keeps its own colour behind a dark scrim where text sits on it.

## Activity panel

Background work lives in the bar along the bottom: what's running (the mark at work while something runs, a red lamp when the last job failed) and, at the right, the **Activity** tab. Opening it slides the panel up from behind the bar (part of the layout, under the page — it pushes the page up rather than floating, so a seam, not a shadow), and the tab rides the panel's top edge to stand on it like a folder tab, in the panel's colour with no line between them. The tab and Esc close it; its top edge drags to resize (remembered per PC).

## Mark and app icon

A Braun-black tile with nine round keys in a 3×3 grid; the bottom-right key is signal orange, like the "=" key on the ET 66 — the one entry you picked out of the index. Source: `app/src-tauri/icons/icon.svg` (every platform size is generated from it with `npx tauri icon`); in the app, `components/Mark.svelte`. It holds up to 16px; don't add a wordmark inside the tile.

## Window frame

The system title bar is off (`"decorations": false`). `components/TitleBar.svelte` draws a 34px frame: the left part continues the sidebar (mark and name), the rest drags the window (double-click maximizes), and three plain window keys sit on the right — close lights `--bad` on hover, never the orange. Invisible 4px edges resize the window while it isn't maximized.

## The mark at work

`components/MarkAnim.svelte`: a soft wave crosses the keypad on the diagonal, from the top-left key to the orange home key, which takes it with a small press and rebound and settles — every pass lands on the still mark. Keys on one diagonal move together and each eases down and back while the next starts, so it reads as one ripple; only transform and opacity animate. It's drawn without the icon's housing: the startup screen (`Splash.svelte`, the bare keys on the theme's ground colour, fading out when the app is ready; clicking the mark in the title bar plays two passes and the rest after them) and, at 14px, the jobs bar's sign that something is running. With reduced motion it's the still mark.
