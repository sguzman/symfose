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
use iced::keyboard::key::Named;
use iced::keyboard::{
  self,
  Key
};

#[derive(
  Debug,
  Clone,
  Copy,
  Default,
  PartialEq,
  Eq,
  Hash,
)]
pub struct KeyModifiers {
  pub ctrl:  bool,
  pub alt:   bool,
  pub logo:  bool,
  pub shift: bool
}

impl KeyModifiers {
  pub fn from_iced(
    modifiers: keyboard::Modifiers
  ) -> Self {
    Self {
      ctrl:  modifiers.control(),
      alt:   modifiers.alt(),
      logo:  modifiers.logo(),
      shift: modifiers.shift()
    }
  }
}

#[derive(
  Debug, Clone, PartialEq, Eq, Hash,
)]
pub struct KeyChord {
  pub key:       String,
  pub modifiers: KeyModifiers
}

impl KeyChord {
  pub fn from_key_event(
    key: &Key,
    modifiers: keyboard::Modifiers,
    ignore_shift_for_char_keys: bool
  ) -> Option<Self> {
    let (token, is_char_key) =
      key_to_token_from_event(key)?;

    let mut normalized_mods =
      KeyModifiers::from_iced(
        modifiers
      );
    if ignore_shift_for_char_keys
      && is_char_key
    {
      normalized_mods.shift = false;
    }

    Some(Self {
      key:       token,
      modifiers: normalized_mods
    })
  }
}

