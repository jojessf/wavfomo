//! Hotkey actions and binding parsing.
//!
//! Bindings are configured as `"Modifier+Key"` strings (e.g. `"Shift+Left"`)
//! and parsed into `(KeyCode, KeyModifiers)` lookup entries. Incoming key events
//! are normalized the same way so config bindings match reliably.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyModifiers};

/// Everything the user can bind a key to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    PlayPause,
    Stop,
    SeekBack,
    SeekForward,
    SeekBackLarge,
    SeekForwardLarge,
    SeekBackFine,
    SeekForwardFine,
    WaveformZoomIn,
    WaveformZoomOut,
    VscaleIncrease,
    VscaleDecrease,
    VolumeUp,
    VolumeDown,
    Mute,
    Quit,
}

pub type Keymap = HashMap<(KeyCode, KeyModifiers), Action>;

/// Keep only the modifier bits we bind on.
fn relevant_mods(mods: KeyModifiers) -> KeyModifiers {
    mods & (KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL)
}

/// Normalize a key so bindings and events compare equal. For character keys the
/// shift state is already encoded in the character, so we lowercase it and drop
/// the SHIFT bit; other keys keep their modifiers.
pub fn normalize(code: KeyCode, mods: KeyModifiers) -> (KeyCode, KeyModifiers) {
    let mods = relevant_mods(mods);
    match code {
        KeyCode::Char(c) => (
            KeyCode::Char(c.to_ascii_lowercase()),
            mods - KeyModifiers::SHIFT,
        ),
        other => (other, mods),
    }
}

/// Parse a binding string like `"Shift+Left"` or `"s"` into a normalized entry.
pub fn parse_binding(s: &str) -> Result<(KeyCode, KeyModifiers), String> {
    let mut mods = KeyModifiers::empty();
    let mut key: Option<KeyCode> = None;

    for part in s.split('+') {
        let part = part.trim();
        match part.to_ascii_lowercase().as_str() {
            "shift" => mods |= KeyModifiers::SHIFT,
            "alt" => mods |= KeyModifiers::ALT,
            "ctrl" | "control" => mods |= KeyModifiers::CONTROL,
            _ => {
                if key.is_some() {
                    return Err(format!("binding '{s}' has more than one key"));
                }
                key = Some(parse_key(part)?);
            }
        }
    }

    let code = key.ok_or_else(|| format!("binding '{s}' has no key"))?;
    Ok(normalize(code, mods))
}

fn parse_key(k: &str) -> Result<KeyCode, String> {
    let code = match k.to_ascii_lowercase().as_str() {
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "space" => KeyCode::Char(' '),
        "esc" | "escape" => KeyCode::Esc,
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        other => {
            let mut chars = other.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => KeyCode::Char(c),
                _ => return Err(format!("unknown key '{k}'")),
            }
        }
    };
    Ok(code)
}

/// Build a lookup map from `(binding string, action)` pairs, reporting the first
/// invalid binding.
pub fn build_keymap(bindings: &[(String, Action)]) -> Result<Keymap, String> {
    let mut map = Keymap::new();
    for (binding, action) in bindings {
        let entry = parse_binding(binding)?;
        map.insert(entry, *action);
    }
    Ok(map)
}
