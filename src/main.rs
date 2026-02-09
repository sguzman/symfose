mod audio;
mod config;
mod input;
mod songs;

use std::cell::RefCell;
use std::collections::{
  BTreeMap,
  HashMap,
  HashSet
};
use std::env;
use std::path::{
  Path,
  PathBuf
};

use anyhow::{
  Context,
  Result
};
use iced::widget::{
  button,
  column,
  container,
  row,
  scrollable,
  text
};
use iced::{
  Element,
  Length,
  Subscription,
  Task,
  Theme,
  event,
  keyboard
};
use tracing::{
  debug,
  info,
  trace
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
use crate::songs::{
  LoadedSong,
  load_song_library
};

#[derive(Debug)]
struct RuntimeBindings {
  note_bindings:  HashMap<KeyChord, u8>,
  note_to_chords:
    BTreeMap<u8, Vec<String>>,
  quit:           HashSet<KeyChord>,
  list_songs:     HashSet<KeyChord>,
  print_bindings: HashSet<KeyChord>,
  play_song:      HashSet<KeyChord>
}

struct PianoApp {
  config:         AppConfig,
  bindings:       RuntimeBindings,
  songs:          Vec<LoadedSong>,
  audio:          AudioEngine,
  selected_song:  Option<usize>,
  active_notes:   HashSet<u8>,
  activity:       Vec<String>,
  startup_notice: String
}

#[derive(Debug, Clone)]
enum Message {
  RuntimeEvent(
    iced::Event,
    iced::event::Status
  ),
  SelectSong(usize),
  PlaySelectedSong
}

fn main() -> Result<()> {
  let config_path =
    configured_config_path();

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

  info!(config_path = %config_path.display(), "booting Symposium GUI");

  let bindings =
    compile_runtime_bindings(&config)?;
  info!(
    note_bindings =
      bindings.note_bindings.len(),
    "bindings compiled"
  );

  let songs = load_song_library(
    &config.song_library
  )
  .with_context(|| {
    format!(
      "failed loading songs from {}",
      config.song_library.directory
    )
  })?;

  let audio =
    AudioEngine::new(&config.audio)?;

  let initial_state = PianoApp {
    startup_notice: format!(
      "Loaded {} song(s) from {}",
      songs.len(),
      config.song_library.directory
    ),
    selected_song: if songs.is_empty() {
      None
    } else {
      Some(0)
    },
    config,
    bindings,
    songs,
    audio,
    active_notes: HashSet::new(),
    activity: vec![
      "Press mapped keys to play. \
       Press F5 to play selected song."
        .to_string(),
    ]
  };

  let state_slot =
    RefCell::new(Some(initial_state));

  iced::application(
    move || {
      state_slot
        .borrow_mut()
        .take()
        .expect(
          "application state should \
           only be initialized once"
        )
    },
    update,
    view
  )
  .title(app_title)
  .window_size((1320.0, 780.0))
  .centered()
  .theme(app_theme)
  .subscription(subscription)
  .run()
  .context(
    "failed running GUI application"
  )?;

  info!("shutdown complete");
  Ok(())
}

fn update(
  app: &mut PianoApp,
  message: Message
) -> Task<Message> {
  match message {
    | Message::RuntimeEvent(
      event,
      status
    ) => {
      if let Some(task) =
        handle_runtime_event(
          app, event, status
        )
      {
        return task;
      }
    }
    | Message::SelectSong(index) => {
      app.selected_song = Some(index);
      if let Some(song) =
        app.songs.get(index)
      {
        let song_id =
          song.song.meta.id.clone();
        let song_title =
          song.song.meta.title.clone();
        let line = format!(
          "Selected song: {}",
          song_title
        );
        app.push_activity(line.clone());
        info!(song_id = %song_id, title = %song_title, "song selected");
      }
    }
    | Message::PlaySelectedSong => {
      if let Some(song_index) =
        app.selected_song
      {
        if let Some(song) =
          app.songs.get(song_index)
        {
          let song_id =
            song.song.meta.id.clone();
          let song_title = song
            .song
            .meta
            .title
            .clone();
          app
            .audio
            .play_song(&song.song);
          let line = format!(
            "Playing song preview: {}",
            song_title
          );
          app.push_activity(
            line.clone()
          );
          info!(song_id = %song_id, title = %song_title, "song playback started");
        }
      } else {
        app.push_activity(
          "No song selected."
            .to_string()
        );
      }
    }
  }

  Task::none()
}

fn handle_runtime_event(
  app: &mut PianoApp,
  event: iced::Event,
  status: iced::event::Status
) -> Option<Task<Message>> {
  match event {
    | iced::Event::Keyboard(
      keyboard::Event::KeyPressed {
        key,
        modifiers,
        repeat,
        ..
      }
    ) => {
      if status
        == iced::event::Status::Captured
      {
        trace!(
          ?key,
          ?modifiers,
          "keyboard event captured by \
           widget"
        );
        return None;
      }

      if repeat
        && !app
          .config
          .input
          .allow_key_repeat
      {
        trace!(
          ?key,
          "ignored repeated key event"
        );
        return None;
      }

      let Some(chord) =
        KeyChord::from_key_event(
          &key,
          modifiers,
          app
            .config
            .input
            .ignore_shift_for_char_keys
        )
      else {
        return None;
      };

      debug!(%chord, ?key, "key pressed");

      if app
        .bindings
        .quit
        .contains(&chord)
      {
        info!(%chord, "quit chord received");
        app.push_activity(
          "Quit requested from \
           keyboard chord."
            .to_string()
        );
        return Some(iced::exit());
      }

      if app
        .bindings
        .list_songs
        .contains(&chord)
      {
        app.select_next_song();
        return None;
      }

      if app
        .bindings
        .print_bindings
        .contains(&chord)
      {
        let count = app
          .bindings
          .note_bindings
          .len();
        app.push_activity(format!(
          "Loaded {count} key \
           bindings. See left panel \
           for mapping."
        ));
        return None;
      }

      if app
        .bindings
        .play_song
        .contains(&chord)
      {
        return Some(Task::done(
          Message::PlaySelectedSong
        ));
      }

      if let Some(midi_note) = app
        .bindings
        .note_bindings
        .get(&chord)
        .copied()
      {
        app
          .active_notes
          .insert(midi_note);
        app.audio.play_note(midi_note);

        let label = format!(
          "{chord} -> {} ({midi_note})",
          midi_note_name(midi_note)
        );
        app
          .push_activity(label.clone());
        info!(%chord, midi_note, note = %midi_note_name(midi_note), "mapped key pressed");
      } else if app
        .config
        .app
        .print_unmapped_keys
      {
        let line = format!(
          "Unmapped chord: {chord}"
        );
        app.push_activity(line.clone());
        debug!(%chord, "unmapped key chord");
      }
    }
    | iced::Event::Keyboard(
      keyboard::Event::KeyReleased {
        key,
        modifiers,
        ..
      }
    ) => {
      let Some(chord) =
        KeyChord::from_key_event(
          &key,
          modifiers,
          app
            .config
            .input
            .ignore_shift_for_char_keys
        )
      else {
        return None;
      };

      if let Some(midi_note) = app
        .bindings
        .note_bindings
        .get(&chord)
        .copied()
      {
        app
          .active_notes
          .remove(&midi_note);
      }
    }
    | iced::Event::Window(
      iced::window::Event::Resized(
        size
      )
    ) => {
      trace!(
        width = size.width,
        height = size.height,
        "window resized"
      );
    }
    | _ => {}
  }

  None
}

fn view(
  app: &PianoApp
) -> Element<'_, Message> {
  let header = container(
    column![
      text("Symposium Virtual Piano")
        .size(34),
      text(
        "Keyboard-driven piano with \
         configurable bindings, song \
         library, and piano-model \
         synthesis."
      )
      .size(16),
      text(&app.startup_notice)
        .size(14),
    ]
    .spacing(6)
  )
  .padding(16)
  .width(Length::Fill)
  .style(container::primary);

  let main_content = row![
    controls_panel(app),
    piano_panel(app),
    songs_panel(app),
  ]
  .spacing(16)
  .height(Length::Fill)
  .width(Length::Fill);

  container(
    column![header, main_content]
      .spacing(16)
  )
  .padding(16)
  .height(Length::Fill)
  .width(Length::Fill)
  .into()
}

