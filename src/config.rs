use std::collections::BTreeMap;
use std::fmt::{
  Display,
  Formatter,
  Result as FmtResult
};
use std::fs;
use std::path::Path;

use anyhow::{
  Context,
  Result,
  bail
};
use serde::{
  Deserialize,
  Serialize
};

pub const DEFAULT_CONFIG_PATH: &str =
  "config/symfose.toml";

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct AppConfig {
  pub app:              AppSection,
  pub logging:          LoggingConfig,
  pub audio:            AudioConfig,
  pub input:            InputConfig,
  pub keyboard:         KeyboardConfig,
  pub gameplay:         GameplayConfig,
  pub control_bindings: ControlBindings,
  pub keybindings: BTreeMap<String, u8>,
  pub song_library: SongLibraryConfig
}

impl Default for AppConfig {
  fn default() -> Self {
    Self {
      app:
        AppSection::default(),
      logging:
        LoggingConfig::default(),
      audio:
        AudioConfig::default(),
      input:
        InputConfig::default(),
      keyboard:
        KeyboardConfig::default(),
      gameplay:
        GameplayConfig::default(),
      control_bindings:
        ControlBindings::default(),
      keybindings:
        default_keybindings(
          KeyboardLayout::default()
        ),
      song_library:
        SongLibraryConfig::default()
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct AppSection {
  pub print_unmapped_keys: bool
}

impl Default for AppSection {
  fn default() -> Self {
    Self {
      print_unmapped_keys: false
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct LoggingConfig {
  #[serde(alias = "level")]
  pub filter:    String,
  pub directory: String
}

impl Default for LoggingConfig {
  fn default() -> Self {
    Self {
      filter:    "info".to_string(),
      directory: "logs".to_string()
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct InputConfig {
  pub allow_key_repeat:           bool,
  pub ignore_shift_for_char_keys: bool
}

impl Default for InputConfig {
  fn default() -> Self {
    Self {
      allow_key_repeat:           false,
      ignore_shift_for_char_keys: true
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct KeyboardConfig {
  pub layout: KeyboardLayout,
  pub use_layout_default_bindings: bool
}

impl Default for KeyboardConfig {
  fn default() -> Self {
    Self {
      layout: KeyboardLayout::default(),
      use_layout_default_bindings: true
    }
  }
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
pub enum KeyboardLayout {
  #[serde(
    rename = "ansi104",
    alias = "ansi_104"
  )]
  Ansi104
}

impl Default for KeyboardLayout {
  fn default() -> Self {
    Self::Ansi104
  }
}

impl Display for KeyboardLayout {
  fn fmt(
    &self,
    f: &mut Formatter<'_>
  ) -> FmtResult {
    match self {
      | Self::Ansi104 => {
        write!(f, "ANSI 104-key")
      }
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct GameplayConfig {
  pub transpose_song_to_fit_bindings:
    bool,
  pub warn_on_missing_song_notes: bool,
  pub optimize_bindings_for_song: bool
}

impl Default for GameplayConfig {
  fn default() -> Self {
    Self {
      transpose_song_to_fit_bindings:
        true,
      warn_on_missing_song_notes:
        true,
      optimize_bindings_for_song:
        false
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct AudioConfig {
  pub instrument:          String,
  pub master_volume:       f32,
  pub note_duration_ms:    u64,
  pub release_duration_ms: u64,
  pub sample_rate_hz:      u32,
  pub instrument_profiles:
    BTreeMap<String, InstrumentProfile>
}

impl Default for AudioConfig {
  fn default() -> Self {
    Self {
      instrument:          "piano"
        .to_string(),
      master_volume:       0.68,
      note_duration_ms:    680,
      release_duration_ms: 720,
      sample_rate_hz:      48_000,
      instrument_profiles:
        default_instrument_profiles()
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(
  tag = "engine",
  rename_all = "snake_case"
)]
pub enum InstrumentProfile {
  Soundfont(SoundFontProfile)
}

impl Default for InstrumentProfile {
  fn default() -> Self {
    Self::Soundfont(
      SoundFontProfile::default()
    )
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct SoundFontProfile {
  pub soundfont_path: String,
  pub bank: u8,
  pub preset: u8,
  pub channel: u8,
  pub maximum_polyphony: usize,
  pub enable_reverb_and_chorus: bool,
  pub instrument_gain_multiplier: f32
}

impl Default for SoundFontProfile {
  fn default() -> Self {
    Self {
      soundfont_path: "res/soundfonts/\
                       piano.sf2"
        .to_string(),
      bank: 0,
      preset: 0,
      channel: 0,
      maximum_polyphony: 128,
      enable_reverb_and_chorus: true,
      instrument_gain_multiplier: 1.0
    }
  }
}

impl AudioConfig {
  pub fn active_profile(
    &self
  ) -> Option<(&str, &InstrumentProfile)>
  {
    self
      .instrument_profiles
      .get_key_value(&self.instrument)
      .map(|(name, profile)| {
        (name.as_str(), profile)
      })
  }

  pub fn active_profile_summary(
    &self
  ) -> String {
    if let Some((
      profile_name,
      profile
    )) = self.active_profile()
    {
      match profile {
        | InstrumentProfile::Soundfont(
          sf2
        ) => {
          format!(
            "{profile_name} \
             (soundfont \
             bank={} preset={} \
             channel={})",
            sf2.bank, sf2.preset,
            sf2.channel
          )
        }
      }
    } else {
      format!(
        "{} (missing profile)",
        self.instrument
      )
    }
  }
}

impl AppConfig {
  pub fn effective_keybindings(
    &self
  ) -> BTreeMap<String, u8> {
    if !self
      .keyboard
      .use_layout_default_bindings
    {
      return self.keybindings.clone();
    }

    let mut merged =
      default_keybindings(
        self.keyboard.layout
      );
    for (key, note) in &self.keybindings
    {
      merged.insert(key.clone(), *note);
    }

    merged
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct ControlBindings {
  pub quit:           Vec<String>,
  pub list_songs:     Vec<String>,
  pub print_bindings: Vec<String>,
  pub play_song:      Vec<String>
}

impl Default for ControlBindings {
  fn default() -> Self {
    Self {
      quit:           vec![
        "esc".to_string(),
        "ctrl+c".to_string(),
      ],
      list_songs:     vec![
        "f1".to_string(),
      ],
      print_bindings: vec![
        "f2".to_string(),
      ],
      play_song:      vec![
        "f5".to_string(),
      ]
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct SongLibraryConfig {
  pub directory:       String,
  pub midi_directory:  String,
  pub schema_path:     String,
  pub cache_directory: String
}

impl Default for SongLibraryConfig {
  fn default() -> Self {
    Self {
      directory:       "res/songs"
        .to_string(),
      midi_directory:  "res/assets/\
                        midi"
        .to_string(),
      schema_path:
        "res/songs/schema/song.schema.\
         json"
          .to_string(),
      cache_directory: ".cache/songs"
        .to_string()
    }
  }
}

pub fn load_or_create(
  path: &Path
) -> Result<AppConfig> {
  if !path.exists() {
    let config = AppConfig::default();
    write_default(path, &config)?;
    return Ok(config);
  }

  let content =
    fs::read_to_string(path)
      .with_context(|| {
        format!(
          "failed reading {}",
          path.display()
        )
      })?;

  let config: AppConfig =
    toml::from_str(&content)
      .with_context(|| {
        format!(
          "failed parsing TOML from {}",
          path.display()
        )
      })?;

  validate_config(&config)?;
  Ok(config)
}

pub fn write_default(
  path: &Path,
  config: &AppConfig
) -> Result<()> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)
      .with_context(|| {
        format!(
          "failed creating {}",
          parent.display()
        )
      })?;
  }

  let rendered =
    toml::to_string_pretty(config)
      .context(
        "failed serializing default \
         config"
      )?;
  fs::write(path, rendered)
    .with_context(|| {
      format!(
        "failed writing {}",
        path.display()
      )
    })?;

  Ok(())
}

fn validate_config(
  config: &AppConfig
) -> Result<()> {
  if !(0.0..=2.5).contains(
    &config.audio.master_volume
  ) {
    bail!(
      "audio.master_volume must be \
       between 0.0 and 2.5"
    );
  }

  if config.audio.note_duration_ms == 0
  {
    bail!(
      "audio.note_duration_ms must be \
       > 0"
    );
  }

  if config.audio.release_duration_ms
    == 0
  {
    bail!(
      "audio.release_duration_ms must \
       be > 0"
    );
  }

  if config.audio.sample_rate_hz
    < 16_000
    || config.audio.sample_rate_hz
      > 192_000
  {
    bail!(
      "audio.sample_rate_hz must be \
       in range 16000..=192000"
    );
  }

  if config
    .audio
    .instrument
    .trim()
    .is_empty()
  {
    bail!(
      "audio.instrument cannot be \
       empty"
    );
  }

  if config
    .audio
    .instrument_profiles
    .is_empty()
  {
    bail!(
      "audio.instrument_profiles must \
       define at least one profile"
    );
  }

  if !config
    .audio
    .instrument_profiles
    .contains_key(
      &config.audio.instrument
    )
  {
    bail!(
      "audio.instrument='{}' does not \
       match any audio.\
       instrument_profiles key",
      config.audio.instrument
    );
  }

  for (profile_name, profile) in
    &config.audio.instrument_profiles
  {
    validate_instrument_profile(
      profile_name,
      profile
    )?;
  }

  if !config
    .keyboard
    .use_layout_default_bindings
    && config.keybindings.is_empty()
  {
    bail!(
      "keybindings must define at \
       least one mapping"
    );
  }

  if config.keybindings.iter().any(
    |(_, midi_note)| *midi_note > 127
  ) {
    bail!(
      "all keybindings must map to \
       MIDI notes in range 0..=127"
    );
  }

  if config
    .song_library
    .directory
    .trim()
    .is_empty()
  {
    bail!(
      "song_library.directory cannot \
       be empty"
    );
  }

  if config
    .song_library
    .midi_directory
    .trim()
    .is_empty()
  {
    bail!(
      "song_library.midi_directory \
       cannot be empty"
    );
  }

  if config
    .song_library
    .schema_path
    .trim()
    .is_empty()
  {
    bail!(
      "song_library.schema_path \
       cannot be empty"
    );
  }

  if config
    .song_library
    .cache_directory
    .trim()
    .is_empty()
  {
    bail!(
      "song_library.cache_directory \
       cannot be empty"
    );
  }

  Ok(())
}

fn validate_instrument_profile(
  profile_name: &str,
  profile: &InstrumentProfile
) -> Result<()> {
  match profile {
    | InstrumentProfile::Soundfont(
      sf2
    ) => {
      if sf2
        .soundfont_path
        .trim()
        .is_empty()
      {
        bail!(
          "audio.instrument_profiles.\
           {profile_name}.\
           soundfont_path cannot be \
           empty"
        );
      }

      if sf2.bank > 127 {
        bail!(
          "audio.instrument_profiles.\
           {profile_name}.bank must \
           be <= 127"
        );
      }

      if sf2.preset > 127 {
        bail!(
          "audio.instrument_profiles.\
           {profile_name}.preset must \
           be <= 127"
        );
      }

      if sf2.channel > 15 {
        bail!(
          "audio.instrument_profiles.\
           {profile_name}.channel \
           must be <= 15"
        );
      }

      if !(8..=256).contains(
        &sf2.maximum_polyphony
      ) {
        bail!(
          "audio.instrument_profiles.\
           {profile_name}.\
           maximum_polyphony must be \
           in range 8..=256"
        );
      }

      if !(0.0..=2.5).contains(
        &sf2.instrument_gain_multiplier
      ) {
        bail!(
          "audio.instrument_profiles.\
           {profile_name}.\
           instrument_gain_multiplier \
           must be between 0.0 and 2.5"
        );
      }
    }
  }

  Ok(())
}

fn default_instrument_profiles()
-> BTreeMap<String, InstrumentProfile> {
  let mut map = BTreeMap::new();
  map.insert(
    "piano".to_string(),
    InstrumentProfile::default()
  );
  map.insert(
    "acoustic_guitar".to_string(),
    InstrumentProfile::Soundfont(
      SoundFontProfile {
        soundfont_path:
          "res/soundfonts/piano.sf2"
            .to_string(),
        bank: 0,
        preset: 24,
        channel: 0,
        maximum_polyphony: 96,
        enable_reverb_and_chorus: true,
        instrument_gain_multiplier: 1.1
      }
    )
  );
  map
}

pub fn keyboard_layout_key_priority(
  layout: KeyboardLayout
) -> &'static [&'static str] {
  match layout {
    | KeyboardLayout::Ansi104 => {
      &[
        "f", "j", "d", "k", "s", "l",
        "a", ";", "g", "h", "r", "u",
        "e", "i", "w", "o", "q", "p",
        "t", "y", "v", "n", "c", "m",
        "x", ",", "z", ".", "b", "'",
        "5", "6", "4", "7", "3", "8",
        "2", "9", "1", "0", "`", "-",
        "=", "/", "[", "]", "\\"
      ]
    }
  }
}

fn default_keybindings(
  layout: KeyboardLayout
) -> BTreeMap<String, u8> {
  let mut map = BTreeMap::new();
  let start_note = 48u8;

  for (index, key) in
    keyboard_layout_key_priority(layout)
      .iter()
      .enumerate()
  {
    let note = start_note
      .saturating_add(index as u8);
    if note > 127 {
      break;
    }
    map
      .insert((*key).to_string(), note);
  }

  map
}
