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
use std::fmt::{
  Display,
  Formatter,
  Result as FmtResult
};
use std::path::{
  Path,
  PathBuf
};
use std::time::{
  Duration,
  Instant
};

use anyhow::{
  Context,
  Result
};
use iced::widget::{
  button,
  column,
  container,
  mouse_area,
  pick_list,
  row,
  scrollable,
  slider,
  space,
  stack,
  text,
  text_input,
  toggler
};
use iced::{
  Color,
  Element,
  Length,
  Subscription,
  Task,
  Theme,
  border,
  event,
  keyboard,
  time
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
  fmt as tracing_fmt
};

use crate::audio::AudioEngine;
use crate::config::{
  AppConfig,
  DEFAULT_CONFIG_PATH,
  KeyboardLayout
};
use crate::input::{
  KeyChord,
  compile_chord_set,
  compile_note_bindings
};
use crate::songs::{
  LoadedSong,
  SongFile,
  load_song_library
};

const FLASH_DURATION: Duration =
  Duration::from_millis(170);
const TICK_RATE: Duration =
  Duration::from_millis(16);
const TIMER_WINDOW_SECONDS: f32 = 0.18;
const TIMER_PERFECT_SECONDS: f32 = 0.07;

const WHITE_KEY_WIDTH: f32 = 72.0;
const WHITE_KEY_HEIGHT: f32 = 250.0;
const BLACK_KEY_WIDTH: f32 = 44.0;
const BLACK_KEY_HEIGHT: f32 = 152.0;

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
  config: AppConfig,
  bindings: RuntimeBindings,
  songs: Vec<LoadedSong>,
  audio: AudioEngine,
  selected_song: Option<usize>,
  prepared_song: Option<PreparedSong>,
  held_notes: HashSet<u8>,
  flashed_notes: HashMap<u8, Instant>,
  activity: Vec<String>,
  startup_notice: String,
  song_search_query: String,
  instrument_options: Vec<String>,
  selected_instrument: String,
  transpose_song_to_fit_bindings: bool,
  warn_on_missing_song_notes: bool,
  optimize_bindings_for_song: bool,
  prepared_transpose_semitones: i8,
  missing_song_notes: Vec<u8>,
  play_mode: PlayMode,
  tutorial_options: TutorialOptions,
  playback: Option<PlaybackState>,
  last_timer_score: Option<TimerScore>,
  volume: f32
}

#[derive(Debug, Clone)]
struct PreparedSong {
  events:           Vec<PreparedEvent>,
  expected_notes:   Vec<ExpectedNote>,
  duration_seconds: f32,
  beat_seconds:     f32
}

#[derive(Debug, Clone)]
struct PreparedEvent {
  at_seconds:       f32,
  duration_seconds: f32,
  duration_ms:      u64,
  velocity:         u8,
  notes:            Vec<u8>
}

#[derive(Debug, Clone)]
struct ExpectedNote {
  at_seconds: f32,
  midi_note:  u8
}

#[derive(
  Debug, Clone, Copy, PartialEq, Eq,
)]
enum PlayMode {
  Timer,
  Tutorial,
  Autoplay
}

impl PlayMode {
  const ALL: [PlayMode; 3] = [
    PlayMode::Timer,
    PlayMode::Tutorial,
    PlayMode::Autoplay
  ];
}

impl Display for PlayMode {
  fn fmt(
    &self,
    f: &mut Formatter<'_>
  ) -> FmtResult {
    let label = match self {
      | PlayMode::Timer => "Timer",
      | PlayMode::Tutorial => {
        "Tutorial"
      }
      | PlayMode::Autoplay => {
        "Auto Play"
      }
    };

    write!(f, "{label}")
  }
}

#[derive(Debug, Clone, Copy)]
struct TutorialOptions {
  only_advance_on_correct_note: bool,
  play_bad_notes_out_loud:      bool
}

impl Default for TutorialOptions {
  fn default() -> Self {
    Self {
      only_advance_on_correct_note:
        true,
      play_bad_notes_out_loud:
        true
    }
  }
}

#[derive(Debug, Clone)]
struct TimerScore {
  expected_notes: usize,
  hit_notes:      usize,
  perfect_hits:   usize,
  good_hits:      usize,
  wrong_notes:    usize,
  missed_notes:   usize
}

impl TimerScore {
  fn new(
    expected_notes: usize
  ) -> Self {
    Self {
      expected_notes,
      hit_notes: 0,
      perfect_hits: 0,
      good_hits: 0,
      wrong_notes: 0,
      missed_notes: 0
    }
  }

  fn accuracy_percent(&self) -> f32 {
    if self.expected_notes == 0 {
      return 0.0;
    }

    (self.hit_notes as f32
      / self.expected_notes as f32)
      * 100.0
  }
}

#[derive(Debug)]
struct PlaybackState {
  mode:                  PlayMode,
  started_at:            Instant,
  cursor_seconds:        f32,
  next_event_index:      usize,
  tutorial_event_index:  usize,
  tutorial_matched:      HashSet<u8>,
  next_metronome_beat_s: f32,
  next_metronome_index:  u64,
  matched_note_indices:  HashSet<usize>,
  score:                 TimerScore
}

impl PlaybackState {
  fn new(
    mode: PlayMode,
    prepared: &PreparedSong
  ) -> Self {
    Self {
      mode,
      started_at: Instant::now(),
      cursor_seconds: 0.0,
      next_event_index: 0,
      tutorial_event_index: 0,
      tutorial_matched: HashSet::new(),
      next_metronome_beat_s: 0.0,
      next_metronome_index: 0,
      matched_note_indices:
        HashSet::new(),
      score: TimerScore::new(
        prepared.expected_notes.len()
      )
    }
  }
}

#[derive(Debug, Clone)]
enum Message {
  RuntimeEvent(
    iced::Event,
    iced::event::Status
  ),
  SelectSong(usize),
  StartPlayback,
  RestartPlayback,
  StopPlayback,
  VolumeChanged(f32),
  PlayModeSelected(PlayMode),
  TutorialAdvanceOnlyCorrectChanged(
    bool
  ),
  TutorialPlayBadNotesChanged(bool),
  TransposeSongToFitBindingsChanged(
    bool
  ),
  WarnOnMissingSongNotesChanged(bool),
  OptimizeBindingsForSongChanged(bool),
  PlayNoteFromClick(u8),
  SongSearchChanged(String),
  ApplySongTagFilter(String),
  InstrumentSelected(String),
  Tick(Instant)
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

  info!(config_path = %config_path.display(), "booting Symfose GUI");

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
  let instrument_options =
    audio.available_profiles();
  let selected_instrument = audio
    .active_profile_name()
    .to_string();

