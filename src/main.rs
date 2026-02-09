mod audio;
mod config;
mod input;

use std::collections::{
  HashMap,
  HashSet
};
use std::env;
use std::io::{
  self,
  Write
};
use std::path::{
  Path,
  PathBuf
};
use std::time::Duration;

use anyhow::{
  Context,
  Result
};
use crossterm::event::{
  self,
  Event,
  KeyEvent,
  KeyEventKind,
  KeyboardEnhancementFlags,
  PopKeyboardEnhancementFlags,
  PushKeyboardEnhancementFlags
};
use crossterm::{
  execute,
  terminal
};
use tracing::{
  debug,
  info,
  trace,
  warn
};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{
  EnvFilter,
  fmt
};

use crate::audio::AudioEngine;
use crate::config::{
  AppConfig,
  DEFAULT_CONFIG_PATH
};
use crate::input::{
  KeyChord,
  compile_chord_set,
  compile_note_bindings
};

#[derive(Debug)]
struct RuntimeBindings {
  note_bindings:  HashMap<KeyChord, u8>,
  quit:           HashSet<KeyChord>,
  list_songs:     HashSet<KeyChord>,
  print_bindings: HashSet<KeyChord>
}

#[derive(Debug)]
struct TerminalSession {
  pushed_keyboard_flags: bool
}

impl TerminalSession {
  fn enter() -> Result<Self> {
    terminal::enable_raw_mode()
      .context(
        "failed to enable raw mode"
      )?;

    let mut stdout = io::stdout();
    let pushed_keyboard_flags = execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES),
        )
        .map(|_| true)
        .unwrap_or_else(|error| {
            warn!(
                error = %error,
                "keyboard enhancement flags unavailable, key repeat/release may be limited",
            );
            false
        });

    Ok(Self {
      pushed_keyboard_flags
    })
  }
}

impl Drop for TerminalSession {
  fn drop(&mut self) {
    if self.pushed_keyboard_flags {
      let mut stdout = io::stdout();
      let _ = execute!(
        stdout,
        PopKeyboardEnhancementFlags
      );
    }
    let _ =
      terminal::disable_raw_mode();
  }
}

fn main() -> Result<()> {
  let config_path =
    configured_config_path();
  let config_already_present =
    config_path.exists();
  let config = config::load_or_create(
    &config_path
  )
  .with_context(|| {
    format!(
      "failed loading config at {}",
      config_path.display()
    )
  })?;

  let _log_guard =
    init_tracing(&config)?;
  info!(config_path = %config_path.display(), "booting Symposium");
  if !config_already_present {
    info!(
        config_path = %config_path.display(),
        "default config created because no config file was found",
    );
  }

  let bindings =
    compile_runtime_bindings(&config)?;
  info!(
    total_note_bindings =
      bindings.note_bindings.len(),
    "bindings compiled"
  );

  let audio =
    AudioEngine::new(&config.audio)?;
  run_event_loop(
    &config, &bindings, &audio
  )?;

  info!("shutdown complete");
  Ok(())
}

fn configured_config_path() -> PathBuf {
  env::var("SYMPOSIUM_CONFIG")
    .map(PathBuf::from)
    .unwrap_or_else(|_| {
      PathBuf::from(DEFAULT_CONFIG_PATH)
    })
}

fn init_tracing(
  config: &AppConfig
) -> Result<WorkerGuard> {
  let filter =
    EnvFilter::try_from_default_env()
      .unwrap_or_else(|_| {
        EnvFilter::new(
          config.logging.level.clone()
        )
      });

  std::fs::create_dir_all(Path::new(
    &config.logging.directory
  ))
  .with_context(|| {
    format!(
      "failed creating {}",
      config.logging.directory
    )
  })?;
  let file_appender =
    tracing_appender::rolling::daily(
      &config.logging.directory,
      "symposium"
    );
  let (file_writer, guard) =
    tracing_appender::non_blocking(
      file_appender
    );

  let console_layer = fmt::layer()
    .with_target(true)
    .with_file(true)
    .with_line_number(true)
    .with_thread_ids(true)
    .with_thread_names(true);

  let file_layer = fmt::layer()
    .with_ansi(false)
    .with_target(true)
    .with_file(true)
    .with_line_number(true)
    .with_thread_ids(true)
    .with_thread_names(true)
    .with_writer(file_writer);

  tracing_subscriber::registry()
    .with(filter)
    .with(console_layer)
    .with(file_layer)
    .init();

  Ok(guard)
}

fn compile_runtime_bindings(
  config: &AppConfig
) -> Result<RuntimeBindings> {
  let note_bindings =
    compile_note_bindings(
      &config.keybindings
    )?;
  let quit = compile_chord_set(
    &config.control_bindings.quit,
    "quit"
  )?;
  let list_songs = compile_chord_set(
    &config.control_bindings.list_songs,
    "list_songs"
  )?;
  let print_bindings =
    compile_chord_set(
      &config
        .control_bindings
        .print_bindings,
      "print_bindings"
    )?;

  Ok(RuntimeBindings {
    note_bindings,
    quit,
    list_songs,
    print_bindings
  })
}

fn run_event_loop(
  config: &AppConfig,
  bindings: &RuntimeBindings,
  audio: &AudioEngine
) -> Result<()> {
  let _terminal =
    TerminalSession::enter()?;
  print_startup_banner(
    config, bindings
  )?;

  loop {
    if !event::poll(
      Duration::from_millis(
        config.app.poll_interval_ms
      )
    )
    .context(
      "failed while polling events"
    )? {
      continue;
    }

    match event::read().context(
      "failed while reading terminal \
       event"
    )? {
      | Event::Key(key_event) => {
        if !should_handle_key_event(
          key_event,
          config.input.allow_key_repeat
        ) {
          trace!(
            ?key_event,
            "ignored key event due to \
             key kind"
          );
          continue;
        }

        let should_quit =
          handle_key_event(
            key_event, config,
            bindings, audio
          )?;
        if should_quit {
          break;
        }
      }
      | Event::Resize(
        width,
        height
      ) => {
        debug!(
          width,
          height, "terminal resized"
        );
      }
      | other => {
        trace!(?other, "non-key event");
      }
    }
  }

  Ok(())
}

