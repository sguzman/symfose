use std::collections::BTreeMap;
use std::fs::File;
use std::path::{
  Path,
  PathBuf
};
use std::sync::Arc;

use anyhow::{
  Context,
  Result,
  bail
};
use rodio::buffer::SamplesBuffer;
use rodio::{
  OutputStream,
  OutputStreamBuilder
};
use rustysynth::{
  SoundFont,
  Synthesizer,
  SynthesizerSettings
};
use tracing::{
  debug,
  info,
  warn
};

use crate::config::{
  AudioConfig,
  InstrumentProfile,
  SoundFontProfile
};
use crate::songs::SongFile;

const COMMON_SOUNDFONT_PATHS: [&str;
  5] = [
  "/usr/share/sounds/sf2/FluidR3_GM.\
   sf2",
  "/usr/share/sounds/sf2/TimGM6mb.sf2",
  "/usr/share/sounds/sf2/default-GM.\
   sf2",
  "/usr/share/soundfonts/FluidR3_GM.\
   sf2",
  "/usr/share/sf2/FluidR3_GM.sf2"
];

pub struct AudioEngine {
  stream:              OutputStream,
  profiles: BTreeMap<
    String,
    LoadedSoundFontProfile
  >,
  active_profile_name: String,
  default_volume:      f32,
  default_duration_ms: u64,
  release_duration_ms: u64
}

struct LoadedSoundFontProfile {
  soundfont: Arc<SoundFont>,
  profile:   SoundFontProfile
}

impl AudioEngine {
  pub fn new(
    config: &AudioConfig
  ) -> Result<Self> {
    let mut builder =
      OutputStreamBuilder::from_default_device().context("no audio output device available")?;

    builder = builder.with_sample_rate(
      config.sample_rate_hz
    );

    let mut stream = builder
      .open_stream_or_fallback()
      .context(
        "failed to open audio stream"
      )?;

    stream.log_on_drop(false);

    let mut profiles = BTreeMap::<
      String,
      LoadedSoundFontProfile
    >::new();
    for (profile_name, profile) in
      &config.instrument_profiles
    {
      let loaded =
        load_soundfont_profile(
          profile_name,
          profile
        )?;
      profiles.insert(
        profile_name.clone(),
        loaded
      );
    }

    if !profiles
      .contains_key(&config.instrument)
    {
      bail!(
        "active instrument profile \
         '{}' not found in \
         audio.instrument_profiles",
        config.instrument
      );
    }

    info!(
      sample_rate = stream.config().sample_rate(),
      channels = stream.config().channel_count(),
      profile_name =
        %config.instrument,
      profiles_loaded = profiles.len(),
      profile_summary = %config.active_profile_summary(),
      master_volume = config.master_volume,
      default_note_duration_ms = config.note_duration_ms,
      release_duration_ms = config.release_duration_ms,
      "audio engine initialized",
    );

    Ok(Self {
      stream,
      profiles,
      active_profile_name: config
        .instrument
        .clone(),
      default_volume: config
        .master_volume,
      default_duration_ms: config
        .note_duration_ms,
      release_duration_ms: config
        .release_duration_ms
    })
  }

  pub fn master_volume(&self) -> f32 {
    self.default_volume
  }

  pub fn active_profile_name(
    &self
  ) -> &str {
    &self.active_profile_name
  }

  pub fn available_profiles(
    &self
  ) -> Vec<String> {
    self
      .profiles
      .keys()
      .cloned()
      .collect::<Vec<_>>()
  }

  pub fn active_profile_summary(
    &self
  ) -> String {
    if let Some(profile) =
      self.current_profile()
    {
      format!(
        "{} (soundfont bank={} \
         preset={} channel={})",
        self.active_profile_name,
        profile.profile.bank,
        profile.profile.preset,
        profile.profile.channel
      )
    } else {
      format!(
        "{} (missing profile)",
        self.active_profile_name
      )
    }
  }

  pub fn set_active_profile(
    &mut self,
    profile_name: &str
  ) -> Result<()> {
    if !self
      .profiles
      .contains_key(profile_name)
    {
      bail!(
        "unknown audio profile \
         '{profile_name}'"
      );
    }

    self.active_profile_name =
      profile_name.to_string();
    info!(
      profile = %self.active_profile_name,
      "active instrument profile changed",
    );
    Ok(())
  }

  pub fn set_master_volume(
    &mut self,
    volume: f32
  ) {
    let clamped =
      volume.clamp(0.0, 2.5);
    self.default_volume = clamped;
    info!(
      master_volume = clamped,
      "master volume updated"
    );
  }

