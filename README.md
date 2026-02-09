# Symposium

Symposium is a Rust desktop app for playing virtual instruments from your keyboard, starting with piano.

It is inspired by browser virtual pianos, but aimed at structured practice and performance scoring.

## Project Ambition

Symposium is being built to support:

- real-time instrument play from configurable keyboard mappings
- playable song charts with tempo-aware timing
- score mode (accuracy + streak + grading)
- timed/challenge modes
- ergonomic mapping optimization per song:
  - avoids same-finger conflicts
  - minimizes awkward reaches
  - supports multi-key chords without relying on shift-heavy combos
- instrument profiles so multiple instruments can be added later without changing core gameplay

## What Works Today

- desktop GUI app (`iced`)
- playable keyboard piano in a native window
- realistic piano synthesis through SoundFont (`SF2`) rendering via `rustysynth`
- song library loaded from `res/songs/*.toml`
- song preview playback honoring note timing, tempo, duration, and velocity
- rich tracing logs to console and rolling files

## Quick Start

```bash
cargo run --release
```

To use a custom config:

```bash
SYMPOSIUM_CONFIG=path/to/symposium.toml cargo run --release
```

## SoundFont Setup

Bundled by default:

- `res/soundfonts/piano.sf2`
- license/attribution: `res/soundfonts/piano.sf2.LICENSE`

If this file is missing, Symposium tries common Linux paths:

- `/usr/share/sounds/sf2/FluidR3_GM.sf2`
- `/usr/share/sounds/sf2/TimGM6mb.sf2`
- `/usr/share/sounds/sf2/default-GM.sf2`
- `/usr/share/soundfonts/FluidR3_GM.sf2`
- `/usr/share/sf2/FluidR3_GM.sf2`

You can replace the bundled file with any compatible SF2 and adjust bank/preset in config.

## Controls (Default)

- Piano notes: `a w s e d f t g y h u j k o l p ;`
- Quit: `esc` or `ctrl+c`
- Next song: `f1`
- Binding summary hint: `f2`
- Play selected song preview: `f5`

## Configuration

Main runtime config:

- `config/symposium.toml`

Key audio settings:

- `audio.instrument`: active profile key
- `audio.master_volume`: global output gain
- `audio.note_duration_ms`: default keypress hold length
- `audio.release_duration_ms`: release tail rendered after note-off
- `audio.instrument_profiles.<name>`: per-instrument profile

Example profile:

```toml
[audio]
instrument = "piano"
master_volume = 0.22
note_duration_ms = 680
release_duration_ms = 720
sample_rate_hz = 48000

[audio.instrument_profiles.piano]
engine = "soundfont"
soundfont_path = "res/soundfonts/piano.sf2"
bank = 0
preset = 0
channel = 0
maximum_polyphony = 128
enable_reverb_and_chorus = true
instrument_gain_multiplier = 1.0
```

## Song Format

Songs are TOML files in `res/songs/` with schema validation from:

- `res/songs/schema/song.schema.json`

Song files include:

- metadata (`id`, `title`, `artist`, `tempo_bpm`, difficulty, tags, etc.)
- sections (`start_beats`, `end_beats`, loop flags)
- timed events with:
  - `at_beats`
  - `duration_beats`
  - `notes` (MIDI note list/chords)
  - optional `velocity`
  - optional hand metadata/lyrics/accent flags

## Repository Layout

- `src/main.rs`: GUI state/update/view and keyboard routing
- `src/audio.rs`: SoundFont rendering + playback scheduling
- `src/input.rs`: key chord parsing and normalized bindings
- `src/config.rs`: config model, defaults, validation, load/create
- `src/songs.rs`: song model + loader/validator
- `config/symposium.toml`: runtime configuration
- `res/songs/`: song data + schema
- `res/soundfonts/`: local SoundFont assets

## Logging

Tracing is enabled throughout startup, input handling, audio rendering, and song playback.

- console logs: live run diagnostics
- file logs: `logs/` (rolling appender)

## Development

```bash
cargo fmt
cargo check
cargo test
```

## Roadmap

1. Score engine with timing windows and per-note grading
2. Timed challenge mode and fail/pass logic
3. Song playback overlays (guide notes / visual timeline)
4. Per-song instrument + binding presets
5. Ergonomic optimizer that auto-generates chord-friendly mappings
6. Additional instruments (strings, pads, synths) through profile expansion