  let selected_song =
    if songs.is_empty() {
      None
    } else {
      Some(0)
    };

  let mut initial_state = PianoApp {
    startup_notice: format!(
      "Loaded {} song(s) from \
       sources: {}, {} (cache: {})",
      songs.len(),
      config.song_library.directory,
      config
        .song_library
        .midi_directory,
      config
        .song_library
        .cache_directory
    ),
    selected_song,
    prepared_song: None,
    volume: audio.master_volume(),
    song_search_query: String::new(),
    instrument_options,
    selected_instrument,
    transpose_song_to_fit_bindings:
      config
        .gameplay
        .transpose_song_to_fit_bindings,
    warn_on_missing_song_notes: config
      .gameplay
      .warn_on_missing_song_notes,
    optimize_bindings_for_song: config
      .gameplay
      .optimize_bindings_for_song,
    prepared_transpose_semitones: 0,
    missing_song_notes: Vec::new(),
    config,
    bindings,
    songs,
    audio,
    held_notes: HashSet::new(),
    flashed_notes: HashMap::new(),
    activity: vec![
      "Press mapped keys to play. \
       Choose a song mode and press \
       Start."
        .to_string(),
    ],
    play_mode: PlayMode::Timer,
    tutorial_options:
      TutorialOptions::default(),
    playback: None,
    last_timer_score: None
  };
  initial_state.rebuild_song_context();

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
  .window_size((1380.0, 860.0))
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
      app.select_song(index);
    }
    | Message::StartPlayback => {
      app.start_playback();
    }
    | Message::RestartPlayback => {
      app.start_playback();
    }
    | Message::StopPlayback => {
      app.stop_playback();
    }
    | Message::VolumeChanged(volume) => {
      app.set_volume(volume);
    }
    | Message::PlayModeSelected(mode) => {
      app.play_mode = mode;
      app.push_activity(format!(
        "Mode selected: {mode}"
      ));
      info!(?mode, "play mode selected");
    }
    | Message::TutorialAdvanceOnlyCorrectChanged(
      value
    ) => {
      app
        .tutorial_options
        .only_advance_on_correct_note =
        value;
      info!(value, "tutorial only_advance_on_correct_note updated");
    }
    | Message::TutorialPlayBadNotesChanged(
      value
    ) => {
      app
        .tutorial_options
        .play_bad_notes_out_loud =
        value;
      info!(value, "tutorial play_bad_notes_out_loud updated");
    }
    | Message::TransposeSongToFitBindingsChanged(
      value
    ) => {
      app.transpose_song_to_fit_bindings =
        value;
      app.rebuild_song_context();
      info!(value, "transpose_song_to_fit_bindings updated");
    }
    | Message::WarnOnMissingSongNotesChanged(
      value
    ) => {
      app.warn_on_missing_song_notes =
        value;
      info!(value, "warn_on_missing_song_notes updated");
    }
    | Message::OptimizeBindingsForSongChanged(
      value
    ) => {
      app.optimize_bindings_for_song =
        value;
      app.rebuild_song_context();
      info!(value, "optimize_bindings_for_song updated");
    }
    | Message::PlayNoteFromClick(
      midi_note
    ) => {
      app.flash_note(midi_note);
      let play_out_loud = app
        .process_note_input(midi_note);
      if play_out_loud {
        app.audio.play_note(midi_note);
      }

      let line = format!(
        "click -> {} ({midi_note})",
        midi_note_name(midi_note)
      );
      app.push_activity(line);
      info!(midi_note, note = %midi_note_name(midi_note), "piano key clicked");
    }
    | Message::SongSearchChanged(
      query
    ) => {
      app.song_search_query = query;
    }
    | Message::ApplySongTagFilter(
      tag
    ) => {
      app.song_search_query = tag;
    }
    | Message::InstrumentSelected(
      instrument
    ) => {
      match app
        .audio
        .set_active_profile(
          &instrument
        )
      {
        | Ok(()) => {
          app.selected_instrument =
            instrument.clone();
          app.push_activity(format!(
            "Instrument switched to \
             {instrument}"
          ));
        }
        | Err(error) => {
          app.push_activity(format!(
            "Failed to switch \
             instrument: {error}"
          ));
        }
      }
    }
    | Message::Tick(now) => {
      app.handle_tick(now);
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
           bindings."
        ));
        return None;
      }

      if app
        .bindings
        .play_song
        .contains(&chord)
      {
        return Some(Task::done(
          Message::StartPlayback
        ));
      }

      if let Some(midi_note) = app
        .bindings
        .note_bindings
        .get(&chord)
        .copied()
      {
        app
          .held_notes
          .insert(midi_note);
        app.flash_note(midi_note);

        let play_out_loud = app
          .process_note_input(
            midi_note
          );
        if play_out_loud {
          app
            .audio
            .play_note(midi_note);
        }

        let label = format!(
          "{chord} -> {} ({midi_note})",
          midi_note_name(midi_note)
        );
        app.push_activity(label);

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
          .held_notes
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
      text("Symfose Virtual Piano")
        .size(34),
      text(
        "Virtual piano workflow with \
         timer scoring, tutorial \
         guidance, and auto play."
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
      "Keyboard: {}",
      app.config.keyboard.layout
    )),
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
      "Start Song Mode: {}",
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

  let mode_picker = pick_list(
    PlayMode::ALL,
    Some(app.play_mode),
    Message::PlayModeSelected
  )
  .placeholder("Mode")
  .width(Length::Fill);

  let playback_controls = row![
    button(text("Start"))
      .on_press(Message::StartPlayback),
    button(text("Restart")).on_press(
      Message::RestartPlayback
    ),
    button(text("Stop"))
      .on_press(Message::StopPlayback),
  ]
  .spacing(6);

  let mut more_options = column![
    text("More Options").size(22)
  ]
  .spacing(6)
  .push(mode_picker)
  .push(playback_controls)
  .push(
    toggler(
      app
        .transpose_song_to_fit_bindings
    )
    .label(
      "Transpose song to fit \
       playable keys"
    )
    .on_toggle(
      Message::TransposeSongToFitBindingsChanged
    )
  )
  .push(
    toggler(
      app
        .warn_on_missing_song_notes
    )
    .label(
      "Warn when selected song has \
       unmapped notes"
    )
    .on_toggle(
      Message::WarnOnMissingSongNotesChanged
    )
  )
  .push(
    toggler(
      app.optimize_bindings_for_song
    )
    .label(
      "Optimize key ergonomics for \
       selected song"
    )
    .on_toggle(
      Message::OptimizeBindingsForSongChanged
    )
  );

  if app.play_mode == PlayMode::Tutorial
  {
    more_options = more_options
      .push(
        toggler(
          app
            .tutorial_options
            .only_advance_on_correct_note
        )
        .label(
          "Only advance on correct \
           note"
        )
        .on_toggle(
          Message::TutorialAdvanceOnlyCorrectChanged
        )
      )
      .push(
        toggler(
          app
            .tutorial_options
            .play_bad_notes_out_loud
        )
        .label(
          "Play bad notes out loud"
        )
        .on_toggle(
          Message::TutorialPlayBadNotesChanged
        )
      );
  }

  container(
    scrollable(
      column![
        controls,
        more_options,
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
  let active_line = if app
    .held_notes
    .is_empty()
  {
    "(none)".to_string()
  } else {
    let mut active = app
      .held_notes
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

  let playback_status =
    app.playback_status_line();

  let header = row![
    column![
      text("Piano").size(22),
      text(format!(
        "Instrument: {}",
        app
          .audio
          .active_profile_summary()
      )),
      text(format!(
        "Held notes: {active_line}"
      )),
      text(playback_status),
    ]
    .spacing(4)
    .width(Length::FillPortion(4)),
    column![
      text("Instrument Profile"),
      pick_list(
        app.instrument_options.clone(),
        Some(
          app
            .selected_instrument
            .clone()
        ),
        Message::InstrumentSelected
      )
      .width(Length::Fill),
      text(format!(
        "Volume: {:.2}",
        app.volume
      )),
      slider(
        0.0..=2.5,
        app.volume,
        Message::VolumeChanged
      )
      .step(0.01)
      .height(22),
    ]
    .spacing(4)
    .width(Length::FillPortion(3)),
  ]
  .spacing(16);

  let timeline =
    song_timeline_panel(app);
  let keyboard = piano_keyboard(app);

  container(
    column![
      header,
      timeline,
      keyboard,
    ]
    .spacing(10)
    .height(Length::Fill)
  )
  .padding(12)
  .width(Length::FillPortion(8))
  .height(Length::Fill)
  .style(container::rounded_box)
  .into()
}

fn song_timeline_panel(
  app: &PianoApp
) -> Element<'_, Message> {
  let Some(prepared) =
    app.prepared_song.as_ref()
  else {
    return container(text(
      "Select a song to show timing \
       tiles."
    ))
    .padding(8)
    .style(container::bordered_box)
    .into();
  };

  if prepared.events.is_empty() {
    return container(text(
      "Selected song has no events."
    ))
    .padding(8)
    .style(container::bordered_box)
    .into();
  }

  let cursor = app
    .playback
    .as_ref()
    .map_or(0.0, |playback| {
      playback.cursor_seconds
    });

  let mut chips = row!().spacing(6);

  for (index, event) in
    prepared.events.iter().enumerate()
  {
    let width =
      (event.duration_seconds * 120.0)
        .clamp(64.0, 220.0);

    let notes = event
      .notes
      .iter()
      .map(|note| {
        app.primary_binding_label(*note)
      })
      .collect::<Vec<_>>()
      .join(" ");

    let is_current = app
      .playback
      .as_ref()
      .is_some_and(|state| {
        match state.mode {
          | PlayMode::Tutorial => {
            state.tutorial_event_index
              == index
          }
          | PlayMode::Timer
          | PlayMode::Autoplay => {
            event.at_seconds <= cursor
              && cursor
                < event.at_seconds
                  + event
                    .duration_seconds
                  + 0.08
          }
        }
      });

    let is_past = event.at_seconds
      + event.duration_seconds
      < cursor;

    let tile_style =
      timeline_tile_style(
        is_current, is_past
      );

    chips = chips.push(
      container(
        column![
          text(notes).size(22),
          text(format!(
            "{:.2}s",
            event.at_seconds
          ))
          .size(12),
        ]
        .spacing(2)
      )
      .width(width)
      .padding(6)
      .style(move |_| tile_style)
    );
  }

  let roll = scrollable(
    container(chips)
      .width(Length::Shrink)
  )
  .horizontal()
  .height(160)
  .width(Length::Fill);

  container(
    column![
      text(
        "Song Keys + Timing \
         (virtual-piano style lane)"
      )
      .size(16),
      roll,
    ]
    .spacing(6)
  )
  .padding(8)
  .style(container::bordered_box)
  .into()
}

fn piano_keyboard(
  app: &PianoApp
) -> Element<'_, Message> {
  let (min_note, max_note) =
    app.keyboard_note_range();

  let white_notes = (min_note
    ..=max_note)
    .filter(|note| is_white_key(*note))
    .collect::<Vec<_>>();

  let mut white_row = row!().spacing(1);
  for white_note in &white_notes {
    white_row =
      white_row.push(white_key_widget(
        app,
        *white_note
      ));
  }

  let mut black_overlay =
    row!().spacing(1);
  for white_note in &white_notes {
    if let Some(black_note) =
      black_key_after(*white_note)
    {
      if black_note >= min_note
        && black_note <= max_note
      {
        black_overlay = black_overlay
          .push(
            container(
              black_key_widget(
                app, black_note
              )
            )
            .width(WHITE_KEY_WIDTH)
            .center_x(WHITE_KEY_WIDTH)
          );
        continue;
      }
    }

    black_overlay = black_overlay.push(
      container(
        space().width(WHITE_KEY_WIDTH)
      )
      .width(WHITE_KEY_WIDTH)
    );
  }

  let white_count =
    white_notes.len().max(1) as f32;
  let keyboard_width = white_count
    * (WHITE_KEY_WIDTH + 1.0);

  let layers = stack([
    container(white_row)
      .height(WHITE_KEY_HEIGHT)
      .into(),
    container(black_overlay)
      .height(WHITE_KEY_HEIGHT)
      .align_y(iced::Top)
      .into()
  ])
  .width(keyboard_width)
  .height(WHITE_KEY_HEIGHT);

  let scroller = scrollable(
    container(layers)
      .width(keyboard_width)
      .height(WHITE_KEY_HEIGHT)
      .padding(2)
  )
  .horizontal()
  .height(WHITE_KEY_HEIGHT + 24.0)
  .width(Length::Fill);

  container(scroller)
    .padding(6)
    .style(container::bordered_box)
    .into()
}

fn white_key_widget<'a>(
  app: &PianoApp,
  note: u8
) -> Element<'a, Message> {
  let active =
    app.is_note_highlighted(note);
  let guided =
    app.guided_notes().contains(&note);

  let label =
    app.primary_binding_label(note);

  let style =
    white_key_style(active, guided);

  mouse_area(
    container(
      column![
        space().height(Length::Fill),
        text(label).size(18),
        text(midi_note_name(note))
          .size(12),
      ]
      .spacing(4)
    )
    .width(WHITE_KEY_WIDTH)
    .height(WHITE_KEY_HEIGHT)
    .padding([8, 6])
    .style(move |_| style)
  )
  .on_press(Message::PlayNoteFromClick(
    note
  ))
  .into()
}