  pub fn play_metronome_tick(
    &mut self,
    accent: bool
  ) {
    let midi_note = if accent {
      94
    } else {
      86
    };
    let velocity = if accent {
      124
    } else {
      92
    };
    let duration_ms = if accent {
      115
    } else {
      90
    };

    self
      .play_note_with_velocity_duration(
        midi_note,
        velocity,
        duration_ms
      );
  }

  pub fn play_note(
    &mut self,
    midi_note: u8
  ) {
    self
      .play_note_with_velocity_duration(
        midi_note,
        112,
        self.default_duration_ms
      );
  }

  pub fn play_note_with_velocity_duration(
    &mut self,
    midi_note: u8,
    velocity: u8,
    duration_ms: u64
  ) {
    let sample_rate = self
      .stream
      .config()
      .sample_rate();
    let frequency_hz =
      midi_to_frequency_hz(midi_note);

    debug!(
      midi_note,
      velocity,
      duration_ms,
      frequency_hz,
      profile = %self.active_profile_name,
      "rendering soundfont note",
    );

    let Some(active_profile) =
      self.current_profile()
    else {
      warn!(
        profile = %self.active_profile_name,
        "active profile missing while rendering note"
      );
      return;
    };

    match render_soundfont_note_samples(
      active_profile,
      midi_note,
      velocity,
      duration_ms,
      self.release_duration_ms,
      sample_rate,
      self.default_volume
    ) {
      | Ok(samples) => {
        self.stream.mixer().add(
          SamplesBuffer::new(
            2,
            sample_rate,
            samples
          )
        );
      }
      | Err(error) => {
        warn!(%error, midi_note, velocity, duration_ms, "failed rendering note");
      }
    }
  }

  #[allow(dead_code)]
  pub fn play_song(
    &mut self,
    song: &SongFile
  ) {
    let sample_rate = self
      .stream
      .config()
      .sample_rate();

    info!(
      song_id = %song.meta.id,
      title = %song.meta.title,
      events = song.events.len(),
      tempo_bpm = song.meta.tempo_bpm,
      profile = %self.active_profile_name,
      "rendering song preview",
    );

    let Some(active_profile) =
      self.current_profile()
    else {
      warn!(
        profile = %self.active_profile_name,
        "active profile missing while rendering song"
      );
      return;
    };

    match render_soundfont_song_samples(
      active_profile,
      song,
      sample_rate,
      self.default_volume,
      self.default_duration_ms,
      self.release_duration_ms
    ) {
      | Ok(samples) => {
        if samples.is_empty() {
          warn!(song_id = %song.meta.id, "song produced no audio samples");
          return;
        }

        let frames = samples.len() / 2;
        let duration_seconds = frames
          as f32
          / sample_rate as f32;

        info!(
          song_id = %song.meta.id,
          rendered_frames = frames,
          duration_seconds,
          "song preview rendered",
        );

        self.stream.mixer().add(
          SamplesBuffer::new(
            2,
            sample_rate,
            samples
          )
        );
      }
      | Err(error) => {
        warn!(%error, song_id = %song.meta.id, "failed rendering song preview");
      }
    }
  }

  fn current_profile(
    &self
  ) -> Option<&LoadedSoundFontProfile>
  {
    self
      .profiles
      .get(&self.active_profile_name)
  }
}

pub fn midi_to_frequency_hz(
  midi_note: u8
) -> f32 {
  let n = f32::from(midi_note);
  440.0
    * 2.0_f32.powf((n - 69.0) / 12.0)
}

fn load_soundfont_profile(
  profile_name: &str,
  profile: &InstrumentProfile
) -> Result<LoadedSoundFontProfile> {
  match profile {
    | InstrumentProfile::Soundfont(
      sf2
    ) => {
      load_soundfont(profile_name, sf2)
    }
  }
}

fn load_soundfont(
  profile_name: &str,
  profile: &SoundFontProfile
) -> Result<LoadedSoundFontProfile> {
  let (soundfont_path, used_fallback) =
    resolve_soundfont_path(
      &profile.soundfont_path
    )
    .with_context(|| {
      let common_paths =
        COMMON_SOUNDFONT_PATHS
          .join(", ");
      format!(
        "missing SoundFont for \
         profile '{profile_name}'. \
         looked for '{}' and fallback \
         paths: {common_paths}. place \
         an SF2 at the configured \
         path or update \
         [audio.instrument_profiles.\
         {profile_name}]",
        profile.soundfont_path
      )
    })?;

  if used_fallback {
    warn!(
      profile_name,
      configured_path = %profile.soundfont_path,
      fallback_path = %soundfont_path.display(),
      "configured SoundFont path missing; using fallback",
    );
  }

  let mut file =
    File::open(&soundfont_path)
      .with_context(|| {
        format!(
          "failed opening SoundFont {}",
          soundfont_path.display()
        )
      })?;

  let soundfont =
    SoundFont::new(&mut file)
      .with_context(|| {
        format!(
          "failed parsing SoundFont {}",
          soundfont_path.display()
        )
      })?;

  let soundfont_info =
    soundfont.get_info();
  info!(
    profile_name,
    path = %soundfont_path.display(),
    bank = profile.bank,
    preset = profile.preset,
    channel = profile.channel,
    maximum_polyphony = profile.maximum_polyphony,
    effects = profile.enable_reverb_and_chorus,
    gain = profile.instrument_gain_multiplier,
    bank_name = %soundfont_info.get_bank_name(),
    author = %soundfont_info.get_author(),
    "soundfont profile loaded",
  );

  Ok(LoadedSoundFontProfile {
    soundfont: Arc::new(soundfont),
    profile:   profile.clone()
  })
}

