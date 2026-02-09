use std::fs;
use std::path::{
  Path,
  PathBuf
};

use anyhow::{
  Context,
  Result,
  bail
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

pub fn load_song_library(
  config: &SongLibraryConfig
) -> Result<Vec<LoadedSong>> {
  let songs_root =
    Path::new(&config.directory);
  let mut loaded = Vec::new();

  if !songs_root.exists() {
    warn!(songs_root = %songs_root.display(), "songs directory does not exist");
    return Ok(loaded);
  }

  for entry in fs::read_dir(songs_root)
    .with_context(|| {
      format!(
        "failed reading songs \
         directory {}",
        songs_root.display()
      )
    })?
  {
    let entry =
      entry.with_context(|| {
        format!(
          "failed iterating {}",
          songs_root.display()
        )
      })?;
    let path = entry.path();

    if path
      .extension()
      .and_then(|ext| ext.to_str())
      != Some("toml")
    {
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

    match load_single_song(&path) {
      | Ok(song) => loaded.push(song),
      | Err(error) => {
        warn!(path = %path.display(), error = %error, "skipping invalid song file")
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

  info!(songs_loaded = loaded.len(), songs_root = %songs_root.display(), "song library loaded");

  Ok(loaded)
}

fn load_single_song(
  path: &Path
) -> Result<LoadedSong> {
  let raw = fs::read_to_string(path)
    .with_context(|| {
      format!(
        "failed reading {}",
        path.display()
      )
    })?;
  let mut song: SongFile =
    toml::from_str(&raw).with_context(
      || {
        format!(
          "failed parsing {}",
          path.display()
        )
      }
    )?;

  validate_song(&song, path)?;
  song.events.sort_by(|left, right| {
    left
      .at_beats
      .total_cmp(&right.at_beats)
  });

  debug!(
    path = %path.display(),
    song_id = %song.meta.id,
    events = song.events.len(),
    "song parsed",
  );

  Ok(LoadedSong {
    path: path.to_path_buf(),
    song
  })
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