fn black_key_widget<'a>(
  app: &PianoApp,
  note: u8
) -> Element<'a, Message> {
  let active =
    app.is_note_highlighted(note);
  let guided =
    app.guided_notes().contains(&note);

  let label =
    app.primary_binding_label(note);
  let style =
    black_key_style(active, guided);

  mouse_area(
    container(
      column![
        text(label).size(16),
        text(midi_note_name(note))
          .size(11),
      ]
      .spacing(2)
    )
    .width(BLACK_KEY_WIDTH)
    .height(BLACK_KEY_HEIGHT)
    .padding([8, 4])
    .style(move |_| style)
  )
  .on_press(Message::PlayNoteFromClick(
    note
  ))
  .into()
}

fn songs_panel(
  app: &PianoApp
) -> Element<'_, Message> {
  let filtered_indices =
    app.filtered_song_indices();
  let search_bar = row![
    text_input(
      "Search title, artist, id, tag",
      &app.song_search_query
    )
    .on_input(
      Message::SongSearchChanged
    )
    .width(Length::Fill),
    button(text("X")).on_press(
      Message::SongSearchChanged(
        String::new()
      )
    ),
  ]
  .spacing(6);

  let mut songs_column = column![
    text("Song Search").size(18),
    search_bar,
    text(format!(
      "Results: {} / {}",
      filtered_indices.len(),
      app.songs.len()
    )),
  ]
  .spacing(6);

  if filtered_indices.is_empty() {
    songs_column =
      songs_column.push(text(
        "No songs matched your search."
      ));
  } else {
    for index in filtered_indices {
      let loaded = &app.songs[index];
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
      let mut tag_row =
        row!().spacing(4);
      for tag in &loaded.song.meta.tags
      {
        let tag_text =
          text(tag.clone())
            .size(11)
            .color(Color::from_rgb(
              0.10, 0.62, 0.18
            ));
        tag_row = tag_row.push(
          button(tag_text)
            .padding([1, 6])
            .style(tag_chip_button_style)
            .on_press(
              Message::ApplySongTagFilter(
                tag.clone()
              )
            )
        );
      }

      songs_column = songs_column.push(
        row![
          button(text(caption))
            .width(Length::Fill)
            .on_press(
              Message::SelectSong(
                index
              )
            ),
          container(tag_row)
            .align_y(iced::Center),
        ]
        .spacing(6)
        .align_y(iced::Center)
      );
    }
  }

  let details =
    selected_song_details(app);

  let selected_pane =
    container(details)
      .padding(10)
      .style(container::rounded_box);
  let search_pane = container(
    scrollable(songs_column)
      .height(Length::Fill)
  )
  .padding(10)
  .style(container::rounded_box);

  container(
    column![
      selected_pane,
      search_pane,
    ]
    .spacing(10)
    .height(Length::Fill)
  )
  .padding(12)
  .width(Length::FillPortion(4))
  .height(Length::Fill)
  .style(container::rounded_box)
  .into()
}