fn resolve_soundfont_path(
  configured_path: &str
) -> Option<(PathBuf, bool)> {
  let configured =
    Path::new(configured_path);
  if configured.exists() {
    return Some((
      configured.to_path_buf(),
      false
    ));
  }

  for candidate in
    COMMON_SOUNDFONT_PATHS
  {
    let candidate =
      Path::new(candidate);
    if candidate.exists() {
      return Some((
        candidate.to_path_buf(),
        true
      ));
    }
  }

  None
}

fn render_soundfont_note_samples(
  profile: &LoadedSoundFontProfile,
  midi_note: u8,
  velocity: u8,
  note_duration_ms: u64,
  release_duration_ms: u64,
  sample_rate: u32,
  master_volume: f32
) -> Result<Vec<f32>> {
  let hold_frames = ms_to_frames(
    note_duration_ms.max(40),
    sample_rate
  );
  let release_frames = ms_to_frames(
    release_duration_ms.max(160),
    sample_rate
  );
  let total_frames = hold_frames
    .saturating_add(release_frames);

  let actions = vec![
    ScheduledAction {
      frame:  0,
      action: MidiAction::NoteOn {
        key:      i32::from(midi_note),
        velocity: i32::from(
          velocity.clamp(1, 127)
        )
      }
    },
    ScheduledAction {
      frame:  hold_frames,
      action: MidiAction::NoteOff {
        key: i32::from(midi_note)
      }
    },
  ];

  render_scheduled_actions(
    profile,
    sample_rate,
    total_frames,
    actions,
    master_volume
  )
}

#[allow(dead_code)]
fn render_soundfont_song_samples(
  profile: &LoadedSoundFontProfile,
  song: &SongFile,
  sample_rate: u32,
  master_volume: f32,
  default_note_duration_ms: u64,
  release_duration_ms: u64
) -> Result<Vec<f32>> {
  if song.events.is_empty() {
    return Ok(Vec::new());
  }

  let beat_seconds =
    60.0 / song.meta.tempo_bpm.max(1.0);
  let fallback_duration_frames =
    ms_to_frames(
      default_note_duration_ms.max(40),
      sample_rate
    );
  let release_frames = ms_to_frames(
    release_duration_ms.max(240),
    sample_rate
  );

  let mut actions = Vec::new();
  let mut max_frame = 0usize;

  for event in &song.events {
    if event.notes.is_empty() {
      continue;
    }

    let start_seconds =
      (event.at_beats.max(0.0))
        * beat_seconds;
    let start_frame = seconds_to_frames(
      start_seconds,
      sample_rate
    );

    let event_duration_frames =
      if event.duration_beats > 0.0 {
        let duration_seconds = (event
          .duration_beats
          * beat_seconds)
          .max(0.04);
        seconds_to_frames(
          duration_seconds,
          sample_rate
        )
      } else {
        fallback_duration_frames
      };

    let velocity = event
      .velocity
      .unwrap_or(
        song.meta.default_velocity
      )
      .clamp(1, 127);

    for midi_note in &event.notes {
      actions.push(ScheduledAction {
        frame:  start_frame,
        action: MidiAction::NoteOn {
          key:      i32::from(
            *midi_note
          ),
          velocity: i32::from(velocity)
        }
      });

      let note_off_frame = start_frame
        .saturating_add(
          event_duration_frames
        );
      actions.push(ScheduledAction {
        frame:  note_off_frame,
        action: MidiAction::NoteOff {
          key: i32::from(*midi_note)
        }
      });
      max_frame =
        max_frame.max(note_off_frame);
    }

    debug!(
      song_id = %song.meta.id,
      start_beats = event.at_beats,
      duration_beats = event.duration_beats,
      notes = ?event.notes,
      velocity,
      "queued song event",
    );
  }

  if actions.is_empty() {
    return Ok(Vec::new());
  }

  let total_frames = max_frame
    .saturating_add(release_frames);
  render_scheduled_actions(
    profile,
    sample_rate,
    total_frames,
    actions,
    master_volume
  )
}

