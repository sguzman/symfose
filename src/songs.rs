use std::collections::hash_map::DefaultHasher;
use std::collections::{
  BTreeMap,
  HashMap
};
use std::fs;
use std::hash::{
  Hash,
  Hasher
};
use std::path::{
  Path,
  PathBuf
};
use std::time::UNIX_EPOCH;

use anyhow::{
  Context,
  Result,
  bail
};
use midly::{
  MetaMessage,
  MidiMessage,
  Smf,
  Timing,
  TrackEventKind
};
use serde::{
  Deserialize,
  Serialize
};
use tracing::{
  debug,
  info,
  warn
};

use crate::config::SongLibraryConfig;

const SONG_CACHE_VERSION: u16 = 1;

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct SongFile {
  pub version:  u16,
  pub schema:   String,
  pub meta:     SongMetadata,
  pub sections: Vec<SongSection>,
  pub events:   Vec<SongEvent>
}

impl Default for SongFile {
  fn default() -> Self {
    Self {
      version:  1,
      schema:   "res/songs/schema/\
                 song.schema.json"
        .to_string(),
      meta:     SongMetadata::default(),
      sections: Vec::new(),
      events:   Vec::new()
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct SongMetadata {
  pub id:               String,
  pub title:            String,
  pub artist:           String,
  pub composer:         String,
  pub arranger:         String,
  pub description:      String,
  pub difficulty:       u8,
  pub tempo_bpm:        f32,
  pub beats_per_bar:    u8,
  pub beat_unit:        u8,
  pub key_signature:    String,
  pub tags:             Vec<String>,
  pub source_url:       String,
  pub sort_order:       i32,
  pub default_velocity: u8
}

impl Default for SongMetadata {
  fn default() -> Self {
    Self {
      id:               "untitled"
        .to_string(),
      title:            "Untitled"
        .to_string(),
      artist:           String::new(),
      composer:         String::new(),
      arranger:         String::new(),
      description:      String::new(),
      difficulty:       1,
      tempo_bpm:        120.0,
      beats_per_bar:    4,
      beat_unit:        4,
      key_signature:    "C major"
        .to_string(),
      tags:             Vec::new(),
      source_url:       String::new(),
      sort_order:       0,
      default_velocity: 96
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct SongSection {
  pub id:          String,
  pub label:       String,
  pub start_beats: f32,
  pub end_beats:   f32,
  pub looped:      bool
}

impl Default for SongSection {
  fn default() -> Self {
    Self {
      id:          String::new(),
      label:       String::new(),
      start_beats: 0.0,
      end_beats:   0.0,
      looped:      false
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Hand {
  Left,
  Right,
  Both
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct SongEvent {
  pub at_beats:       f32,
  pub duration_beats: f32,
  pub notes:          Vec<u8>,
  pub velocity:       Option<u8>,
  pub hand:           Option<Hand>,
  pub lyric:          Option<String>,
  pub accent:         bool
}

impl Default for SongEvent {
  fn default() -> Self {
    Self {
      at_beats:       0.0,
      duration_beats: 1.0,
      notes:          Vec::new(),
      velocity:       None,
      hand:           None,
      lyric:          None,
      accent:         false
    }
  }
}

#[derive(Debug, Clone)]
pub struct LoadedSong {
  pub path: PathBuf,
  pub song: SongFile
}

impl LoadedSong {
  pub fn duration_beats(&self) -> f32 {
    self
      .song
      .events
      .iter()
      .map(|event| {
        event.at_beats
          + event.duration_beats
      })
      .fold(0.0, f32::max)
  }
}

#[derive(Debug, Clone)]
struct SongSource {
  kind: SourceKind,
  path: PathBuf
}

#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Serialize,
  Deserialize,
)]
#[serde(rename_all = "snake_case")]
enum SourceKind {
  Toml,
  Midi
}

impl SourceKind {
  fn cache_subdir(
    self
  ) -> &'static str {
    match self {
      | Self::Toml => "toml",
      | Self::Midi => "midi"
    }
  }
}

#[derive(
  Debug,
  Clone,
  PartialEq,
  Eq,
  Serialize,
  Deserialize,
)]
struct SourceFingerprint {
  modified_secs:  u64,
  modified_nanos: u32,
  size_bytes:     u64
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
struct CachedSongFile {
  cache_version: u16,
  source_path:   String,
  source_kind:   SourceKind,
  fingerprint:   SourceFingerprint,
  song:          SongFile
}

#[derive(Debug, Clone, Copy)]
struct MidiNoteRange {
  start_tick: u64,
  end_tick:   u64,
  note:       u8,
  velocity:   u8
}

pub fn load_song_library(
  config: &SongLibraryConfig
) -> Result<Vec<LoadedSong>> {
  let songs_root =
    Path::new(&config.directory);
  let midi_root =
    Path::new(&config.midi_directory);
  let cache_root =
    Path::new(&config.cache_directory);

  ensure_cache_dirs(cache_root)?;

  let mut sources = Vec::new();
  sources.extend(
    discover_toml_sources(
      songs_root,
      Path::new(&config.schema_path)
    )?
  );
  sources.extend(
    discover_midi_sources(midi_root)?
  );

  sources.sort_by(|left, right| {
    left.path.cmp(&right.path)
  });

  let mut loaded = Vec::new();
  let mut midi_loaded = 0usize;
  let mut toml_loaded = 0usize;

  for source in sources {
    match load_source_with_cache(
      &source, config, cache_root
    ) {
      | Ok(song) => {
        match source.kind {
          | SourceKind::Toml => {
            toml_loaded += 1
          }
          | SourceKind::Midi => {
            midi_loaded += 1
          }
        }
        loaded.push(song);
      }
      | Err(error) => {
        warn!(path = %source.path.display(), source_kind = ?source.kind, error = %error, "skipping invalid song source")
      }
    }
  }

  loaded.sort_by(|left, right| {
    left
      .song
      .meta
      .sort_order
      .cmp(&right.song.meta.sort_order)
      .then(
        left
          .song
          .meta
          .title
          .cmp(&right.song.meta.title)
      )
  });

  info!(songs_loaded = loaded.len(), toml_loaded, midi_loaded, cache_root = %cache_root.display(), "song library loaded");

  Ok(loaded)
}

fn load_source_with_cache(
  source: &SongSource,
  config: &SongLibraryConfig,
  cache_root: &Path
) -> Result<LoadedSong> {
  let fingerprint =
    source_fingerprint(&source.path)?;
  let cache_path =
    cache_path_for_source(
      cache_root, source
    );

  if let Some(song) =
    load_cached_song_if_fresh(
      &cache_path,
      source,
      &fingerprint
    )?
  {
    return Ok(LoadedSong {
      path: source.path.clone(),
      song
    });
  }

  let mut song = match source.kind {
    | SourceKind::Toml => {
      parse_toml_song(&source.path)?
    }
    | SourceKind::Midi => {
      parse_midi_song(
        &source.path,
        &config.schema_path
      )?
    }
  };

  finalize_song(
    &mut song,
    &source.path
  )?;

  write_cached_song(
    &cache_path,
    source,
    &fingerprint,
    &song
  )?;

  Ok(LoadedSong {
    path: source.path.clone(),
    song
  })
}

fn ensure_cache_dirs(
  cache_root: &Path
) -> Result<()> {
  for kind in
    [SourceKind::Toml, SourceKind::Midi]
  {
    let dir = cache_root
      .join(format!(
        "v{}",
        SONG_CACHE_VERSION
      ))
      .join(kind.cache_subdir());

    fs::create_dir_all(&dir)
      .with_context(|| {
        format!(
          "failed creating cache \
           directory {}",
          dir.display()
        )
      })?;
  }

  Ok(())
}

fn discover_toml_sources(
  songs_root: &Path,
  schema_path: &Path
) -> Result<Vec<SongSource>> {
  if !songs_root.exists() {
    warn!(songs_root = %songs_root.display(), "songs directory does not exist");
    return Ok(Vec::new());
  }

  let schema_path =
    schema_path.to_path_buf();
  let paths = collect_files_recursive(
    songs_root
  )?;

  let mut sources = Vec::new();
  for path in paths {
    if path
      .extension()
      .and_then(|ext| ext.to_str())
      != Some("toml")
    {
      continue;
    }

    if path == schema_path {
      continue;
    }

    if path
      .file_name()
      .and_then(|name| name.to_str())
      .is_some_and(|name| {
        name.ends_with(".schema.toml")
      })
    {
      continue;
    }

    sources.push(SongSource {
      kind: SourceKind::Toml,
      path
    });
  }

  Ok(sources)
}

fn discover_midi_sources(
  midi_root: &Path
) -> Result<Vec<SongSource>> {
  if !midi_root.exists() {
    info!(midi_root = %midi_root.display(), "midi directory does not exist yet; skipping midi import");
    return Ok(Vec::new());
  }

  let paths =
    collect_files_recursive(midi_root)?;

  let mut sources = Vec::new();
  for path in paths {
    let ext = path
      .extension()
      .and_then(|ext| ext.to_str())
      .map(|ext| {
        ext.to_ascii_lowercase()
      });

    if !matches!(
      ext.as_deref(),
      Some("mid") | Some("midi")
    ) {
      continue;
    }

    sources.push(SongSource {
      kind: SourceKind::Midi,
      path
    });
  }

  Ok(sources)
}

fn collect_files_recursive(
  root: &Path
) -> Result<Vec<PathBuf>> {
  let mut files = Vec::new();
  let mut stack =
    vec![root.to_path_buf()];

  while let Some(directory) =
    stack.pop()
  {
    for entry in
      fs::read_dir(&directory)
        .with_context(|| {
          format!(
            "failed reading {}",
            directory.display()
          )
        })?
    {
      let entry =
        entry.with_context(|| {
          format!(
            "failed iterating {}",
            directory.display()
          )
        })?;

      let path = entry.path();
      if path.is_dir() {
        stack.push(path);
      } else {
        files.push(path);
      }
    }
  }

  Ok(files)
}

fn source_fingerprint(
  path: &Path
) -> Result<SourceFingerprint> {
  let metadata = fs::metadata(path)
    .with_context(|| {
      format!(
        "failed reading metadata for \
         {}",
        path.display()
      )
    })?;

  let modified = metadata
    .modified()
    .unwrap_or(UNIX_EPOCH);
  let duration = modified
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default();

  Ok(SourceFingerprint {
    modified_secs:  duration.as_secs(),
    modified_nanos: duration
      .subsec_nanos(),
    size_bytes:     metadata.len()
  })
}

fn cache_path_for_source(
  cache_root: &Path,
  source: &SongSource
) -> PathBuf {
  let stem = source
    .path
    .file_stem()
    .and_then(|stem| stem.to_str())
    .unwrap_or("song");

  let cache_stem =
    sanitize_for_cache(stem);

  let mut hasher = DefaultHasher::new();
  source
    .path
    .to_string_lossy()
    .hash(&mut hasher);
  let hash = hasher.finish();

  cache_root
    .join(format!(
      "v{}",
      SONG_CACHE_VERSION
    ))
    .join(source.kind.cache_subdir())
    .join(format!(
      "{cache_stem}_{hash:016x}.toml"
    ))
}

fn sanitize_for_cache(
  input: &str
) -> String {
  let mut out = String::new();
  for ch in input.chars() {
    if ch.is_ascii_alphanumeric()
      || ch == '_'
      || ch == '-'
    {
      out.push(ch);
    } else {
      out.push('_');
    }
  }

  if out.is_empty() {
    "song".to_string()
  } else {
    out
  }
}

fn load_cached_song_if_fresh(
  cache_path: &Path,
  source: &SongSource,
  fingerprint: &SourceFingerprint
) -> Result<Option<SongFile>> {
  if !cache_path.exists() {
    return Ok(None);
  }

  let raw =
    fs::read_to_string(cache_path)
      .with_context(|| {
        format!(
          "failed reading cache {}",
          cache_path.display()
        )
      })?;

  let cached: CachedSongFile =
    toml::from_str(&raw).with_context(
      || {
        format!(
          "failed parsing cache {}",
          cache_path.display()
        )
      }
    )?;

  if cached.cache_version
    != SONG_CACHE_VERSION
  {
    debug!(cache_path = %cache_path.display(), "stale cache version");
    return Ok(None);
  }

  if cached.source_kind != source.kind {
    return Ok(None);
  }

  if cached.source_path
    != source.path.to_string_lossy()
  {
    return Ok(None);
  }

  if &cached.fingerprint != fingerprint
  {
    return Ok(None);
  }

  let mut song = cached.song;
  finalize_song(
    &mut song,
    &source.path
  )?;

  debug!(path = %source.path.display(), cache_path = %cache_path.display(), source_kind = ?source.kind, "loaded song from cache");

  Ok(Some(song))
}

fn write_cached_song(
  cache_path: &Path,
  source: &SongSource,
  fingerprint: &SourceFingerprint,
  song: &SongFile
) -> Result<()> {
  if let Some(parent) =
    cache_path.parent()
  {
    fs::create_dir_all(parent)
      .with_context(|| {
        format!(
          "failed creating cache \
           directory {}",
          parent.display()
        )
      })?;
  }

  let payload = CachedSongFile {
    cache_version: SONG_CACHE_VERSION,
    source_path:   source
      .path
      .to_string_lossy()
      .to_string(),
    source_kind:   source.kind,
    fingerprint:   fingerprint.clone(),
    song:          song.clone()
  };

  let rendered =
    toml::to_string_pretty(&payload)
      .context(
        "failed serializing song \
         cache payload"
      )?;

  fs::write(cache_path, rendered)
    .with_context(|| {
      format!(
        "failed writing cache {}",
        cache_path.display()
      )
    })?;

  Ok(())
}

fn parse_toml_song(
  path: &Path
) -> Result<SongFile> {
  let raw = fs::read_to_string(path)
    .with_context(|| {
      format!(
        "failed reading {}",
        path.display()
      )
    })?;

  let song: SongFile = toml::from_str(
    &raw
  )
  .with_context(|| {
    format!(
      "failed parsing {}",
      path.display()
    )
  })?;

  Ok(song)
}

fn parse_midi_song(
  path: &Path,
  schema_path: &str
) -> Result<SongFile> {
  let bytes = fs::read(path)
    .with_context(|| {
      format!(
        "failed reading MIDI {}",
        path.display()
      )
    })?;

  let smf = Smf::parse(&bytes)
    .with_context(|| {
      format!(
        "failed parsing MIDI {}",
        path.display()
      )
    })?;

  let ticks_per_beat =
    ticks_per_beat_from_timing(
      smf.header.timing,
      path
    );

  let mut tempo_changes: Vec<(
    u64,
    u32
  )> = Vec::new();
  let mut time_signature: Option<(
    u8,
    u8
  )> = None;

  let mut active_notes: HashMap<
    (u8, u8),
    Vec<(u64, u8)>
  > = HashMap::new();
  let mut note_ranges = Vec::new();

  for track in &smf.tracks {
    let mut absolute_tick = 0_u64;

    for event in track {
      absolute_tick = absolute_tick
        .saturating_add(u64::from(
          event.delta.as_int()
        ));

      match event.kind {
        | TrackEventKind::Midi {
          channel,
          message
        } => {
          let channel =
            channel.as_int();
          handle_midi_message(
            message,
            channel,
            absolute_tick,
            &mut active_notes,
            &mut note_ranges
          );
        }
        | TrackEventKind::Meta(
          meta_message
        ) => {
          handle_meta_message(
            meta_message,
            absolute_tick,
            &mut tempo_changes,
            &mut time_signature
          );
        }
        | _ => {}
      }
    }
  }

  close_unended_notes(
    &active_notes,
    ticks_per_beat,
    &mut note_ranges
  );

  if note_ranges.is_empty() {
    bail!(
      "{} contains no playable MIDI \
       note events",
      path.display()
    );
  }

  tempo_changes.sort_by(
    |left, right| left.0.cmp(&right.0)
  );

  if tempo_changes.len() > 1 {
    warn!(path = %path.display(), tempo_changes = tempo_changes.len(), "MIDI file has tempo changes; using first tempo for current song format");
  }

  let tempo_micros = tempo_changes
    .first()
    .map(|(_, micros)| *micros)
    .unwrap_or(500_000);
  let tempo_bpm = (60_000_000.0
    / tempo_micros as f32)
    .clamp(10.0, 400.0);

  let (beats_per_bar, beat_unit) =
    time_signature.unwrap_or((4, 4));

  let mut grouped = BTreeMap::<
    (u64, u64, u8),
    Vec<u8>
  >::new();
  let mut velocity_sum = 0_u32;

  for range in note_ranges {
    if range.end_tick
      <= range.start_tick
    {
      continue;
    }

    velocity_sum +=
      u32::from(range.velocity);

    grouped
      .entry((
        range.start_tick,
        range.end_tick,
        range.velocity
      ))
      .or_default()
      .push(range.note);
  }

  if grouped.is_empty() {
    bail!(
      "{} produced no grouped note \
       events after MIDI parse",
      path.display()
    );
  }

  let total_note_count = grouped
    .values()
    .map(Vec::len)
    .sum::<usize>()
    .max(1);
  let default_velocity =
    ((velocity_sum as usize)
      / total_note_count)
      .clamp(1, 127) as u8;

  let mut events = Vec::new();
  for (
    (start_tick, end_tick, velocity),
    mut notes
  ) in grouped
  {
    notes.sort_unstable();
    notes.dedup();

    let at_beats = ticks_to_beats(
      start_tick,
      ticks_per_beat
    );
    let duration_beats =
      ticks_to_beats(
        end_tick - start_tick,
        ticks_per_beat
      )
      .max(0.05);

    events.push(SongEvent {
      at_beats,
      duration_beats,
      notes,
      velocity: Some(velocity),
      hand: None,
      lyric: None,
      accent: false
    });
  }

  let file_stem = path
    .file_stem()
    .and_then(|stem| stem.to_str())
    .unwrap_or("untitled");
  let id = sanitize_song_id(file_stem);
  let title =
    humanize_song_title(file_stem);

  let mut song = SongFile {
    version: 1,
    schema: schema_path.to_string(),
    meta: SongMetadata {
      id,
      title,
      artist: "MIDI Import".to_string(),
      composer: String::new(),
      arranger: "MIDI Loader"
        .to_string(),
      description: format!(
        "Imported from MIDI file {}",
        path.display()
      ),
      difficulty: 2,
      tempo_bpm,
      beats_per_bar,
      beat_unit,
      key_signature: "Unknown"
        .to_string(),
      tags: vec![
        "midi".to_string(),
        "imported".to_string(),
      ],
      source_url: path
        .to_string_lossy()
        .to_string(),
      sort_order: 200,
      default_velocity
    },
    sections: Vec::new(),
    events
  };

  finalize_song(&mut song, path)?;

  info!(path = %path.display(), song_id = %song.meta.id, events = song.events.len(), tempo_bpm = song.meta.tempo_bpm, "midi imported as song");

  Ok(song)
}

fn handle_midi_message(
  message: MidiMessage,
  channel: u8,
  absolute_tick: u64,
  active_notes: &mut HashMap<
    (u8, u8),
    Vec<(u64, u8)>
  >,
  note_ranges: &mut Vec<MidiNoteRange>
) {
  match message {
    | MidiMessage::NoteOn {
      key,
      vel
    } => {
      let note = key.as_int();
      let velocity = vel.as_int();

      if velocity == 0 {
        finish_active_note(
          channel,
          note,
          absolute_tick,
          active_notes,
          note_ranges
        );
      } else {
        active_notes
          .entry((channel, note))
          .or_default()
          .push((
            absolute_tick,
            velocity
          ));
      }
    }
    | MidiMessage::NoteOff {
      key,
      ..
    } => {
      finish_active_note(
        channel,
        key.as_int(),
        absolute_tick,
        active_notes,
        note_ranges
      );
    }
    | _ => {}
  }
}

fn finish_active_note(
  channel: u8,
  note: u8,
  absolute_tick: u64,
  active_notes: &mut HashMap<
    (u8, u8),
    Vec<(u64, u8)>
  >,
  note_ranges: &mut Vec<MidiNoteRange>
) {
  if let Some(starts) = active_notes
    .get_mut(&(channel, note))
  {
    if let Some((
      start_tick,
      velocity
    )) = starts.pop()
    {
      let end_tick = absolute_tick
        .max(start_tick + 1);

      note_ranges.push(MidiNoteRange {
        start_tick,
        end_tick,
        note,
        velocity
      });
    }
  }
}

fn handle_meta_message(
  message: MetaMessage,
  absolute_tick: u64,
  tempo_changes: &mut Vec<(u64, u32)>,
  time_signature: &mut Option<(u8, u8)>
) {
  match message {
    | MetaMessage::Tempo(
      micros_per_quarter
    ) => {
      tempo_changes.push((
        absolute_tick,
        micros_per_quarter.as_int()
      ));
    }
    | MetaMessage::TimeSignature(
      numerator,
      denominator_pow,
      _,
      _
    ) => {
      if time_signature.is_none()
        && numerator > 0
      {
        let beat_unit = 2_u8
          .checked_pow(u32::from(
            denominator_pow
          ))
          .unwrap_or(4);

        if matches!(
          beat_unit,
          1 | 2 | 4 | 8 | 16 | 32
        ) {
          *time_signature = Some((
            numerator, beat_unit
          ));
        }
      }
    }
    | _ => {}
  }
}

fn close_unended_notes(
  active_notes: &HashMap<
    (u8, u8),
    Vec<(u64, u8)>
  >,
  ticks_per_beat: u32,
  note_ranges: &mut Vec<MidiNoteRange>
) {
  let fallback_duration =
    u64::from(ticks_per_beat.max(1))
      / 2;

  for ((_, note), starts) in
    active_notes
  {
    for (start_tick, velocity) in starts
    {
      note_ranges.push(MidiNoteRange {
        start_tick: *start_tick,
        end_tick:   start_tick
          .saturating_add(
            fallback_duration.max(1)
          ),
        note:       *note,
        velocity:   *velocity
      });
    }
  }
}

fn ticks_per_beat_from_timing(
  timing: Timing,
  path: &Path
) -> u32 {
  match timing {
    | Timing::Metrical(
      ticks_per_beat
    ) => {
      u32::from(ticks_per_beat.as_int())
    }
    | Timing::Timecode(_, _) => {
      warn!(path = %path.display(), "MIDI uses SMPTE timing; using fallback ticks_per_beat=480");
      480
    }
  }
}

fn ticks_to_beats(
  ticks: u64,
  ticks_per_beat: u32
) -> f32 {
  ticks as f32
    / ticks_per_beat.max(1) as f32
}

fn sanitize_song_id(
  input: &str
) -> String {
  let mut out = String::new();
  for ch in input.chars() {
    if ch.is_ascii_alphanumeric()
      || ch == '_'
      || ch == '-'
    {
      out.push(ch.to_ascii_lowercase());
    } else {
      out.push('_');
    }
  }

  let trimmed =
    out.trim_matches('_').to_string();

  if trimmed.is_empty() {
    "midi_song".to_string()
  } else {
    trimmed
  }
}

fn humanize_song_title(
  input: &str
) -> String {
  let replaced = input
    .replace('_', " ")
    .replace('-', " ");

  let trimmed = replaced.trim();
  if trimmed.is_empty() {
    "Imported MIDI".to_string()
  } else {
    trimmed.to_string()
  }
}

fn finalize_song(
  song: &mut SongFile,
  source_path: &Path
) -> Result<()> {
  song.events.sort_by(|left, right| {
    left
      .at_beats
      .total_cmp(&right.at_beats)
  });

  validate_song(song, source_path)?;

  debug!(
    path = %source_path.display(),
    song_id = %song.meta.id,
    events = song.events.len(),
    "song finalized",
  );

  Ok(())
}

fn validate_song(
  song: &SongFile,
  path: &Path
) -> Result<()> {
  if song.version == 0 {
    bail!(
      "{} has invalid version 0",
      path.display()
    );
  }

  if song.meta.id.trim().is_empty() {
    bail!(
      "{} missing meta.id",
      path.display()
    );
  }

  if song.meta.title.trim().is_empty() {
    bail!(
      "{} missing meta.title",
      path.display()
    );
  }

  if song.meta.tempo_bpm <= 0.0 {
    bail!(
      "{} has non-positive tempo_bpm",
      path.display()
    );
  }

  if song.meta.beats_per_bar == 0 {
    bail!(
      "{} has beats_per_bar = 0",
      path.display()
    );
  }

  if !matches!(
    song.meta.beat_unit,
    1 | 2 | 4 | 8 | 16 | 32
  ) {
    bail!(
      "{} has unsupported beat_unit {}",
      path.display(),
      song.meta.beat_unit
    );
  }

  if !(1..=127).contains(
    &song.meta.default_velocity
  ) {
    bail!(
      "{} has default_velocity \
       outside 1..=127",
      path.display()
    );
  }

  if song.events.is_empty() {
    bail!(
      "{} has no note events",
      path.display()
    );
  }

  for (index, event) in
    song.events.iter().enumerate()
  {
    if event.at_beats < 0.0 {
      bail!(
        "{} event[{index}] has \
         negative at_beats",
        path.display()
      );
    }

    if event.duration_beats <= 0.0 {
      bail!(
        "{} event[{index}] has \
         non-positive duration_beats",
        path.display()
      );
    }

    if event.notes.is_empty() {
      bail!(
        "{} event[{index}] has no \
         notes",
        path.display()
      );
    }

    if event
      .notes
      .iter()
      .any(|note| *note > 127)
    {
      bail!(
        "{} event[{index}] has MIDI \
         note outside 0..=127",
        path.display()
      );
    }

    if let Some(velocity) =
      event.velocity
    {
      if !(1..=127).contains(&velocity)
      {
        bail!(
          "{} event[{index}] has \
           velocity outside 1..=127",
          path.display()
        );
      }
    }
  }

  for (index, section) in
    song.sections.iter().enumerate()
  {
    if section.id.trim().is_empty() {
      bail!(
        "{} section[{index}] missing \
         id",
        path.display()
      );
    }

    if section.end_beats
      < section.start_beats
    {
      bail!(
        "{} section[{index}] ends \
         before it starts",
        path.display()
      );
    }
  }

  Ok(())
}
