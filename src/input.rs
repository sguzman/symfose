use std::collections::{
  BTreeMap,
  HashMap,
  HashSet
};
use std::fmt::{
  self,
  Display,
  Formatter
};

use anyhow::{
  Context,
  Result,
  bail
};
use crossterm::event::{
  KeyCode,
  KeyEvent,
  KeyModifiers
};

#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Hash,
)]
pub struct KeyChord {
  pub code:      KeyCode,
  pub modifiers: KeyModifiers
}

impl KeyChord {
  pub fn from_event(
    event: KeyEvent,
    ignore_shift_for_char_keys: bool
  ) -> Option<Self> {
    if matches!(
      event.code,
      KeyCode::Modifier(_)
    ) {
      return None;
    }

    let mut code = event.code;
    let mut modifiers = event.modifiers;

    if let KeyCode::Char(character) =
      code
    {
      let normalized =
        character.to_ascii_lowercase();
      code = KeyCode::Char(normalized);

      if ignore_shift_for_char_keys {
        modifiers
          .remove(KeyModifiers::SHIFT);
      }
    }

    let allowed_modifiers =
      KeyModifiers::SHIFT
        | KeyModifiers::CONTROL
        | KeyModifiers::ALT
        | KeyModifiers::SUPER;

    Some(Self {
      code,
      modifiers: modifiers
        & allowed_modifiers
    })
  }
}

impl Display for KeyChord {
  fn fmt(
    &self,
    f: &mut Formatter<'_>
  ) -> fmt::Result {
    let mut parts = Vec::new();

    if self
      .modifiers
      .contains(KeyModifiers::CONTROL)
    {
      parts.push("ctrl".to_string());
    }
    if self
      .modifiers
      .contains(KeyModifiers::ALT)
    {
      parts.push("alt".to_string());
    }
    if self
      .modifiers
      .contains(KeyModifiers::SUPER)
    {
      parts.push("super".to_string());
    }
    if self
      .modifiers
      .contains(KeyModifiers::SHIFT)
    {
      parts.push("shift".to_string());
    }

    parts
      .push(key_code_label(self.code));
    write!(f, "{}", parts.join("+"))
  }
}

pub fn compile_note_bindings(
  raw_bindings: &BTreeMap<String, u8>
) -> Result<HashMap<KeyChord, u8>> {
  let mut compiled = HashMap::new();

  for (chord_spec, midi_note) in
    raw_bindings
  {
    if *midi_note > 127 {
      bail!(
        "{chord_spec} maps to invalid \
         MIDI note {midi_note}"
      );
    }

    let chord = parse_chord(chord_spec)
      .with_context(|| {
        format!(
          "invalid keybinding \
           {chord_spec}"
        )
      })?;
    if let Some(existing_note) =
      compiled.insert(chord, *midi_note)
    {
      bail!(
        "duplicate keybinding {chord} \
         (MIDI {existing_note} and \
         MIDI {midi_note})",
      );
    }
  }

  Ok(compiled)
}

pub fn compile_chord_set(
  entries: &[String],
  label: &str
) -> Result<HashSet<KeyChord>> {
  let mut set = HashSet::new();

  for entry in entries {
    let chord = parse_chord(entry)
      .with_context(|| {
        format!(
          "invalid {label} control \
           binding: {entry}"
        )
      })?;
    set.insert(chord);
  }

  Ok(set)
}

pub fn parse_chord(
  spec: &str
) -> Result<KeyChord> {
  let mut modifiers =
    KeyModifiers::empty();
  let mut key_code: Option<KeyCode> =
    None;

  for token in spec
    .split('+')
    .map(str::trim)
    .filter(|token| !token.is_empty())
  {
    let token_lower =
      token.to_ascii_lowercase();

    match token_lower.as_str() {
      | "ctrl" | "control" => {
        modifiers
          .insert(KeyModifiers::CONTROL)
      }
      | "alt" | "option" => {
        modifiers
          .insert(KeyModifiers::ALT)
      }
      | "shift" => {
        modifiers
          .insert(KeyModifiers::SHIFT)
      }
      | "super" | "meta" | "cmd"
      | "command" | "win"
      | "windows" => {
        modifiers
          .insert(KeyModifiers::SUPER)
      }
      | _ => {
        if key_code.is_some() {
          bail!(
            "multiple non-modifier \
             keys in chord '{spec}'"
          );
        }
        key_code =
          Some(parse_key(token)?);
      }
    }
  }

  let code =
    key_code.with_context(|| {
      format!(
        "missing key in chord '{spec}'"
      )
    })?;

  Ok(KeyChord {
    code,
    modifiers
  })
}