fn selected_song_details(
  app: &PianoApp
) -> Element<'_, Message> {
  let Some(index) = app.selected_song
  else {
    return column![text(
      "No song selected."
    )]
    .into();
  };

  let Some(loaded) =
    app.songs.get(index)
  else {
    return column![text(
      "No song selected."
    )]
    .into();
  };

  let prepared =
    app.prepared_song.as_ref();
  let duration_seconds = prepared
    .map_or(0.0, |song| {
      song.duration_seconds
    });
  let duration_beats =
    loaded.duration_beats();

  let mut info_column = column![
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
      "Events: {}",
      loaded.song.events.len()
    )),
    text(format!(
      "Duration: {duration_seconds:.\
       2}s"
    )),
    text(format!(
      "Duration (beats): \
       {duration_beats:.2}"
    )),
    text(format!(
      "File: {}",
      loaded.path.display()
    )),
    text(format!(
      "Cursor: {:.2}s",
      app.playback.as_ref().map_or(
        0.0,
        |playback| {
          playback.cursor_seconds
        }
      )
    )),
    text(format!(
      "Transpose applied: {} \
       semitone(s)",
      app.prepared_transpose_semitones
    )),
  ]
  .spacing(4);

  if app.warn_on_missing_song_notes {
    if app.missing_song_notes.is_empty()
    {
      info_column =
        info_column.push(text(
          "Mapping check: all song \
           notes are currently \
           playable."
        ));
    } else {
      let list = app
        .missing_song_notes
        .iter()
        .map(|note| {
          format!(
            "{} ({})",
            midi_note_name(*note),
            note
          )
        })
        .collect::<Vec<_>>()
        .join(", ");
      info_column = info_column.push(
        text(format!(
          "Missing key mappings: \
           {list}"
        ))
      );
    }
  }

  if let Some(score) = app
    .playback
    .as_ref()
    .and_then(|playback| {
      (playback.mode == PlayMode::Timer)
        .then_some(&playback.score)
    })
  {
    info_column =
      info_column.push(text(format!(
        "Live score: hit {} / {} \
         ({:.1}%)",
        score.hit_notes,
        score.expected_notes,
        score.accuracy_percent()
      )));
  }

  if let Some(score) =
    &app.last_timer_score
  {
    info_column =
      info_column.push(text(format!(
        "Last timer result: {:.1}% \
         (perfect {} good {} wrong {} \
         missed {})",
        score.accuracy_percent(),
        score.perfect_hits,
        score.good_hits,
        score.wrong_notes,
        score.missed_notes
      )));
  }

  info_column.into()
}

fn subscription(
  _app: &PianoApp
) -> Subscription<Message> {
  Subscription::batch(vec![
    event::listen_with(map_event),
    time::every(TICK_RATE)
      .map(Message::Tick),
  ])
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
  "Symfose - Virtual Piano".to_string()
}

fn app_theme(
  _state: &PianoApp
) -> Theme {
  Theme::Light
}

impl PianoApp {
  fn filtered_song_indices(
    &self
  ) -> Vec<usize> {
    let needle = self
      .song_search_query
      .trim()
      .to_ascii_lowercase();

    self
      .songs
      .iter()
      .enumerate()
      .filter(|(_, loaded)| {
        if needle.is_empty() {
          return true;
        }

        let tags = loaded
          .song
          .meta
          .tags
          .join(" ")
          .to_ascii_lowercase();

        loaded
          .song
          .meta
          .title
          .to_ascii_lowercase()
          .contains(&needle)
          || loaded
            .song
            .meta
            .artist
            .to_ascii_lowercase()
            .contains(&needle)
          || loaded
            .song
            .meta
            .id
            .to_ascii_lowercase()
            .contains(&needle)
          || tags.contains(&needle)
      })
      .map(|(index, _)| index)
      .collect::<Vec<_>>()
  }

