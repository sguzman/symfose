use std::time::Duration;

use anyhow::{
  Context,
  Result
};
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
  Waveform
};

pub struct AudioEngine {
  stream:   OutputStream,
  waveform: Waveform
}

impl AudioEngine {
  pub fn new(
    config: &AudioConfig
  ) -> Result<Self> {
    let mut builder = OutputStreamBuilder::from_default_device()
            .context("no audio output device available")?;
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
        waveform = ?config.waveform,
        "audio engine initialized",
    );

    Ok(Self {
      stream,
      waveform: config.waveform
    })
  }

  pub fn play_note(
    &self,
    midi_note: u8,
    volume: f32,
    duration_ms: u64
  ) {
    let frequency_hz =
      midi_to_frequency_hz(midi_note);
    let function = waveform_to_function(
      self.waveform
    );
    let duration =
      Duration::from_millis(
        duration_ms
      );

    debug!(
      midi_note,
      frequency_hz,
      volume,
      duration_ms,
      "playing note",
    );

    self.stream.mixer().add(
      SignalGenerator::new(
        self
          .stream
          .config()
          .sample_rate(),
        frequency_hz,
        function
      )
      .take_duration(duration)
      .amplify(volume.clamp(0.0, 1.0))
    );
  }
}

pub fn midi_to_frequency_hz(
  midi_note: u8
) -> f32 {
  let n = f32::from(midi_note);
  440.0
    * 2.0_f32.powf((n - 69.0) / 12.0)
}

fn waveform_to_function(
  waveform: Waveform
) -> Function {
  match waveform {
    | Waveform::Sine => Function::Sine,
    | Waveform::Triangle => {
      Function::Triangle
    }
    | Waveform::Square => {
      Function::Square
    }
    | Waveform::Sawtooth => {
      Function::Sawtooth
    }
  }
}