fn controls_panel(
  app: &PianoApp
) -> Element<'_, Message> {
  let mut binding_rows =
    column![text("Bindings").size(22)]
      .spacing(4);

  for (note, chords) in
    &app.bindings.note_to_chords
  {
    let chord_list = chords.join(", ");
    binding_rows =
      binding_rows.push(text(format!(
        "{:>3} {:<4} <- {chord_list}",
        note,
        midi_note_name(*note)
      )));
  }

  let mut activity_rows =
    column![text("Activity").size(22)]
      .spacing(4);
  for line in app.activity.iter().rev()
  {
    activity_rows =
      activity_rows.push(text(line));
  }

  let controls = column![
    text("Controls").size(22),
    text(format!(
      "Quit: {}",
      app
        .config
        .control_bindings
        .quit
        .join(" or ")
    )),
    text(format!(
      "Next Song: {}",
      app
        .config
        .control_bindings
        .list_songs
        .join(" or ")
    )),
    text(format!(
      "Play Selected Song: {}",
      app
        .config
        .control_bindings
        .play_song
        .join(" or ")
    )),
    text(format!(
      "Print Bindings Hint: {}",
      app
        .config
        .control_bindings
        .print_bindings
        .join(" or ")
    )),
  ]
  .spacing(4);

  container(
    scrollable(
      column![
        controls,
        binding_rows,
        activity_rows
      ]
      .spacing(14)
    )
    .height(Length::Fill)
    .width(Length::Fill)
  )
  .padding(12)
  .width(Length::FillPortion(4))
  .height(Length::Fill)
  .style(container::rounded_box)
  .into()
}