  fn rebuild_song_context(&mut self) {
    let mut bindings =
      match compile_runtime_bindings(
        &self.config
      ) {
        | Ok(compiled) => compiled,
        | Err(error) => {
          self.push_activity(format!(
            "Failed to compile \
             bindings: {error}"
          ));
          return;
        }
      };

    if self.optimize_bindings_for_song {
      if let Some(index) =
        self.selected_song
      {
        if let Some(song) =
          self.songs.get(index)
        {
          apply_song_ergonomic_bindings(
            &mut bindings,
            &song.song,
            self.config.keyboard.layout
          );
        }
      }
    }

    self.bindings = bindings;

    let (prepared, transpose, missing) =
      self
        .selected_song
        .and_then(|index| {
          self.songs.get(index)
        })
        .map_or(
          (None, 0i8, Vec::new()),
          |loaded| {
            prepare_song_for_bindings(
              &loaded.song,
              &self.bindings,
              self
                .transpose_song_to_fit_bindings
            )
          }
        );

    self.prepared_song = prepared;
    self.prepared_transpose_semitones =
      transpose;
    self.missing_song_notes = missing;

    if self.warn_on_missing_song_notes
      && !self
        .missing_song_notes
        .is_empty()
    {
      self.push_activity(format!(
        "Selected song has {} note(s) \
         without key mappings.",
        self.missing_song_notes.len()
      ));
    }
  }

  fn push_activity(
    &mut self,
    line: String
  ) {
    self.activity.push(line);

    const MAX_ENTRIES: usize = 40;
    if self.activity.len() > MAX_ENTRIES
    {
      let overflow =
        self.activity.len()
          - MAX_ENTRIES;
      self.activity.drain(0..overflow);
    }
  }

  fn set_volume(
    &mut self,
    volume: f32
  ) {
    let clamped =
      volume.clamp(0.0, 2.5);
    self.volume = clamped;
    self
      .audio
      .set_master_volume(clamped);
  }

  fn flash_note(
    &mut self,
    midi_note: u8
  ) {
    let expires =
      Instant::now() + FLASH_DURATION;
    self
      .flashed_notes
      .insert(midi_note, expires);
  }

  fn prune_flashes(
    &mut self,
    now: Instant
  ) {
    self.flashed_notes.retain(
      |_, expires| *expires > now
    );
  }

  fn is_note_highlighted(
    &self,
    note: u8
  ) -> bool {
    self.held_notes.contains(&note)
      || self
        .flashed_notes
        .get(&note)
        .is_some_and(|until| {
          *until > Instant::now()
        })
  }

  fn playback_status_line(
    &self
  ) -> String {
    match &self.playback {
      | Some(playback) => {
        format!(
          "Mode: {} | Cursor: {:.2}s",
          playback.mode,
          playback.cursor_seconds
        )
      }
      | None => {
        "Mode idle. Choose Timer, \
         Tutorial, or Auto Play."
          .to_string()
      }
    }
  }

  fn keyboard_note_range(
    &self
  ) -> (u8, u8) {
    let min_note = self
      .bindings
      .note_to_chords
      .keys()
      .next()
      .copied()
      .unwrap_or(60);
    let max_note = self
      .bindings
      .note_to_chords
      .keys()
      .next_back()
      .copied()
      .unwrap_or(76);

    (min_note, max_note)
  }

  fn primary_binding_label(
    &self,
    note: u8
  ) -> String {
    self
      .bindings
      .note_to_chords
      .get(&note)
      .and_then(|entries| {
        entries.first()
      })
      .cloned()
      .unwrap_or_else(|| {
        "-".to_string()
      })
  }

  fn guided_notes(
    &self
  ) -> HashSet<u8> {
    let mut notes = HashSet::new();

    let Some(playback) = &self.playback
    else {
      return notes;
    };
    let Some(prepared) =
      &self.prepared_song
    else {
      return notes;
    };

    match playback.mode {
      | PlayMode::Tutorial => {
        if let Some(event) =
          prepared.events.get(
            playback
              .tutorial_event_index
          )
        {
          notes.extend(
            event.notes.iter().copied()
          );
        }
      }
      | PlayMode::Timer
      | PlayMode::Autoplay => {
        let cursor =
          playback.cursor_seconds;
        for event in &prepared.events {
          if (event.at_seconds - cursor)
            .abs()
            <= 0.12
          {
            notes.extend(
              event
                .notes
                .iter()
                .copied()
            );
          }
        }
      }
    }

    notes
  }

  fn select_song(
    &mut self,
    index: usize
  ) {
    self.selected_song = Some(index);
    self.rebuild_song_context();

    self.playback = None;
    self.last_timer_score = None;

    if let Some(song) =
      self.songs.get(index)
    {
      let song_id =
        song.song.meta.id.clone();
      let song_title =
        song.song.meta.title.clone();
      let line = format!(
        "Selected song: {}",
        song_title
      );
      self.push_activity(line);
      info!(song_id = %song_id, title = %song_title, "song selected");
    }
  }

  fn select_next_song(&mut self) {
    let filtered =
      self.filtered_song_indices();
    if filtered.is_empty() {
      self.push_activity(
        "No songs available in \
         current search filter."
          .to_string()
      );
      return;
    }

    let next = match self.selected_song
    {
      | Some(current) => {
        let current_pos = filtered
          .iter()
          .position(|index| {
            *index == current
          })
          .unwrap_or(0);
        let next_pos = (current_pos
          + 1)
          % filtered.len();
        filtered[next_pos]
      }
      | None => filtered[0]
    };

    self.select_song(next);
  }

  fn start_playback(&mut self) {
    let Some(prepared) =
      self.prepared_song.as_ref()
    else {
      self.push_activity(
        "Select a song before \
         starting playback."
          .to_string()
      );
      return;
    };

    if prepared.events.is_empty() {
      self.push_activity(
        "Selected song has no notes \
         to play."
          .to_string()
      );
      return;
    }

    self.held_notes.clear();
    self.flashed_notes.clear();
    self.last_timer_score = None;

    let mut state = PlaybackState::new(
      self.play_mode,
      prepared
    );

    if state.mode == PlayMode::Tutorial
    {
      state.cursor_seconds = prepared
        .events
        .first()
        .map_or(0.0, |event| {
          event.at_seconds
        });
    }

    self.playback = Some(state);
    self.push_activity(format!(
      "Playback started in {} mode.",
      self.play_mode
    ));

    info!(mode = %self.play_mode, "playback started");
  }