fn should_handle_key_event(
  key_event: KeyEvent,
  allow_key_repeat: bool
) -> bool {
  match key_event.kind {
    | KeyEventKind::Press => true,
    | KeyEventKind::Repeat => {
      allow_key_repeat
    }
    | KeyEventKind::Release => false
  }
}

fn handle_key_event(
  key_event: KeyEvent,
  config: &AppConfig,
  bindings: &RuntimeBindings,
  audio: &AudioEngine
) -> Result<bool> {
  let Some(chord) =
    KeyChord::from_event(
      key_event,
      config
        .input
        .ignore_shift_for_char_keys
    )
  else {
    trace!(
      ?key_event,
      "ignored event that did not \
       produce a chord"
    );
    return Ok(false);
  };

  trace!(%chord, ?key_event, "keyboard chord received");

  if bindings.quit.contains(&chord) {
    info!(%chord, "quit chord received");
    println!();
    println!("Exiting Symposium.");
    io::stdout().flush().context(
      "failed flushing stdout"
    )?;
    return Ok(true);
  }

  if bindings
    .list_songs
    .contains(&chord)
  {
    info!(%chord, "list songs chord received");
    print_song_list(config)?;
    return Ok(false);
  }

  if bindings
    .print_bindings
    .contains(&chord)
  {
    info!(%chord, "print bindings chord received");
    print_binding_table(bindings)?;
    return Ok(false);
  }

  if let Some(midi_note) = bindings
    .note_bindings
    .get(&chord)
    .copied()
  {
    let note_name =
      midi_note_name(midi_note);
    info!(
        %chord,
        midi_note,
        note_name = %note_name,
        "mapped key pressed",
    );
    audio.play_note(
      midi_note,
      config.audio.master_volume,
      config.audio.note_duration_ms
    );

    println!(
      "{} -> {} ({})",
      chord, note_name, midi_note
    );
    io::stdout().flush().context(
      "failed flushing stdout"
    )?;
    return Ok(false);
  }

  if config.app.print_unmapped_keys {
    debug!(%chord, "unmapped key chord");
    println!(
      "{chord} is not mapped to a note"
    );
    io::stdout().flush().context(
      "failed flushing stdout"
    )?;
  } else {
    trace!(%chord, "unmapped key chord");
  }

  Ok(false)
}

fn print_startup_banner(
  config: &AppConfig,
  bindings: &RuntimeBindings
) -> Result<()> {
  let mut sorted_bindings = bindings
    .note_bindings
    .iter()
    .map(|(chord, midi_note)| {
      (*midi_note, chord.to_string())
    })
    .collect::<Vec<_>>();
  sorted_bindings.sort_unstable_by(
    |left, right| {
      left
        .0
        .cmp(&right.0)
        .then(left.1.cmp(&right.1))
    }
  );

  println!("Symposium Terminal Piano");
  println!(
    "Press configured keys to play \
     notes."
  );
  println!(
    "Controls: quit={:?}, \
     list_songs={:?}, \
     print_bindings={:?}",
    config.control_bindings.quit,
    config.control_bindings.list_songs,
    config
      .control_bindings
      .print_bindings,
  );
  println!(
    "Audio: waveform={:?}, volume={}, \
     note_duration_ms={}",
    config.audio.waveform,
    config.audio.master_volume,
    config.audio.note_duration_ms,
  );
  println!(
    "Loaded {} note bindings.",
    sorted_bindings.len()
  );
  println!(
    "Press F2 (default) to print all \
     bindings."
  );
  println!();
  io::stdout().flush().context(
    "failed flushing stdout"
  )?;
  Ok(())
}

fn print_binding_table(
  bindings: &RuntimeBindings
) -> Result<()> {
  let mut sorted = bindings
    .note_bindings
    .iter()
    .map(|(chord, midi_note)| {
      (*midi_note, chord.to_string())
    })
    .collect::<Vec<_>>();
  sorted.sort_unstable_by(
    |left, right| {
      left
        .0
        .cmp(&right.0)
        .then(left.1.cmp(&right.1))
    }
  );

  println!();
  println!("Active note bindings:");
  for (midi_note, chord) in sorted {
    println!(
      "  {:>3} {:<4} <- {}",
      midi_note,
      midi_note_name(midi_note),
      chord,
    );
  }
  println!();
  io::stdout().flush().context(
    "failed flushing stdout"
  )?;
  Ok(())
}

fn print_song_list(
  config: &AppConfig
) -> Result<()> {
  println!();
  println!("Configured songs:");
  if config.songs.is_empty() {
    println!("  (none)");
  } else {
    for song in &config.songs {
      println!(
        "  [{}] {} @{} BPM",
        song.id,
        song.title,
        song.tempo_bpm
      );
      println!(
        "      {}",
        song.notation
      );
    }
  }
  println!();
  io::stdout().flush().context(
    "failed flushing stdout"
  )?;
  Ok(())
}

fn midi_note_name(
  midi_note: u8
) -> String {
  const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F",
    "F#", "G", "G#", "A", "A#", "B"
  ];

  let note_name = NOTE_NAMES
    [usize::from(midi_note % 12)];
  let octave =
    i16::from(midi_note / 12) - 1;
  format!("{note_name}{octave}")
}