fn parse_key(
  token: &str
) -> Result<KeyCode> {
  let token = token.trim();
  let token_lower =
    token.to_ascii_lowercase();

  let key_code = match token_lower
    .as_str()
  {
    | "esc" | "escape" => KeyCode::Esc,
    | "enter" | "return" => {
      KeyCode::Enter
    }
    | "tab" => KeyCode::Tab,
    | "backtab" => KeyCode::BackTab,
    | "backspace" => KeyCode::Backspace,
    | "space" => KeyCode::Char(' '),
    | "left" => KeyCode::Left,
    | "right" => KeyCode::Right,
    | "up" => KeyCode::Up,
    | "down" => KeyCode::Down,
    | "home" => KeyCode::Home,
    | "end" => KeyCode::End,
    | "pageup" => KeyCode::PageUp,
    | "pagedown" => KeyCode::PageDown,
    | "delete" | "del" => {
      KeyCode::Delete
    }
    | "insert" | "ins" => {
      KeyCode::Insert
    }
    | "comma" => KeyCode::Char(','),
    | "period" | "dot" => {
      KeyCode::Char('.')
    }
    | "slash" => KeyCode::Char('/'),
    | "semicolon" => KeyCode::Char(';'),
    | "apostrophe" | "quote" => {
      KeyCode::Char('\'')
    }
    | "minus" => KeyCode::Char('-'),
    | "equals" => KeyCode::Char('='),
    | "left_bracket" | "lbracket" => {
      KeyCode::Char('[')
    }
    | "right_bracket" | "rbracket" => {
      KeyCode::Char(']')
    }
    | "backslash" => {
      KeyCode::Char('\\')
    }
    | "grave" | "backtick" => {
      KeyCode::Char('`')
    }
    | "plus" => KeyCode::Char('+'),
    | _ => {
      if token_lower.starts_with('f')
        && token_lower.len() > 1
      {
        let function_number =
          &token_lower[1..];
        let n = function_number
          .parse::<u8>()
          .with_context(|| {
            format!(
              "invalid function key \
               '{token}'"
            )
          })?;
        if n == 0 {
          bail!(
            "function key numbers \
             start at 1: '{token}'"
          );
        }
        KeyCode::F(n)
      } else {
        let mut chars = token.chars();
        match (
          chars.next(),
          chars.next()
        ) {
          | (Some(character), None) => {
            KeyCode::Char(
              character
                .to_ascii_lowercase()
            )
          }
          | _ => {
            bail!(
              "unknown key token \
               '{token}'"
            )
          }
        }
      }
    }
  };

  Ok(key_code)
}

fn key_code_label(
  key_code: KeyCode
) -> String {
  match key_code {
    | KeyCode::Backspace => {
      "backspace".to_string()
    }
    | KeyCode::Enter => {
      "enter".to_string()
    }
    | KeyCode::Left => {
      "left".to_string()
    }
    | KeyCode::Right => {
      "right".to_string()
    }
    | KeyCode::Up => "up".to_string(),
    | KeyCode::Down => {
      "down".to_string()
    }
    | KeyCode::Home => {
      "home".to_string()
    }
    | KeyCode::End => "end".to_string(),
    | KeyCode::PageUp => {
      "pageup".to_string()
    }
    | KeyCode::PageDown => {
      "pagedown".to_string()
    }
    | KeyCode::Tab => "tab".to_string(),
    | KeyCode::BackTab => {
      "backtab".to_string()
    }
    | KeyCode::Delete => {
      "delete".to_string()
    }
    | KeyCode::Insert => {
      "insert".to_string()
    }
    | KeyCode::F(n) => format!("f{n}"),
    | KeyCode::Char(character) => {
      character.to_string()
    }
    | KeyCode::Null => {
      "null".to_string()
    }
    | KeyCode::Esc => "esc".to_string(),
    | KeyCode::CapsLock => {
      "caps_lock".to_string()
    }
    | KeyCode::ScrollLock => {
      "scroll_lock".to_string()
    }
    | KeyCode::NumLock => {
      "num_lock".to_string()
    }
    | KeyCode::PrintScreen => {
      "print_screen".to_string()
    }
    | KeyCode::Pause => {
      "pause".to_string()
    }
    | KeyCode::Menu => {
      "menu".to_string()
    }
    | KeyCode::KeypadBegin => {
      "keypad_begin".to_string()
    }
    | KeyCode::Media(media) => {
      format!("media:{media:?}")
    }
    | KeyCode::Modifier(modifier) => {
      format!("modifier:{modifier:?}")
    }
  }
}

#[cfg(test)]
mod tests {
  use crossterm::event::KeyEvent;

  use super::*;

  #[test]
  fn parses_function_key() {
    let chord =
      parse_chord("f1").unwrap();
    assert_eq!(
      chord.code,
      KeyCode::F(1)
    );
  }

  #[test]
  fn parses_plain_f_as_character() {
    let chord =
      parse_chord("f").unwrap();
    assert_eq!(
      chord.code,
      KeyCode::Char('f')
    );
  }

  #[test]
  fn normalizes_shifted_char_when_requested()
   {
    let event = KeyEvent::new(
      KeyCode::Char('A'),
      KeyModifiers::SHIFT
    );
    let chord =
      KeyChord::from_event(event, true)
        .unwrap();

    assert_eq!(
      chord.code,
      KeyCode::Char('a')
    );
    assert_eq!(
      chord.modifiers,
      KeyModifiers::empty()
    );
  }
}