  fn stop_playback(&mut self) {
    if self.playback.is_some() {
      self.playback = None;
      self.push_activity(
        "Playback stopped.".to_string()
      );
      info!("playback stopped");
    }
  }

  fn handle_tick(
    &mut self,
    now: Instant
  ) {
    self.prune_flashes(now);

    let Some(mut playback) =
      self.playback.take()
    else {
      return;
    };

    let Some(prepared) =
      self.prepared_song.clone()
    else {
      return;
    };

    let mut keep_running = true;

    match playback.mode {
      | PlayMode::Timer => {
        let elapsed = now
          .duration_since(
            playback.started_at
          )
          .as_secs_f32();
        playback.cursor_seconds =
          elapsed;

        while elapsed
          >= playback
            .next_metronome_beat_s
        {
          let accent = playback
            .next_metronome_index
            % self
              .selected_beats_per_bar()
              as u64
            == 0;
          self
            .audio
            .play_metronome_tick(
              accent
            );
          playback
            .next_metronome_index += 1;
          playback
            .next_metronome_beat_s +=
            prepared.beat_seconds;
        }

        if elapsed
          > prepared.duration_seconds
            + 1.2
        {
          playback.score.missed_notes =
            playback
              .score
              .expected_notes
              .saturating_sub(
                playback
                  .score
                  .hit_notes
              );

          self.last_timer_score = Some(
            playback.score.clone()
          );

          self.push_activity(format!(
            "Timer complete: {:.1}% \
             accuracy (perfect {} \
             good {} wrong {} missed \
             {}).",
            playback
              .score
              .accuracy_percent(),
            playback.score.perfect_hits,
            playback.score.good_hits,
            playback.score.wrong_notes,
            playback.score.missed_notes,
          ));

          keep_running = false;
          info!(
            accuracy = playback
              .score
              .accuracy_percent(),
            "timer mode finished"
          );
        }
      }
      | PlayMode::Autoplay => {
        let elapsed = now
          .duration_since(
            playback.started_at
          )
          .as_secs_f32();
        playback.cursor_seconds =
          elapsed;

        while let Some(event) = prepared
          .events
          .get(
            playback.next_event_index
          )
          .cloned()
        {
          if event.at_seconds > elapsed
          {
            break;
          }

          self.trigger_event(&event);
          playback.next_event_index +=
            1;
        }

        if elapsed
          > prepared.duration_seconds
            + 0.8
        {
          self.push_activity(
            "Auto Play complete."
              .to_string()
          );
          keep_running = false;
          info!("autoplay finished");
        }
      }
      | PlayMode::Tutorial => {
        if let Some(event) = prepared
          .events
          .get(
            playback
              .tutorial_event_index
          )
          .cloned()
        {
          playback.cursor_seconds =
            event.at_seconds;
        } else {
          playback.cursor_seconds =
            prepared.duration_seconds;
          self.push_activity(
            "Tutorial complete."
              .to_string()
          );
          keep_running = false;
          info!("tutorial finished");
        }
      }
    }

    if keep_running {
      self.playback = Some(playback);
    }
  }

  fn process_note_input(
    &mut self,
    midi_note: u8
  ) -> bool {
    let mut play_out_loud = true;

    let Some(mut playback) =
      self.playback.take()
    else {
      return play_out_loud;
    };

    let Some(prepared) =
      self.prepared_song.clone()
    else {
      self.playback = Some(playback);
      return play_out_loud;
    };

    let mut keep_running = true;

    match playback.mode {
      | PlayMode::Timer => {
        let now = Instant::now();
        let cursor = now
          .duration_since(
            playback.started_at
          )
          .as_secs_f32();
        playback.cursor_seconds =
          cursor;

        let mut best_match: Option<(
          usize,
          f32
        )> = None;

        for (index, expected) in
          prepared
            .expected_notes
            .iter()
            .enumerate()
        {
          if expected.midi_note
            != midi_note
          {
            continue;
          }
          if playback
            .matched_note_indices
            .contains(&index)
          {
            continue;
          }

          let delta = (expected
            .at_seconds
            - cursor)
            .abs();
          if delta
            > TIMER_WINDOW_SECONDS
          {
            continue;
          }

          match best_match {
            | Some((_, best_delta))
              if delta
                >= best_delta => {}
            | _ => {
              best_match =
                Some((index, delta));
            }
          }
        }

        if let Some((index, delta)) =
          best_match
        {
          playback
            .matched_note_indices
            .insert(index);
          playback.score.hit_notes += 1;

          if delta
            <= TIMER_PERFECT_SECONDS
          {
            playback
              .score
              .perfect_hits += 1;
          } else {
            playback.score.good_hits +=
              1;
          }

          debug!(
            midi_note,
            delta, "timer note matched"
          );
        } else {
          playback.score.wrong_notes +=
            1;
          debug!(
            midi_note,
            "timer note missed"
          );
        }
      }
      | PlayMode::Tutorial => {
        if let Some(event) = prepared
          .events
          .get(
            playback
              .tutorial_event_index
          )
          .cloned()
        {
          let correct = event
            .notes
            .contains(&midi_note);

          if correct {
            playback
              .tutorial_matched
              .insert(midi_note);

            let expected_unique = event
              .notes
              .iter()
              .copied()
              .collect::<HashSet<_>>()
              .len();

            if playback
              .tutorial_matched
              .len()
              >= expected_unique
            {
              playback
                .tutorial_event_index += 1;
              playback
                .tutorial_matched
                .clear();
            }
          } else {
            play_out_loud = self
              .tutorial_options
              .play_bad_notes_out_loud;

            if self
              .tutorial_options
              .only_advance_on_correct_note
            {
              self.push_activity(format!(
                "Tutorial expects: {}",
                event
                  .notes
                  .iter()
                  .map(|note| self
                    .primary_binding_label(
                      *note
                    ))
                  .collect::<Vec<_>>()
                  .join(" ")
              ));
            } else {
              playback
                .tutorial_event_index += 1;
              playback
                .tutorial_matched
                .clear();
            }
          }

          if playback
            .tutorial_event_index
            >= prepared.events.len()
          {
            keep_running = false;
            self.push_activity(
              "Tutorial complete."
                .to_string()
            );
          }
        }
      }
      | PlayMode::Autoplay => {
        // Manual notes are allowed
        // while autoplay runs.
      }
    }

    if keep_running {
      self.playback = Some(playback);
    }

    play_out_loud
  }

  fn trigger_event(
    &mut self,
    event: &PreparedEvent
  ) {
    for midi_note in &event.notes {
      self.audio
        .play_note_with_velocity_duration(
          *midi_note,
          event.velocity,
          event.duration_ms
        );
      self.flash_note(*midi_note);
    }
  }

