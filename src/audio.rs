use std::f32::consts::TAU;
use std::time::Duration;

use anyhow::{
  Context,
  Result
};
use rodio::buffer::SamplesBuffer;
use rodio::source::{
  Function,
  SignalGenerator,
  Source
};
use rodio::{
  OutputStream,
  OutputStreamBuilder
};
use tracing::{
  debug,
  info
};

use crate::config::{
  AudioConfig,
  Instrument
};
use crate::songs::SongFile;

pub struct AudioEngine {
  stream:              OutputStream,
  default_instrument:  Instrument,
  default_volume:      f32,
  default_duration_ms: u64
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

    info!(
      sample_rate = stream.config().sample_rate(),
      channels = stream.config().channel_count(),
      instrument = ?config.instrument,
      volume = config.master_volume,
      "audio engine initialized",
    );

    Ok(Self {
      stream,
      default_instrument: config
        .instrument,
      default_volume: config
        .master_volume,
      default_duration_ms: config
        .note_duration_ms
    })
  }

  pub fn play_note(
    &self,
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
    &self,
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
      instrument = ?self.default_instrument,
      "playing note",
    );

    let samples = render_note_samples(
      self.default_instrument,
      midi_note,
      velocity,
      duration_ms,
      sample_rate,
      self.default_volume
    );

    self.stream.mixer().add(
      SamplesBuffer::new(
        2,
        sample_rate,
        samples
      )
    );
  }

  pub fn play_song(
    &self,
    song: &SongFile
  ) {
    let beat_seconds = 60.0
      / song.meta.tempo_bpm.max(1.0);
    let sample_rate = self
      .stream
      .config()
      .sample_rate();

    info!(
      song_id = %song.meta.id,
      title = %song.meta.title,
      events = song.events.len(),
      tempo_bpm = song.meta.tempo_bpm,
      "scheduling song playback",
    );

    for event in &song.events {
      let start_after =
        Duration::from_secs_f32(
          (event.at_beats
            * beat_seconds)
            .max(0.0)
        );
      let duration_ms =
        ((event.duration_beats
          * beat_seconds)
          * 1000.0)
          .round()
          .max(40.0) as u64;
      let velocity =
        event.velocity.unwrap_or(
          song.meta.default_velocity
        );

      for midi_note in &event.notes {
        let samples =
          render_note_samples(
            self.default_instrument,
            *midi_note,
            velocity,
            duration_ms,
            sample_rate,
            self.default_volume
          );

        self.stream.mixer().add(
          SamplesBuffer::new(
            2,
            sample_rate,
            samples
          )
          .delay(start_after)
        );
      }
    }
  }
}

pub fn midi_to_frequency_hz(
  midi_note: u8
) -> f32 {
  let n = f32::from(midi_note);
  440.0
    * 2.0_f32.powf((n - 69.0) / 12.0)
}

fn render_note_samples(
  instrument: Instrument,
  midi_note: u8,
  velocity: u8,
  duration_ms: u64,
  sample_rate: u32,
  master_volume: f32
) -> Vec<f32> {
  match instrument {
    | Instrument::PianoModel => {
      render_piano_model(
        midi_note,
        velocity,
        duration_ms,
        sample_rate,
        master_volume
      )
    }
    | Instrument::Sine => {
      render_oscillator(
        Function::Sine,
        midi_note,
        velocity,
        duration_ms,
        sample_rate,
        master_volume
      )
    }
    | Instrument::Triangle => {
      render_oscillator(
        Function::Triangle,
        midi_note,
        velocity,
        duration_ms,
        sample_rate,
        master_volume
      )
    }
    | Instrument::Square => {
      render_oscillator(
        Function::Square,
        midi_note,
        velocity,
        duration_ms,
        sample_rate,
        master_volume
      )
    }
    | Instrument::Sawtooth => {
      render_oscillator(
        Function::Sawtooth,
        midi_note,
        velocity,
        duration_ms,
        sample_rate,
        master_volume
      )
    }
  }
}