fn piano_panel(
  app: &PianoApp
) -> Element<'_, Message> {
  let mut row_keys = row!().spacing(8);

  for (note, chords) in
    &app.bindings.note_to_chords
  {
    let active =
      app.active_notes.contains(note);
    let label = chords.join(" / ");

    let key_card = container(
      column![
        text(midi_note_name(*note))
          .size(20),
        text(format!("MIDI {note}"))
          .size(14),
        text(label).size(13),
      ]
      .spacing(3)
    )
    .width(86)
    .padding(8)
    .style(
      if active {
        container::success
      } else {
        container::bordered_box
      }
    );

    row_keys = row_keys.push(key_card);
  }

  let active_line = if app
    .active_notes
    .is_empty()
  {
    "(none)".to_string()
  } else {
    let mut active = app
      .active_notes
      .iter()
      .copied()
      .collect::<Vec<_>>();
    active.sort_unstable();

    active
      .iter()
      .map(|note| midi_note_name(*note))
      .collect::<Vec<_>>()
      .join(", ")
  };

  container(
    column![
      text("Piano").size(22),
      text(format!(
        "Instrument: {:?}",
        app.config.audio.instrument
      )),
      text(format!(
        "Active notes: {active_line}"
      )),
      scrollable(row_keys)
        .height(Length::Fill),
    ]
    .spacing(8)
    .height(Length::Fill)
  )
  .padding(12)
  .width(Length::FillPortion(6))
  .height(Length::Fill)
  .style(container::rounded_box)
  .into()
}

