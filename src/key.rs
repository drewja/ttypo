// Cross-target key event types.
//
// On native we re-export crossterm's types directly so the shared
// `test::handle_key`, `keyboard::note_event`, and `title::handle_key`
// signatures and pattern matches keep working unchanged.
//
// On wasm we mirror the same shape with our own minimal types, because
// crossterm 0.29's terminal/cursor modules don't compile on
// wasm32-unknown-unknown. The wasm web entry converts ratzilla's
// `KeyEvent` into one of these in src/web.rs.

#[cfg(not(target_arch = "wasm32"))]
pub use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[cfg(target_arch = "wasm32")]
pub use wasm::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[cfg(target_arch = "wasm32")]
mod wasm {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct KeyModifiers(u8);

    impl KeyModifiers {
        pub const NONE: Self = Self(0);
        pub const SHIFT: Self = Self(1 << 0);
        pub const CONTROL: Self = Self(1 << 1);
        pub const ALT: Self = Self(1 << 2);

        pub fn empty() -> Self {
            Self(0)
        }

        pub fn contains(self, other: Self) -> bool {
            (self.0 & other.0) == other.0
        }
    }

    impl std::ops::BitOr for KeyModifiers {
        type Output = Self;
        fn bitor(self, rhs: Self) -> Self {
            Self(self.0 | rhs.0)
        }
    }

    impl std::ops::BitOrAssign for KeyModifiers {
        fn bitor_assign(&mut self, rhs: Self) {
            self.0 |= rhs.0;
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum KeyCode {
        Char(char),
        Backspace,
        Enter,
        Left,
        Right,
        Up,
        Down,
        Home,
        End,
        PageUp,
        PageDown,
        Tab,
        BackTab,
        Delete,
        Insert,
        F(u8),
        Esc,
        Null,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum KeyEventKind {
        Press,
        Repeat,
        Release,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct KeyEvent {
        pub code: KeyCode,
        pub modifiers: KeyModifiers,
        pub kind: KeyEventKind,
    }
}