fn render_oscillator(
  function: Function,
  midi_note: u8,
  velocity: u8,
  duration_ms: u64,
  sample_rate: u32,
  master_volume: f32
) -> Vec<f32> {
  let frequency_hz =
    midi_to_frequency_hz(midi_note);
  let velocity_gain =
    (f32::from(velocity) / 127.0)
      .powf(1.3);

  let mono = SignalGenerator::new(
    sample_rate,
    frequency_hz,
    function
  )
  .take_duration(Duration::from_millis(
    duration_ms
  ))
  .amplify(
    (master_volume * velocity_gain)
      .clamp(0.0, 1.0)
  );

  let mut out = Vec::new();
  for sample in mono {
    out.push(sample);
    out.push(sample);
  }

  out
}

fn render_piano_model(
  midi_note: u8,
  velocity: u8,
  duration_ms: u64,
  sample_rate: u32,
  master_volume: f32
) -> Vec<f32> {
  let release_ms = 360_u64;
  let total_ms =
    duration_ms + release_ms;
  let total_frames =
    ((sample_rate as f32
      * total_ms as f32)
      / 1000.0)
      .round() as usize;

  let frequency_hz =
    midi_to_frequency_hz(midi_note);
  let velocity_gain =
    (f32::from(velocity) / 127.0)
      .powf(1.35);
  let dampening =
    2.0 + (frequency_hz / 240.0);

  let mut samples = Vec::with_capacity(
    total_frames * 2
  );

  for frame in 0..total_frames {
    let t =
      frame as f32 / sample_rate as f32;
    let sustain_end =
      duration_ms as f32 / 1000.0;

    let attack =
      (t / 0.004).clamp(0.0, 1.0);
    let body_decay =
      (-t * dampening).exp();

    let release = if t <= sustain_end {
      1.0
    } else {
      (-(t - sustain_end) * 11.0).exp()
    };

    let envelope =
      attack * body_decay * release;

    // Three slightly detuned strings
    // per note.
    let mut tonal = 0.0;
    for detune in
      [0.9973_f32, 1.0, 1.0026]
    {
      let phase =
        TAU * frequency_hz * detune * t;
      tonal += phase.sin();
      tonal += (2.0 * phase).sin()
        * 0.48
        * (-t * 3.3).exp();
      tonal += (3.0 * phase).sin()
        * 0.27
        * (-t * 4.2).exp();
      tonal += (4.0 * phase).sin()
        * 0.18
        * (-t * 5.1).exp();
      tonal += (5.0 * phase).sin()
        * 0.10
        * (-t * 6.0).exp();
      tonal += (6.0 * phase).sin()
        * 0.06
        * (-t * 6.8).exp();
    }

    tonal *= 0.33;

    // Quick hammer noise burst.
    let hammer =
      pseudo_noise(frame, midi_note)
        * 0.28
        * (-t * 55.0).exp();

    // Simple soundboard resonance.
    let resonance =
      (TAU * (frequency_hz * 0.5) * t)
        .sin()
        * 0.08
        * (-t * 1.9).exp();

    let raw =
      (tonal + hammer + resonance)
        * envelope;
    let sample = (raw * 1.65).tanh()
      * velocity_gain
      * master_volume
      * 0.92;

    let pan = ((f32::from(midi_note)
      - 60.0)
      / 28.0)
      .clamp(-0.7, 0.7);
    let left =
      sample * (1.0 - pan) * 0.5;
    let right =
      sample * (1.0 + pan) * 0.5;

    samples.push(left);
    samples.push(right);
  }

  samples
}

fn pseudo_noise(
  frame_index: usize,
  midi_note: u8
) -> f32 {
  let mut x = (frame_index as u64)
    .wrapping_mul(0x9e37_79b9_7f4a_7c15)
    .wrapping_add(
      (u64::from(midi_note) + 1)
        * 0xd1b5_4a32_d192_ed03
    );

  x ^= x >> 12;
  x ^= x << 25;
  x ^= x >> 27;

  let y = x.wrapping_mul(
    0x2545_f491_4f6c_dd1d
  );
  let normalized = (y >> 41) as f32
    / ((1_u32 << 23) as f32);

  (normalized * 2.0) - 1.0
}