fn songs_panel(
  app: &PianoApp
) -> Element<'_, Message> {
  let mut songs_column =
    column![text("Songs").size(22)]
      .spacing(6);

  if app.songs.is_empty() {
    songs_column =
      songs_column.push(text(
        "No song files found in \
         res/songs."
      ));
  } else {
    for (index, loaded) in
      app.songs.iter().enumerate()
    {
      let selected = app.selected_song
        == Some(index);
      let marker = if selected {
        "*"
      } else {
        " "
      };
      let caption = format!(
        "{marker} {} ({:.0} BPM)",
        loaded.song.meta.title,
        loaded.song.meta.tempo_bpm
      );

      songs_column = songs_column.push(
        button(text(caption)).on_press(
          Message::SelectSong(index)
        )
      );
    }
  }

  let details = if let Some(index) =
    app.selected_song
  {
    if let Some(loaded) =
      app.songs.get(index)
    {
      let beats =
        loaded.duration_beats();
      let seconds = beats
        * (60.0
          / loaded
            .song
            .meta
            .tempo_bpm
            .max(1.0));

      column![
        text("Selected Song").size(22),
        text(format!(
          "ID: {}",
          loaded.song.meta.id
        )),
        text(format!(
          "Title: {}",
          loaded.song.meta.title
        )),
        text(format!(
          "Artist: {}",
          loaded.song.meta.artist
        )),
        text(format!(
          "Tempo: {:.0} BPM",
          loaded.song.meta.tempo_bpm
        )),
        text(format!(
          "Time Signature: {}/{}",
          loaded
            .song
            .meta
            .beats_per_bar,
          loaded.song.meta.beat_unit
        )),
        text(format!(
          "Events: {}",
          loaded.song.events.len()
        )),
        text(format!(
          "Duration: {:.2} beats \
           ({seconds:.2}s)",
          beats
        )),
        text(format!(
          "File: {}",
          loaded.path.display()
        )),
        button(text(
          "Play Selected Song Preview"
        ))
        .on_press(
          Message::PlaySelectedSong
        ),
      ]
      .spacing(4)
    } else {
      column![text("No song selected.")]
    }
  } else {
    column![text("No song selected.")]
  };

  container(
    scrollable(
      column![songs_column, details]
        .spacing(14)
    )
    .height(Length::Fill)
  )
  .padding(12)
  .width(Length::FillPortion(4))
  .height(Length::Fill)
  .style(container::rounded_box)
  .into()
}

fn subscription(
  _app: &PianoApp
) -> Subscription<Message> {
  event::listen_with(map_event)
}

fn map_event(
  event: iced::Event,
  status: iced::event::Status,
  _window: iced::window::Id
) -> Option<Message> {
  Some(Message::RuntimeEvent(
    event, status
  ))
}

fn app_title(
  _state: &PianoApp
) -> String {
  "Symposium - Virtual Piano"
    .to_string()
}

fn app_theme(
  _state: &PianoApp
) -> Theme {
  Theme::Light
}

impl PianoApp {
  fn push_activity(
    &mut self,
    line: String
  ) {
    self.activity.push(line);

    const MAX_ENTRIES: usize = 30;
    if self.activity.len() > MAX_ENTRIES
    {
      let overflow =
        self.activity.len()
          - MAX_ENTRIES;
      self.activity.drain(0..overflow);
    }
  }

  fn select_next_song(&mut self) {
    if self.songs.is_empty() {
      self.push_activity(
        "No songs available to select."
          .to_string()
      );
      return;
    }

    let next = match self.selected_song
    {
      | Some(current) => {
        (current + 1) % self.songs.len()
      }
      | None => 0
    };

    self.selected_song = Some(next);

    if let Some(song) =
      self.songs.get(next)
    {
      let song_id =
        song.song.meta.id.clone();
      let song_title =
        song.song.meta.title.clone();
      let line = format!(
        "Selected song: {}",
        song_title
      );
      self.push_activity(line.clone());
      info!(song_id = %song_id, title = %song_title, "song selected via shortcut");
    }
  }
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
  let play_song = compile_chord_set(
    &config.control_bindings.play_song,
    "play_song"
  )?;

  let mut note_to_chords =
    BTreeMap::<u8, Vec<String>>::new();
  for (chord, note) in &note_bindings {
    note_to_chords
      .entry(*note)
      .or_default()
      .push(chord.to_string());
  }

  for chords in
    note_to_chords.values_mut()
  {
    chords.sort_unstable();
  }

  Ok(RuntimeBindings {
    note_bindings,
    note_to_chords,
    quit,
    list_songs,
    print_bindings,
    play_song
  })
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
