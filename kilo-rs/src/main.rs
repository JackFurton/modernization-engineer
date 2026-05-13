mod editor;
mod input;
mod row;
mod terminal;

use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;

use editor::Editor;
use input::{is_ctrl, read_key, Key};
use terminal::RawMode;

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let fd = stdin.as_raw_fd();
    let _raw = RawMode::enable(fd)?;

    let mut editor = Editor::new(fd)?;
    if let Some(arg) = std::env::args().nth(1) {
        editor.open(PathBuf::from(arg))?;
    }
    editor.set_status("HELP: Ctrl-S = save | Ctrl-X = quit");

    loop {
        editor.refresh_screen()?;
        let key = read_key(fd)?;
        if !process_key(&mut editor, key)? {
            break;
        }
    }

    // leave the cursor on a fresh line
    let mut stdout = io::stdout().lock();
    write!(stdout, "\x1b[2J\x1b[H")?;
    stdout.flush()?;
    Ok(())
}

fn process_key(editor: &mut Editor, key: Key) -> io::Result<bool> {
    match key {
        Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End => {
            editor.move_cursor(key);
            editor.reset_quit();
        }
        Key::PageUp | Key::PageDown => {
            let dir = if key == Key::PageUp { Key::ArrowUp } else { Key::ArrowDown };
            for _ in 0..24 {
                editor.move_cursor(dir);
            }
            editor.reset_quit();
        }
        Key::Delete => {
            editor.move_cursor(Key::ArrowRight);
            editor.del_char();
            editor.reset_quit();
        }
        Key::Esc => editor.reset_quit(),
        Key::Char(c) => {
            if is_ctrl(c, b'x') {
                if editor.consume_quit_attempt() {
                    return Ok(false);
                }
            } else if is_ctrl(c, b's') {
                editor.save()?;
                editor.reset_quit();
            } else if c == b'\r' {
                editor.insert_newline();
                editor.reset_quit();
            } else if c == 127 || is_ctrl(c, b'h') {
                editor.del_char();
                editor.reset_quit();
            } else if c >= 32 || c == b'\t' {
                editor.insert_char(c);
                editor.reset_quit();
            }
        }
    }
    Ok(true)
}