  fn selected_beats_per_bar(
    &self
  ) -> u8 {
    self
      .selected_song
      .and_then(|index| {
        self.songs.get(index)
      })
      .map(|song| {
        song.song.meta.beats_per_bar
      })
      .filter(|beats| *beats > 0)
      .unwrap_or(4)
  }
}

fn configured_config_path() -> PathBuf {
  env::var("SYMFOSE_CONFIG")
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
      "symfose"
    );
  let (file_writer, guard) =
    tracing_appender::non_blocking(
      file_appender
    );

  let console_layer =
    tracing_fmt::layer()
      .with_target(true)
      .with_file(true)
      .with_line_number(true)
      .with_thread_ids(true)
      .with_thread_names(true);

  let file_layer = tracing_fmt::layer()
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
      &config.effective_keybindings()
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

fn apply_song_ergonomic_bindings(
  bindings: &mut RuntimeBindings,
  song: &SongFile,
  layout: KeyboardLayout
) {
  let mut note_scores =
    HashMap::<u8, usize>::new();
  let mut cooccur =
    HashMap::<(u8, u8), usize>::new();

  for event in &song.events {
    let mut notes = event.notes.clone();
    notes.sort_unstable();
    notes.dedup();
    if notes.is_empty() {
      continue;
    }

    let chord_bonus =
      notes.len().saturating_sub(1) * 3;
    for note in &notes {
      *note_scores
        .entry(*note)
        .or_default() +=
        1 + chord_bonus;
    }

    for left in 0..notes.len() {
      for right in
        (left + 1)..notes.len()
      {
        *cooccur
          .entry((
            notes[left],
            notes[right]
          ))
          .or_default() += 1;
      }
    }
  }

  if note_scores.is_empty() {
    return;
  }

  let mut ranked_notes = note_scores
    .into_iter()
    .collect::<Vec<_>>();
  ranked_notes.sort_by(
    |left, right| {
      right
        .1
        .cmp(&left.1)
        .then(left.0.cmp(&right.0))
    }
  );

  let mut left_pool =
    ergonomic_left_keys(layout);
  let mut right_pool =
    ergonomic_right_keys(layout);
  if left_pool.is_empty()
    && right_pool.is_empty()
  {
    return;
  }

  let song_notes = ranked_notes
    .iter()
    .map(|(note, _)| *note)
    .collect::<HashSet<_>>();
  let mut next_map = bindings
    .note_bindings
    .iter()
    .filter_map(|(chord, note)| {
      (!song_notes.contains(note))
        .then_some((
          chord.clone(),
          *note
        ))
    })
    .collect::<HashMap<_, _>>();

  let mut keys_in_use = next_map
    .keys()
    .map(|chord| chord.to_string())
    .collect::<HashSet<_>>();
  left_pool.retain(|key| {
    !keys_in_use.contains(key)
  });
  right_pool.retain(|key| {
    !keys_in_use.contains(key)
  });

  let mut assigned_side =
    HashMap::<u8, bool>::new();
  let median_note = ranked_notes
    .iter()
    .map(|(note, _)| *note as f32)
    .sum::<f32>()
    / ranked_notes.len() as f32;

  for (note, score) in &ranked_notes {
    let mut same_left_penalty = 0usize;
    let mut same_right_penalty = 0usize;
    for (other, on_left) in
      &assigned_side
    {
      let pair = if note < other {
        (*note, *other)
      } else {
        (*other, *note)
      };
      let weight = cooccur
        .get(&pair)
        .copied()
        .unwrap_or(0);
      if *on_left {
        same_left_penalty += weight;
      } else {
        same_right_penalty += weight;
      }
    }

    let prefer_left_by_pitch =
      (*note as f32) <= median_note;
    let choose_left =
      if same_left_penalty
        != same_right_penalty
      {
        same_left_penalty
          < same_right_penalty
      } else {
        let left_open = left_pool.len();
        let right_open =
          right_pool.len();
        if left_open != right_open {
          left_open > right_open
        } else {
          prefer_left_by_pitch
        }
      };

    let key = if choose_left {
      take_first_available(
        &mut left_pool
      )
      .or_else(|| {
        take_first_available(
          &mut right_pool
        )
      })
    } else {
      take_first_available(
        &mut right_pool
      )
      .or_else(|| {
        take_first_available(
          &mut left_pool
        )
      })
    };

    let Some(key) = key else {
      continue;
    };

    let Ok(chord) =
      crate::input::parse_chord(&key)
    else {
      continue;
    };
    keys_in_use.insert(key);
    next_map.insert(chord, *note);
    assigned_side
      .insert(*note, choose_left);

    trace!(
      midi_note = note,
      score,
      choose_left,
      "song ergonomic binding assigned"
    );
  }

  let mut note_to_chords =
    BTreeMap::<u8, Vec<String>>::new();
  for (chord, note) in &next_map {
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

  bindings.note_bindings = next_map;
  bindings.note_to_chords =
    note_to_chords;

  info!(
    song_notes = song_notes.len(),
    mapped_notes =
      bindings.note_to_chords.len(),
    "applied ergonomic bindings for \
     selected song"
  );
}

fn ergonomic_left_keys(
  layout: KeyboardLayout
) -> Vec<String> {
  match layout {
    | KeyboardLayout::Ansi104 => {
      vec![
        "f", "d", "s", "a", "g", "r",
        "e", "w", "q", "v", "c", "x",
        "z", "t", "b", "5", "4", "3",
        "2", "1", "`",
      ]
      .into_iter()
      .map(str::to_string)
      .collect()
    }
  }
}

fn ergonomic_right_keys(
  layout: KeyboardLayout
) -> Vec<String> {
  match layout {
    | KeyboardLayout::Ansi104 => {
      vec![
        "j", "k", "l", ";", "h", "u",
        "i", "o", "p", "n", "m", ",",
        ".", "/", "'", "6", "7", "8",
        "9", "0", "-", "=", "[", "]",
        "\\",
      ]
      .into_iter()
      .map(str::to_string)
      .collect()
    }
  }
}

fn take_first_available(
  pool: &mut Vec<String>
) -> Option<String> {
  if pool.is_empty() {
    None
  } else {
    Some(pool.remove(0))
  }
}

fn prepare_song_for_bindings(
  source_song: &SongFile,
  bindings: &RuntimeBindings,
  transpose_to_fit: bool
) -> (Option<PreparedSong>, i8, Vec<u8>)
{
  let available_notes = bindings
    .note_to_chords
    .keys()
    .copied()
    .collect::<HashSet<_>>();

  let transpose = if transpose_to_fit {
    choose_transpose_for_fit(
      source_song,
      &available_notes
    )
  } else {
    0
  };

  let adapted_song = if transpose != 0 {
    transpose_song_by_semitones(
      source_song,
      transpose
    )
  } else {
    source_song.clone()
  };

  let prepared =
    prepare_song(&adapted_song);
  let mut missing = prepared
    .expected_notes
    .iter()
    .map(|entry| entry.midi_note)
    .filter(|note| {
      !available_notes.contains(note)
    })
    .collect::<Vec<_>>();
  missing.sort_unstable();
  missing.dedup();

  (Some(prepared), transpose, missing)
}

fn choose_transpose_for_fit(
  song: &SongFile,
  available_notes: &HashSet<u8>
) -> i8 {
  let unique_notes = song
    .events
    .iter()
    .flat_map(|event| {
      event.notes.iter()
    })
    .copied()
    .collect::<HashSet<_>>();

  if unique_notes.is_empty() {
    return 0;
  }

  let shifts = [
    -48, -36, -24, -12, 0, 12, 24, 36,
    48
  ];

  let mut best_shift = 0i8;
  let mut best_score = 0usize;

  for shift in shifts {
    let mut score = 0usize;
    for note in &unique_notes {
      let shifted =
        i16::from(*note) + shift;
      if !(0..=127).contains(&shifted) {
        continue;
      }
      if available_notes
        .contains(&(shifted as u8))
      {
        score += 1;
      }
    }

    let shift_abs = shift.abs() as i16;
    let best_abs =
      i16::from(best_shift).abs();
    let is_better = score > best_score
      || (score == best_score
        && shift_abs < best_abs);
    if is_better {
      best_score = score;
      best_shift = shift as i8;
    }
  }

  best_shift
}

fn transpose_song_by_semitones(
  source: &SongFile,
  semitones: i8
) -> SongFile {
  let mut cloned = source.clone();
  let delta = i16::from(semitones);

  for event in &mut cloned.events {
    for note in &mut event.notes {
      let shifted =
        i16::from(*note) + delta;
      if (0..=127).contains(&shifted) {
        *note = shifted as u8;
      }
    }
  }

  cloned
}

fn prepare_song(
  song: &SongFile
) -> PreparedSong {
  let beat_seconds =
    60.0 / song.meta.tempo_bpm.max(1.0);

  let mut expected_notes = Vec::new();
  let mut prepared_events = Vec::new();

  let mut duration_seconds: f32 = 0.0;

  for event in &song.events {
    if event.notes.is_empty() {
      continue;
    }

    let at_seconds =
      event.at_beats.max(0.0)
        * beat_seconds;
    let duration_seconds_for_event =
      if event.duration_beats > 0.0 {
        (event.duration_beats
          * beat_seconds)
          .max(0.04)
      } else {
        0.32
      };

    let duration_ms =
      (duration_seconds_for_event
        * 1000.0)
        .round()
        .max(45.0) as u64;

    let velocity = event
      .velocity
      .unwrap_or(
        song.meta.default_velocity
      )
      .clamp(1, 127);

    for midi_note in &event.notes {
      expected_notes.push(
        ExpectedNote {
          at_seconds,
          midi_note: *midi_note
        }
      );
    }

    duration_seconds = duration_seconds
      .max(
        at_seconds
          + duration_seconds_for_event
      );

    prepared_events.push(
      PreparedEvent {
        at_seconds,
        duration_seconds:
          duration_seconds_for_event,
        duration_ms,
        velocity,
        notes: event.notes.clone()
      }
    );
  }

  PreparedSong {
    events: prepared_events,
    expected_notes,
    duration_seconds,
    beat_seconds
  }
}

fn is_white_key(midi_note: u8) -> bool {
  !is_black_key(midi_note)
}

fn is_black_key(midi_note: u8) -> bool {
  matches!(
    midi_note % 12,
    1 | 3 | 6 | 8 | 10
  )
}

fn black_key_after(
  white_note: u8
) -> Option<u8> {
  match white_note % 12 {
    | 0 | 2 | 5 | 7 | 9 => {
      Some(white_note + 1)
    }
    | _ => None
  }
}

fn white_key_style(
  active: bool,
  guided: bool
) -> container::Style {
  let mut style =
    container::Style::default()
      .background(
        if active {
          Color::from_rgb8(255, 180, 95)
        } else if guided {
          Color::from_rgb8(
            255, 242, 204
          )
        } else {
          Color::from_rgb8(
            245, 245, 245
          )
        }
      )
      .color(Color::from_rgb8(
        25, 25, 25
      ));

  style.border =
    border::rounded(0).width(1).color(
      Color::from_rgb8(140, 140, 140)
    );

  style
}

fn tag_chip_button_style(
  _theme: &Theme,
  status: button::Status
) -> button::Style {
  let base = button::Style {
    background: Some(
      Color::from_rgba(
        0.12, 0.62, 0.20, 0.12
      )
      .into()
    ),
    border: border::rounded(12)
      .width(1)
      .color(Color::from_rgba(
        0.12, 0.62, 0.20, 0.55
      )),
    text_color: Color::from_rgb(
      0.10, 0.62, 0.18
    ),
    ..Default::default()
  };

  match status {
    | button::Status::Hovered => {
      button::Style {
        background: Some(
          Color::from_rgba(
            0.12, 0.62, 0.20, 0.22
          )
          .into()
        ),
        ..base
      }
    }
    | button::Status::Pressed => {
      button::Style {
        background: Some(
          Color::from_rgba(
            0.12, 0.62, 0.20, 0.34
          )
          .into()
        ),
        ..base
      }
    }
    | button::Status::Disabled => {
      button::Style {
        text_color: Color::from_rgba(
          0.10, 0.62, 0.18, 0.5
        ),
        ..base
      }
    }
    | _ => base
  }
}

fn black_key_style(
  active: bool,
  guided: bool
) -> container::Style {
  let mut style =
    container::Style::default()
      .background(
        if active {
          Color::from_rgb8(255, 136, 70)
        } else if guided {
          Color::from_rgb8(84, 84, 84)
        } else {
          Color::from_rgb8(26, 26, 26)
        }
      )
      .color(Color::from_rgb8(
        242, 242, 242
      ));

  style.border =
    border::rounded(0).width(1).color(
      Color::from_rgb8(16, 16, 16)
    );

  style
}

fn timeline_tile_style(
  is_current: bool,
  is_past: bool
) -> container::Style {
  let mut style =
    container::Style::default().color(
      Color::from_rgb8(20, 20, 20)
    );

  style.background = Some(
    if is_current {
      Color::from_rgb8(255, 212, 138)
    } else if is_past {
      Color::from_rgb8(220, 220, 220)
    } else {
      Color::from_rgb8(244, 244, 244)
    }
    .into()
  );
  style.border =
    border::rounded(6).width(1).color(
      Color::from_rgb8(160, 160, 160)
    );

  style
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
