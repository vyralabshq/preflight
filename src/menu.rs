//! A one question arrow key menu, without pulling in a terminal library.
//!
//! Reading arrow keys means turning off line buffering and echo, which is a
//! termios call rather than anything exotic. The terminal is always restored,
//! including when the read fails partway.

use std::io::{Read, Write};

/// The terminal's original settings, put back when this is dropped.
struct RawMode(libc::termios);

impl RawMode {
    fn enter() -> Option<RawMode> {
        let mut original: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut original) } != 0 {
            return None;
        }
        let mut raw = original;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        match unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) } {
            0 => Some(RawMode(original)),
            _ => None,
        }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.0) };
    }
}

#[derive(Debug, PartialEq)]
pub enum Key {
    Up,
    Down,
    Enter,
    Digit(usize),
    Quit,
    Other,
}

fn read_key() -> Key {
    let mut b = [0u8; 1];
    if std::io::stdin().read_exact(&mut b).is_err() {
        return Key::Quit;
    }
    match b[0] {
        b'\r' | b'\n' => Key::Enter,
        b'q' | 3 | 4 => Key::Quit,
        c @ b'1'..=b'9' => Key::Digit((c - b'0') as usize),
        0x1b => {
            let mut rest = [0u8; 2];
            if std::io::stdin().read_exact(&mut rest).is_err() {
                return Key::Quit;
            }
            match rest {
                [b'[', b'A'] => Key::Up,
                [b'[', b'B'] => Key::Down,
                _ => Key::Other,
            }
        }
        _ => Key::Other,
    }
}

/// Where a keypress leaves the cursor, or the choice it settles on.
///
/// Split out from the drawing so the wrap around can be tested without a
/// terminal.
pub fn step(cursor: usize, len: usize, key: &Key) -> Result<usize, usize> {
    match key {
        Key::Up => Err(cursor.checked_sub(1).unwrap_or(len - 1)),
        Key::Down => Err((cursor + 1) % len),
        Key::Digit(n) if *n >= 1 && *n <= len => Ok(n - 1),
        Key::Enter => Ok(cursor),
        _ => Err(cursor),
    }
}

/// Show a list, let the arrows move through it, return the chosen index.
///
/// Returns the starting index unchanged if the terminal will not go raw, so a
/// terminal preflight cannot drive is never a reason to fail.
pub fn select(title: &str, options: &[(&str, &str)], start: usize, footer: &str) -> usize {
    let Some(_raw) = RawMode::enter() else {
        return start;
    };
    let mut cursor = start;
    let mut out = std::io::stdout();

    println!("\n{title}");
    println!("\x1b[2m  arrows to move, enter to choose\x1b[0m\n");
    for _ in options {
        println!();
    }
    println!("\x1b[2m  {footer}\x1b[0m");

    loop {
        // Back up over the options and the footer, then redraw them.
        print!("\x1b[{}A", options.len() + 1);
        for (i, (name, description)) in options.iter().enumerate() {
            let row = format!("{:<9} {description}", name);
            match i == cursor {
                true => println!("\x1b[2K\x1b[36m> {row}\x1b[0m"),
                false => println!("\x1b[2K\x1b[2m  {row}\x1b[0m"),
            }
        }
        println!("\x1b[2K\x1b[2m  {footer}\x1b[0m");
        let _ = out.flush();

        let key = read_key();
        if matches!(key, Key::Quit) {
            return start;
        }
        match step(cursor, options.len(), &key) {
            Ok(chosen) => return chosen,
            Err(moved) => cursor = moved,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_wrap_at_both_ends() {
        assert_eq!(step(0, 3, &Key::Up), Err(2));
        assert_eq!(step(2, 3, &Key::Down), Err(0));
        assert_eq!(step(0, 3, &Key::Down), Err(1));
        assert_eq!(step(1, 3, &Key::Up), Err(0));
    }

    #[test]
    fn enter_takes_the_cursor_and_digits_jump() {
        assert_eq!(step(1, 3, &Key::Enter), Ok(1));
        assert_eq!(step(0, 3, &Key::Digit(3)), Ok(2));
        // out of range digits are ignored rather than selecting nothing
        assert_eq!(step(1, 3, &Key::Digit(9)), Err(1));
    }

    #[test]
    fn unknown_keys_leave_the_cursor_alone() {
        assert_eq!(step(1, 3, &Key::Other), Err(1));
    }
}
