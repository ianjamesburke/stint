---
id: "0002"
title: "Config file loader"
status: todo
priority: p2
size: s
estimate: "2h"
sprint: "s1"
blocked_by:
  - "0001"
area:
  - "config"
tags:
  - "io"
---

Read `~/.config/weather/config.toml` for the default location and units.

## Why

Needs the crate scaffold (0001) in place before there is anywhere to load
config into. Blocked until 0001 is done.

## Done When

- [ ] `WeatherConfig` struct parses TOML with location and temperature units
- [ ] Missing config file falls back gracefully to system defaults
- [ ] Unit tests cover missing, valid, and malformed config scenarios
