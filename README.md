# Symposium

Symposium is a Rust project for building playable virtual instruments with a strong focus on keyboard ergonomics.

The long-term target is a Virtual Piano-style experience with:

- real-time keyboard piano play
- playable song sheets
- scoring and timed challenge modes
- fully configurable keybindings
- per-song ergonomic keybinding optimization that minimizes finger conflicts and awkward chords

## Current status (MVP)

This repo now includes a working terminal piano:

- press mapped keyboard keys to play notes immediately
- configurable bindings loaded from TOML
- starter song catalog loaded from TOML
- control chords for quitting and listing songs/bindings
- extensive tracing to terminal and rolling log files

## Run

1. Install Rust (nightly toolchain is configured by this repo).
2. Run:

```bash
cargo run
```

By default, config is loaded from `config/symposium.toml`.

You can override config path:

```bash
SYMPOSIUM_CONFIG=path/to/your-config.toml cargo run
```

## Controls (default)

- Piano keys: `a w s e d f t g y h u j k o l p ;`
- Quit: `esc` or `ctrl+c`
- List songs: `f1`
- Print active bindings: `f2`

## Configuration

Main config file: `config/symposium.toml`

High-value sections:

- `[audio]`: waveform, note duration, sample rate, master volume
- `[input]`: repeat handling and shift normalization behavior
- `[control_bindings]`: runtime control chords
- `[keybindings]`: key chord -> MIDI note mapping
- `[[songs]]`: starter song metadata and notation strings

Example mapping:

```toml
[keybindings]
"a" = 60 # C4
"w" = 61 # C#4
"s" = 62 # D4
```

Chord syntax supports modifiers:

- `ctrl+c`
- `shift+a`
- `alt+f4`
- `f1`

## Logging and tracing

Symposium uses `tracing` + `tracing-subscriber` with:

- console logging
- daily rolling file logs in `logs/`

Default log filter is configured in TOML (`logging.level`) and can be overridden with `RUST_LOG`.

Example:

```bash
RUST_LOG=symposium=trace cargo run
```

## Architecture

- `src/main.rs`: runtime orchestration, terminal event loop, control handling
- `src/config.rs`: TOML model, defaults, load/create, validation
- `src/input.rs`: key chord parsing + normalized event mapping + binding compilation
- `src/audio.rs`: low-latency note playback using `rodio`

## Ambition roadmap

1. Move from terminal MVP to GUI piano interface (iced/wgpu) with visible keys and chord highlights.
2. Add song player/practice views with progress tracking.
3. Implement score mode (accuracy + timing + combo/streak).
4. Implement timed challenge mode.
5. Add per-song binding profiles and one-click profile switching.
6. Build ergonomic optimizer:
   - model fingers, hand zones, and stretch limits
   - minimize same-finger collisions and shift-heavy chords
   - optimize mapping for each song’s chord/timing graph
7. Expand from piano to additional virtual instruments.

## Development

```bash
cargo fmt
cargo check
```

If your terminal/audio backend differs, tune `audio.sample_rate_hz`, `audio.master_volume`, and `audio.note_duration_ms` in `config/symposium.toml`.
