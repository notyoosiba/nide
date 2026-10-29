use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Normalizes key events, specifically mapping macOS Option-character overrides
/// and Ctrl-number shortcuts into standard Alt+<digit> events:
/// - '¡' (Option+1 on macOS) -> Alt+1
/// - '™' / '€' (Option+2 on macOS) -> Alt+2
/// - '£' (Option+3 on macOS) -> Alt+3
/// - Ctrl+1 -> Alt+1
/// - Ctrl+2 -> Alt+2
/// - Ctrl+3 -> Alt+3
/// - Ctrl+` -> Alt+`
#[must_use]
pub fn normalize_key(key: KeyEvent) -> KeyEvent {
    match key.code {
        // macOS Option+1 produces '¡'
        KeyCode::Char('¡') => KeyEvent {
            code: KeyCode::Char('1'),
            modifiers: KeyModifiers::ALT,
            ..key
        },
        // macOS Option+2 produces '™' (US) or '€' (European layouts)
        KeyCode::Char('™' | '€') => KeyEvent {
            code: KeyCode::Char('2'),
            modifiers: KeyModifiers::ALT,
            ..key
        },
        // macOS Option+3 produces '£'
        KeyCode::Char('£') => KeyEvent {
            code: KeyCode::Char('3'),
            modifiers: KeyModifiers::ALT,
            ..key
        },
        // Ctrl+1 fallback
        KeyCode::Char('1') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyEvent {
            code: KeyCode::Char('1'),
            modifiers: (key.modifiers - KeyModifiers::CONTROL) | KeyModifiers::ALT,
            ..key
        },
        // Ctrl+2 fallback
        KeyCode::Char('2') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyEvent {
            code: KeyCode::Char('2'),
            modifiers: (key.modifiers - KeyModifiers::CONTROL) | KeyModifiers::ALT,
            ..key
        },
        // Ctrl+3 fallback
        KeyCode::Char('3') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyEvent {
            code: KeyCode::Char('3'),
            modifiers: (key.modifiers - KeyModifiers::CONTROL) | KeyModifiers::ALT,
            ..key
        },
        // Ctrl+` or Ctrl+~ fallback for quake terminal
        KeyCode::Char('`' | '~') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyEvent {
            code: KeyCode::Char('`'),
            modifiers: (key.modifiers - KeyModifiers::CONTROL) | KeyModifiers::ALT,
            ..key
        },
        KeyCode::Char('~') if key.modifiers.contains(KeyModifiers::ALT) => KeyEvent {
            code: KeyCode::Char('`'),
            ..key
        },
        // macOS European / Polish ISO keyboard overrides:
        // - Option+§ produces '¬' (the physical key under Esc / left of 1)
        // - Option+Shift+` produces 'Ŕ' on Polish Pro
        KeyCode::Char('¬' | 'Ŕ') => KeyEvent {
            code: KeyCode::Char('`'),
            modifiers: KeyModifiers::ALT,
            ..key
        },
        // Alt + § on European layouts
        KeyCode::Char('§' | '±') if key.modifiers.contains(KeyModifiers::ALT) => KeyEvent {
            code: KeyCode::Char('`'),
            modifiers: (key.modifiers - KeyModifiers::ALT) | KeyModifiers::ALT,
            ..key
        },
        // Universal F12 Quake console toggle
        KeyCode::F(12) => KeyEvent {
            code: KeyCode::Char('`'),
            modifiers: KeyModifiers::ALT,
            ..key
        },
        _ => key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    #[test]
    fn test_macos_option_char_overrides() {
        // Option+1 on macOS emits '¡'
        let k1 = normalize_key(press(KeyCode::Char('¡'), KeyModifiers::NONE));
        assert_eq!(k1.code, KeyCode::Char('1'));
        assert_eq!(k1.modifiers, KeyModifiers::ALT);

        // Option+2 emits '™'
        let k2_us = normalize_key(press(KeyCode::Char('™'), KeyModifiers::NONE));
        assert_eq!(k2_us.code, KeyCode::Char('2'));
        assert_eq!(k2_us.modifiers, KeyModifiers::ALT);

        // Option+2 emits '€' on European layouts
        let k2_eu = normalize_key(press(KeyCode::Char('€'), KeyModifiers::NONE));
        assert_eq!(k2_eu.code, KeyCode::Char('2'));
        assert_eq!(k2_eu.modifiers, KeyModifiers::ALT);

        // Option+3 emits '£'
        let k3 = normalize_key(press(KeyCode::Char('£'), KeyModifiers::NONE));
        assert_eq!(k3.code, KeyCode::Char('3'));
        assert_eq!(k3.modifiers, KeyModifiers::ALT);
    }

    #[test]
    fn test_ctrl_digit_fallbacks() {
        let c1 = normalize_key(press(KeyCode::Char('1'), KeyModifiers::CONTROL));
        assert_eq!(c1.code, KeyCode::Char('1'));
        assert_eq!(c1.modifiers, KeyModifiers::ALT);

        let c2 = normalize_key(press(KeyCode::Char('2'), KeyModifiers::CONTROL));
        assert_eq!(c2.code, KeyCode::Char('2'));
        assert_eq!(c2.modifiers, KeyModifiers::ALT);

        let c3 = normalize_key(press(KeyCode::Char('3'), KeyModifiers::CONTROL));
        assert_eq!(c3.code, KeyCode::Char('3'));
        assert_eq!(c3.modifiers, KeyModifiers::ALT);
    }

    #[test]
    fn test_other_keys_unmodified() {
        let enter = press(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(normalize_key(enter), enter);

        let tab = press(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(normalize_key(tab), tab);

        let alt_1 = press(KeyCode::Char('1'), KeyModifiers::ALT);
        assert_eq!(normalize_key(alt_1), alt_1);
    }

    #[test]
    fn test_macos_quake_overrides() {
        // Option+§ on Polish/European keyboards produces '¬'
        let k_not = normalize_key(press(KeyCode::Char('¬'), KeyModifiers::NONE));
        assert_eq!(k_not.code, KeyCode::Char('`'));
        assert_eq!(k_not.modifiers, KeyModifiers::ALT);

        // Option+Shift+` on Polish keyboard produces 'Ŕ'
        let k_r = normalize_key(press(KeyCode::Char('Ŕ'), KeyModifiers::NONE));
        assert_eq!(k_r.code, KeyCode::Char('`'));
        assert_eq!(k_r.modifiers, KeyModifiers::ALT);

        // Alt+§ on European layouts
        let k_sec = normalize_key(press(KeyCode::Char('§'), KeyModifiers::ALT));
        assert_eq!(k_sec.code, KeyCode::Char('`'));
        assert_eq!(k_sec.modifiers, KeyModifiers::ALT);

        // F12 fallback
        let f12 = normalize_key(press(KeyCode::F(12), KeyModifiers::NONE));
        assert_eq!(f12.code, KeyCode::Char('`'));
        assert_eq!(f12.modifiers, KeyModifiers::ALT);
    }
}
