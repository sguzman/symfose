use std::collections::BTreeMap;
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
  "config/symposium.toml";

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct AppConfig {
  pub app:              AppSection,
  pub logging:          LoggingConfig,
  pub audio:            AudioConfig,
  pub input:            InputConfig,
  pub control_bindings: ControlBindings,
  pub keybindings: BTreeMap<String, u8>,
  pub songs:            Vec<SongConfig>
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
      control_bindings:
        ControlBindings::default(),
      keybindings:
        default_keybindings(),
      songs:            default_songs()
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct AppSection {
  pub poll_interval_ms:    u64,
  pub print_unmapped_keys: bool
}

impl Default for AppSection {
  fn default() -> Self {
    Self {
      poll_interval_ms:    16,
      print_unmapped_keys: false
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct LoggingConfig {
  pub level:     String,
  pub directory: String
}

impl Default for LoggingConfig {
  fn default() -> Self {
    Self {
      level:     "symposium=debug,info"
        .to_string(),
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
pub struct AudioConfig {
  pub master_volume:    f32,
  pub note_duration_ms: u64,
  pub sample_rate_hz:   u32,
  pub waveform:         Waveform
}

impl Default for AudioConfig {
  fn default() -> Self {
    Self {
      master_volume:    0.15,
      note_duration_ms: 550,
      sample_rate_hz:   48_000,
      waveform:         Waveform::Sine
    }
  }
}

#[derive(
  Debug,
  Clone,
  Copy,
  Serialize,
  Deserialize,
  Default,
)]
#[serde(rename_all = "snake_case")]
pub enum Waveform {
  #[default]
  Sine,
  Triangle,
  Square,
  Sawtooth
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct ControlBindings {
  pub quit:           Vec<String>,
  pub list_songs:     Vec<String>,
  pub print_bindings: Vec<String>
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
      ]
    }
  }
}

#[derive(
  Debug, Clone, Serialize, Deserialize,
)]
#[serde(default)]
pub struct SongConfig {
  pub id:        String,
  pub title:     String,
  pub notation:  String,
  pub tempo_bpm: u16
}

impl Default for SongConfig {
  fn default() -> Self {
    Self {
      id:        "untitled".to_string(),
      title:     "Untitled Song"
        .to_string(),
      notation:  String::new(),
      tempo_bpm: 120
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
  if !(0.0..=1.0).contains(
    &config.audio.master_volume
  ) {
    bail!(
      "audio.master_volume must be \
       between 0.0 and 1.0"
    );
  }
  if config.audio.note_duration_ms == 0
  {
    bail!(
      "audio.note_duration_ms must be \
       > 0"
    );
  }
  if config.audio.sample_rate_hz < 8_000
  {
    bail!(
      "audio.sample_rate_hz must be \
       at least 8000"
    );
  }
  if config.keybindings.is_empty() {
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

  Ok(())
}

fn default_keybindings()
-> BTreeMap<String, u8> {
  let mut map = BTreeMap::new();

  map.insert("a".to_string(), 60);
  map.insert("w".to_string(), 61);
  map.insert("s".to_string(), 62);
  map.insert("e".to_string(), 63);
  map.insert("d".to_string(), 64);
  map.insert("f".to_string(), 65);
  map.insert("t".to_string(), 66);
  map.insert("g".to_string(), 67);
  map.insert("y".to_string(), 68);
  map.insert("h".to_string(), 69);
  map.insert("u".to_string(), 70);
  map.insert("j".to_string(), 71);
  map.insert("k".to_string(), 72);
  map.insert("o".to_string(), 73);
  map.insert("l".to_string(), 74);
  map.insert("p".to_string(), 75);
  map.insert(";".to_string(), 76);

  map
}

fn default_songs() -> Vec<SongConfig> {
  vec![
    SongConfig {
      id:        "twinkle".to_string(),
      title:     "Twinkle Twinkle \
                  (Starter)"
        .to_string(),
      notation:  "C C G G A A G | F F \
                  E E D D C"
        .to_string(),
      tempo_bpm: 100
    },
    SongConfig {
      id:        "ode_to_joy"
        .to_string(),
      title:     "Ode To Joy (Starter)"
        .to_string(),
      notation:  "E E F G | G F E D | \
                  C C D E | E D D"
        .to_string(),
      tempo_bpm: 110
    },
  ]
}