impl Display for KeyChord {
  fn fmt(
    &self,
    f: &mut Formatter<'_>
  ) -> fmt::Result {
    let mut parts = Vec::new();

    if self.modifiers.ctrl {
      parts.push("ctrl".to_string());
    }
    if self.modifiers.alt {
      parts.push("alt".to_string());
    }
    if self.modifiers.logo {
      parts.push("super".to_string());
    }
    if self.modifiers.shift {
      parts.push("shift".to_string());
    }

    parts.push(self.key.clone());

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
      compiled.insert(
        chord.clone(),
        *midi_note
      )
    {
      bail!(
        "duplicate keybinding {chord} \
         (MIDI {existing_note} and \
         MIDI {midi_note})"
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
    KeyModifiers::default();
  let mut key_token: Option<String> =
    None;

  for token in spec
    .split('+')
    .map(str::trim)
    .filter(|token| !token.is_empty())
  {
    let lowered =
      token.to_ascii_lowercase();

    match lowered.as_str() {
      | "ctrl" | "control" => {
        modifiers.ctrl = true
      }
      | "alt" | "option" => {
        modifiers.alt = true
      }
      | "super" | "meta" | "cmd"
      | "command" | "win"
      | "windows" => {
        modifiers.logo = true
      }
      | "shift" => {
        modifiers.shift = true
      }
      | _ => {
        if key_token.is_some() {
          bail!(
            "multiple non-modifier \
             keys in chord '{spec}'"
          );
        }

        key_token = Some(
          parse_key_token(&lowered)?
        );
      }
    }
  }

  let key =
    key_token.with_context(|| {
      format!(
        "missing key in chord '{spec}'"
      )
    })?;

  Ok(KeyChord {
    key,
    modifiers
  })
}

fn parse_key_token(
  token: &str
) -> Result<String> {
  let normalized = match token {
    | "esc" | "escape" => {
      "esc".to_string()
    }
    | "enter" | "return" => {
      "enter".to_string()
    }
    | "tab" => "tab".to_string(),
    | "backtab" => {
      "backtab".to_string()
    }
    | "backspace" => {
      "backspace".to_string()
    }
    | "space" => "space".to_string(),
    | "left" => "left".to_string(),
    | "right" => "right".to_string(),
    | "up" => "up".to_string(),
    | "down" => "down".to_string(),
    | "home" => "home".to_string(),
    | "end" => "end".to_string(),
    | "pageup" => "pageup".to_string(),
    | "pagedown" => {
      "pagedown".to_string()
    }
    | "delete" | "del" => {
      "delete".to_string()
    }
    | "insert" | "ins" => {
      "insert".to_string()
    }
    | "comma" => ",".to_string(),
    | "period" | "dot" => {
      ".".to_string()
    }
    | "slash" => "/".to_string(),
    | "semicolon" => ";".to_string(),
    | "apostrophe" | "quote" => {
      "'".to_string()
    }
    | "minus" => "-".to_string(),
    | "equals" => "=".to_string(),
    | "left_bracket" | "lbracket" => {
      "[".to_string()
    }
    | "right_bracket" | "rbracket" => {
      "]".to_string()
    }
    | "backslash" => "\\".to_string(),
    | "grave" | "backtick" => {
      "`".to_string()
    }
    | "plus" => "+".to_string(),
    | _ => {
      if let Some(f_key) =
        parse_function_key_token(token)?
      {
        f_key
      } else {
        let mut chars = token.chars();
        match (
          chars.next(),
          chars.next()
        ) {
          | (Some(ch), None) => {
            ch.to_ascii_lowercase()
              .to_string()
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

  Ok(normalized)
}

fn parse_function_key_token(
  token: &str
) -> Result<Option<String>> {
  if !(token.starts_with('f')
    && token.len() > 1)
  {
    return Ok(None);
  }

  let n = token[1..]
    .parse::<u8>()
    .with_context(|| {
      format!(
        "invalid function key \
         '{token}'"
      )
    })?;

  if !(1..=24).contains(&n) {
    bail!(
      "function key must be in range \
       f1..f24: '{token}'"
    );
  }

  Ok(Some(format!("f{n}")))
}

fn key_to_token_from_event(
  key: &Key
) -> Option<(String, bool)> {
  match key.as_ref() {
    | Key::Character(text) => {
      let mut chars = text.chars();
      let first = chars.next()?;

      if chars.next().is_some() {
        return Some((
          text.to_ascii_lowercase(),
          false
        ));
      }

      if first == ' ' {
        return Some((
          "space".to_string(),
          false
        ));
      }

      Some((
        first
          .to_ascii_lowercase()
          .to_string(),
        true
      ))
    }
    | Key::Named(named) => {
      named_key_to_token(named)
        .map(|token| (token, false))
    }
    | Key::Unidentified => None
  }
}

fn named_key_to_token(
  named: Named
) -> Option<String> {
  match named {
    | Named::Escape => {
      Some("esc".to_string())
    }
    | Named::Enter => {
      Some("enter".to_string())
    }
    | Named::Tab => {
      Some("tab".to_string())
    }
    | Named::Backspace => {
      Some("backspace".to_string())
    }
    | Named::Space => {
      Some("space".to_string())
    }
    | Named::ArrowLeft => {
      Some("left".to_string())
    }
    | Named::ArrowRight => {
      Some("right".to_string())
    }
    | Named::ArrowUp => {
      Some("up".to_string())
    }
    | Named::ArrowDown => {
      Some("down".to_string())
    }
    | Named::Home => {
      Some("home".to_string())
    }
    | Named::End => {
      Some("end".to_string())
    }
    | Named::PageUp => {
      Some("pageup".to_string())
    }
    | Named::PageDown => {
      Some("pagedown".to_string())
    }
    | Named::Delete => {
      Some("delete".to_string())
    }
    | Named::Insert => {
      Some("insert".to_string())
    }
    | _ => {
      named_function_key(named)
        .map(|n| format!("f{n}"))
    }
  }
}

fn named_function_key(
  named: Named
) -> Option<u8> {
  match named {
    | Named::F1 => Some(1),
    | Named::F2 => Some(2),
    | Named::F3 => Some(3),
    | Named::F4 => Some(4),
    | Named::F5 => Some(5),
    | Named::F6 => Some(6),
    | Named::F7 => Some(7),
    | Named::F8 => Some(8),
    | Named::F9 => Some(9),
    | Named::F10 => Some(10),
    | Named::F11 => Some(11),
    | Named::F12 => Some(12),
    | Named::F13 => Some(13),
    | Named::F14 => Some(14),
    | Named::F15 => Some(15),
    | Named::F16 => Some(16),
    | Named::F17 => Some(17),
    | Named::F18 => Some(18),
    | Named::F19 => Some(19),
    | Named::F20 => Some(20),
    | Named::F21 => Some(21),
    | Named::F22 => Some(22),
    | Named::F23 => Some(23),
    | Named::F24 => Some(24),
    | _ => None
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_function_key() {
    let chord =
      parse_chord("f1").unwrap();
    assert_eq!(chord.key, "f1");
  }

  #[test]
  fn parses_plain_f_as_character() {
    let chord =
      parse_chord("f").unwrap();
    assert_eq!(chord.key, "f");
  }

  #[test]
  fn normalizes_shifted_character_when_configured()
   {
    let key =
      Key::Character("A".into());
    let chord =
      KeyChord::from_key_event(
        &key,
        keyboard::Modifiers::SHIFT,
        true
      )
      .expect("chord expected");

    assert_eq!(chord.key, "a");
    assert!(!chord.modifiers.shift);
  }
}
