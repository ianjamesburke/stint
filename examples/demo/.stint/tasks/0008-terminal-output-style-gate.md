---
id: "0008"
title: "Decide on Unicode vs ASCII symbols for weather rendering"
status: backlog
tags:
  - "gate"
area:
  - "cli"
---

## Ask

Should weather conditions render with colorful Unicode glyphs (☀️, 🌧️) or plain ASCII characters (*, ~) across all terminals?

## Options

1. (Recommended) Render Unicode glyphs by default, but automatically fall back to ASCII if stdout is not a UTF-8 capable TTY or if `--ascii` flag is passed.
2. Strict ASCII only for universal compatibility across minimal terminals and pipe redirection.
3. Strict Unicode emojis only without fallback.

## Good answer

Specifies whether to adopt option 1 or option 2, confirming the CLI flag name (`--ascii` vs `--no-unicode`) and environment variable check (`TERM=dumb` or `NO_COLOR`).

## Test

Run `weather London` and verify output renders crisp glyphs; run `weather London --ascii` and verify ASCII-safe rendering.

## Evidence

Testing across modern terminal emulators (Ghostty, iTerm2, Alacritty, Windows Terminal) showed full UTF-8 emoji support. CI logs and non-UTF-8 SSH sessions render question mark replacement boxes when emojis are forced.

## Links

- Blocks: task 0005 (`0005-render-forecast.md`)
- Terminal capability check crate: `console` or `supports-unicode`

## Decision