fn render_scheduled_actions(
  profile: &LoadedSoundFontProfile,
  sample_rate: u32,
  total_frames: usize,
  mut actions: Vec<ScheduledAction>,
  master_volume: f32
) -> Result<Vec<f32>> {
  if total_frames == 0 {
    return Ok(Vec::new());
  }

  actions.retain(|entry| {
    entry.frame <= total_frames
  });

  actions.sort_by(|left, right| {
    left
      .frame
      .cmp(&right.frame)
      .then_with(|| {
        left.action.sort_order().cmp(
          &right.action.sort_order()
        )
      })
  });

  let mut synth = build_synthesizer(
    profile,
    sample_rate
  )?;
  let mut left =
    vec![0.0_f32; total_frames];
  let mut right =
    vec![0.0_f32; total_frames];
  let channel =
    i32::from(profile.profile.channel);

  let mut cursor = 0usize;
  let mut action_index = 0usize;

  while action_index < actions.len() {
    let frame =
      actions[action_index].frame;

    if frame > cursor {
      synth.render(
        &mut left[cursor..frame],
        &mut right[cursor..frame]
      );
      cursor = frame;
    }

    while action_index < actions.len()
      && actions[action_index].frame
        == frame
    {
      apply_midi_action(
        &mut synth,
        channel,
        actions[action_index].action
      );
      action_index += 1;
    }
  }

  if cursor < total_frames {
    synth.render(
      &mut left[cursor..],
      &mut right[cursor..]
    );
  }

  let gain = (master_volume
    * profile
      .profile
      .instrument_gain_multiplier)
    .clamp(0.0, 2.5);

  let mut interleaved =
    Vec::with_capacity(
      total_frames * 2
    );
  for frame in 0..total_frames {
    interleaved.push(
      (left[frame] * gain)
        .clamp(-1.0, 1.0)
    );
    interleaved.push(
      (right[frame] * gain)
        .clamp(-1.0, 1.0)
    );
  }

  Ok(interleaved)
}

fn build_synthesizer(
  profile: &LoadedSoundFontProfile,
  sample_rate: u32
) -> Result<Synthesizer> {
  if !(16_000..=192_000)
    .contains(&sample_rate)
  {
    bail!(
      "sample rate {} is outside \
       rustysynth range 16000..=192000",
      sample_rate
    );
  }

  let mut settings =
    SynthesizerSettings::new(
      sample_rate as i32
    );
  settings.maximum_polyphony =
    profile.profile.maximum_polyphony;
  settings.enable_reverb_and_chorus =
    profile
      .profile
      .enable_reverb_and_chorus;

  let mut synth = Synthesizer::new(
    &profile.soundfont,
    &settings
  )
  .context(
    "failed to create soundfont \
     synthesizer"
  )?;

  let channel =
    i32::from(profile.profile.channel);
  synth.process_midi_message(
    channel,
    0xb0,
    0x00,
    i32::from(profile.profile.bank)
  );
  synth.process_midi_message(
    channel, 0xb0, 0x20, 0
  );
  synth.process_midi_message(
    channel,
    0xc0,
    i32::from(profile.profile.preset),
    0
  );
  synth.process_midi_message(
    channel, 0xb0, 0x07, 127
  );
  synth.process_midi_message(
    channel, 0xb0, 0x0b, 127
  );

  Ok(synth)
}

fn apply_midi_action(
  synth: &mut Synthesizer,
  channel: i32,
  action: MidiAction
) {
  match action {
    | MidiAction::NoteOn {
      key,
      velocity
    } => {
      synth
        .note_on(channel, key, velocity)
    }
    | MidiAction::NoteOff {
      key
    } => synth.note_off(channel, key)
  }
}

fn ms_to_frames(
  milliseconds: u64,
  sample_rate: u32
) -> usize {
  let frames = (milliseconds as f64
    * sample_rate as f64)
    / 1000.0;
  frames.round().max(1.0) as usize
}

#[allow(dead_code)]
fn seconds_to_frames(
  seconds: f32,
  sample_rate: u32
) -> usize {
  let frames = (seconds.max(0.0)
    * sample_rate as f32)
    .round();
  frames.max(0.0) as usize
}

#[derive(Debug, Clone, Copy)]
enum MidiAction {
  NoteOn {
    key:      i32,
    velocity: i32
  },
  NoteOff {
    key: i32
  }
}

impl MidiAction {
  fn sort_order(self) -> u8 {
    match self {
      | Self::NoteOff {
        ..
      } => 0,
      | Self::NoteOn {
        ..
      } => 1
    }
  }
}

#[derive(Debug, Clone, Copy)]
struct ScheduledAction {
  frame:  usize,
  action: MidiAction
}
