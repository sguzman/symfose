# Symposium

Symposium is a Rust virtual-instrument project focused on playable keyboard piano + song practice with ergonomic mapping.

It is inspired by browser-based virtual pianos, but extends them with:

- per-song scoring (planned)
- timed challenge modes (planned)
- fully configurable keybindings
- ergonomics-aware mapping optimization (planned)

## What Works Now

- GUI desktop app using `iced` (winit + wgpu under the hood)
- keyboard-playable piano in a real window
- piano-model synthesis (non-sine default)
- configurable keybindings via TOML
- song library loaded from `res/songs/*.toml`
- song preview playback with timing + tempo + velocity
- tracing logs to console and rolling files in `logs/`

## Run

```bash
cargo run --release
```

Config path override:

```bash
SYMPOSIUM_CONFIG=path/to/config.toml cargo run --release
```

## Controls (Default)

- Piano: `a w s e d f t g y h u j k o l p ;`
- Quit: `esc` or `ctrl+c`
- Next song: `f1`
- Binding summary hint: `f2`
- Play selected song: `f5`

## Project Layout

- `src/main.rs`: GUI app state/update/view and keyboard routing
- `src/audio.rs`: audio output + piano-model note rendering + song playback scheduling
- `src/input.rs`: key-chord parsing and runtime chord normalization
- `src/config.rs`: app config model, defaults, validation, load/create
- `src/songs.rs`: song file model and loader/validator
- `config/symposium.toml`: runtime config
- `res/songs/*.toml`: song data files
- `res/songs/schema/song.schema.json`: song TOML schema

## Song Format

Songs are separate TOML files under `res/songs/`.

Each song supports:

- metadata (`id`, `title`, `tempo_bpm`, time signature, difficulty, tags, etc.)
- optional sections (`start_beats`, `end_beats`, looping flags)
- timed events with beat offsets, durations, chords (`notes = [midi...]`), velocity, and hand metadata

This is rich enough to drive:

- guided playback
- score/timing evaluation
- per-song ergonomic optimization

## Song Schema

Schema file:

- `res/songs/schema/song.schema.json`

Taplo config is wired so files in `res/songs/*.toml` validate against this schema.

## Audio

Default instrument is `piano_model`, a physically-inspired synthesized piano voice with:

- fast hammer attack
- multi-string detune
- harmonic decay shaping
- release tail

You can still switch to basic oscillators in config (`sine`, `triangle`, `square`, `sawtooth`) for debugging.

## Configuration

Main config file:

- `config/symposium.toml`

Important sections:

- `[audio]`: instrument, sample rate, volume, note duration
- `[input]`: key repeat and shift behavior
- `[control_bindings]`: global command chords
- `[keybindings]`: key chord -> MIDI mapping
- `[song_library]`: song directory + schema path

## Development

```bash
cargo fmt
cargo check
cargo test
```

## Roadmap

1. Accurate score engine (timing windows, streaks, accuracy categories)
2. Timed mode and challenge variants
3. On-screen sheet + falling note visualizations
4. Per-song binding profiles
5. Ergonomic optimizer that reduces same-finger collisions and awkward stretches
6. SoundFont-backed acoustic instruments
