use std::fs;
use std::io::{self, Write};
use std::os::fd::RawFd;
use std::path::PathBuf;
use std::time::Instant;

use crate::input::Key;
use crate::row::Row;
use crate::terminal;

const KILO_VERSION: &str = "0.0.1";
const QUIT_TIMES: u8 = 3;

pub struct Editor {
    rows: Vec<Row>,
    cx: usize,
    cy: usize,
    rowoff: usize,
    coloff: usize,
    screen_rows: usize,
    screen_cols: usize,
    filename: Option<PathBuf>,
    dirty: bool,
    statusmsg: String,
    statusmsg_time: Instant,
    quit_times: u8,
}

impl Editor {
    pub fn new(fd: RawFd) -> io::Result<Self> {
        let (rows, cols) = terminal::window_size(fd)?;
        Ok(Editor {
            rows: Vec::new(),
            cx: 0,
            cy: 0,
            rowoff: 0,
            coloff: 0,
            screen_rows: rows.saturating_sub(2) as usize,
            screen_cols: cols as usize,
            filename: None,
            dirty: false,
            statusmsg: String::new(),
            statusmsg_time: Instant::now(),
            quit_times: QUIT_TIMES,
        })
    }

    pub fn open(&mut self, path: PathBuf) -> io::Result<()> {
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        self.rows.clear();
        for line in bytes.split(|&b| b == b'\n') {
            let line = strip_cr(line);
            self.rows.push(Row::new(line.to_vec()));
        }
        if let Some(last) = self.rows.last() {
            if last.chars.is_empty() {
                self.rows.pop();
            }
        }
        self.filename = Some(path);
        self.dirty = false;
        Ok(())
    }

    pub fn save(&mut self) -> io::Result<()> {
        let Some(path) = self.filename.clone() else {
            self.set_status("No filename. (save-as not implemented)");
            return Ok(());
        };
        let mut buf: Vec<u8> = Vec::new();
        for (i, row) in self.rows.iter().enumerate() {
            buf.extend_from_slice(&row.chars);
            if i + 1 < self.rows.len() {
                buf.push(b'\n');
            }
        }
        let len = buf.len();
        match fs::write(&path, &buf) {
            Ok(()) => {
                self.dirty = false;
                self.set_status(&format!("{} bytes written to disk", len));
                Ok(())
            }
            Err(e) => {
                self.set_status(&format!("Can't save! I/O error: {}", e));
                Ok(())
            }
        }
    }

    pub fn set_status(&mut self, msg: &str) {
        self.statusmsg = msg.to_string();
        self.statusmsg_time = Instant::now();
    }

    fn current_row(&self) -> Option<&Row> {
        self.rows.get(self.cy)
    }

    fn scroll(&mut self) {
        let rx = self.current_row().map(|r| r.cx_to_rx(self.cx)).unwrap_or(0);
        if self.cy < self.rowoff {
            self.rowoff = self.cy;
        }
        if self.cy >= self.rowoff + self.screen_rows {
            self.rowoff = self.cy - self.screen_rows + 1;
        }
        if rx < self.coloff {
            self.coloff = rx;
        }
        if rx >= self.coloff + self.screen_cols {
            self.coloff = rx - self.screen_cols + 1;
        }
    }

    pub fn refresh_screen(&mut self) -> io::Result<()> {
        self.scroll();
        let mut buf = String::new();
        buf.push_str("\x1b[?25l"); // hide cursor
        buf.push_str("\x1b[H"); // home

        self.draw_rows(&mut buf);
        self.draw_status_bar(&mut buf);
        self.draw_message_bar(&mut buf);

        // position cursor
        let rx = self
            .current_row()
            .map(|r| r.cx_to_rx(self.cx))
            .unwrap_or(0);
        let cy_screen = (self.cy - self.rowoff) + 1;
        let cx_screen = (rx - self.coloff) + 1;
        buf.push_str(&format!("\x1b[{};{}H", cy_screen, cx_screen));
        buf.push_str("\x1b[?25h"); // show cursor

        let mut stdout = io::stdout().lock();
        stdout.write_all(buf.as_bytes())?;
        stdout.flush()
    }

    fn draw_rows(&self, buf: &mut String) {
        for y in 0..self.screen_rows {
            let filerow = self.rowoff + y;
            if filerow >= self.rows.len() {
                if self.rows.is_empty() && y == self.screen_rows / 3 {
                    let welcome = format!("Kilo-rs editor -- version {}", KILO_VERSION);
                    let welcome = if welcome.len() > self.screen_cols {
                        &welcome[..self.screen_cols]
                    } else {
                        &welcome[..]
                    };
                    let pad = (self.screen_cols.saturating_sub(welcome.len())) / 2;
                    if pad > 0 {
                        buf.push('~');
                        for _ in 0..pad.saturating_sub(1) {
                            buf.push(' ');
                        }
                    }
                    buf.push_str(welcome);
                } else {
                    buf.push('~');
                }
            } else {
                let row = &self.rows[filerow];
                let len = row.render.len().saturating_sub(self.coloff);
                let len = len.min(self.screen_cols);
                if len > 0 {
                    let slice = &row.render[self.coloff..self.coloff + len];
                    buf.push_str(&String::from_utf8_lossy(slice));
                }
            }
            buf.push_str("\x1b[K"); // clear line to right
            buf.push_str("\r\n");
        }
    }

    fn draw_status_bar(&self, buf: &mut String) {
        buf.push_str("\x1b[7m"); // inverse video
        let name = self
            .filename
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("[No Name]");
        let modified = if self.dirty { "(modified)" } else { "" };
        let left = format!("{:.20} - {} lines {}", name, self.rows.len(), modified);
        let right = format!("{}/{}", self.cy + 1, self.rows.len());
        let mut shown = 0;
        let left_to_show = left.chars().take(self.screen_cols).collect::<String>();
        buf.push_str(&left_to_show);
        shown += left_to_show.len();
        while shown < self.screen_cols {
            if self.screen_cols - shown == right.len() {
                buf.push_str(&right);
                break;
            }
            buf.push(' ');
            shown += 1;
        }
        buf.push_str("\x1b[m");
        buf.push_str("\r\n");
    }

    fn draw_message_bar(&self, buf: &mut String) {
        buf.push_str("\x1b[K");
        if !self.statusmsg.is_empty() && self.statusmsg_time.elapsed().as_secs() < 5 {
            let msg = if self.statusmsg.len() > self.screen_cols {
                &self.statusmsg[..self.screen_cols]
            } else {
                &self.statusmsg[..]
            };
            buf.push_str(msg);
        }
    }

    pub fn move_cursor(&mut self, key: Key) {
        let row_len_at_cy = self.rows.get(self.cy).map(|r| r.chars.len()).unwrap_or(0);
        match key {
            Key::ArrowLeft => {
                if self.cx > 0 {
                    self.cx -= 1;
                } else if self.cy > 0 {
                    self.cy -= 1;
                    self.cx = self.rows[self.cy].chars.len();
                }
            }
            Key::ArrowRight => {
                if self.cx < row_len_at_cy {
                    self.cx += 1;
                } else if self.cy < self.rows.len() {
                    self.cy += 1;
                    self.cx = 0;
                }
            }
            Key::ArrowUp => {
                if self.cy > 0 {
                    self.cy -= 1;
                }
            }
            Key::ArrowDown => {
                if self.cy + 1 < self.rows.len() {
                    self.cy += 1;
                }
            }
            Key::Home => self.cx = 0,
            Key::End => self.cx = row_len_at_cy,
            _ => {}
        }
        // snap cx if we moved onto a shorter line
        let row_len = self.rows.get(self.cy).map(|r| r.chars.len()).unwrap_or(0);
        if self.cx > row_len {
            self.cx = row_len;
        }
    }

    pub fn insert_char(&mut self, c: u8) {
        if self.cy == self.rows.len() {
            self.rows.push(Row::new(Vec::new()));
        }
        self.rows[self.cy].insert_char(self.cx, c);
        self.cx += 1;
        self.dirty = true;
    }

    pub fn insert_newline(&mut self) {
        if self.cx == 0 {
            self.rows.insert(self.cy, Row::new(Vec::new()));
        } else {
            let row = &mut self.rows[self.cy];
            let tail = row.chars.split_off(self.cx);
            row.update_render();
            self.rows.insert(self.cy + 1, Row::new(tail));
        }
        self.cy += 1;
        self.cx = 0;
        self.dirty = true;
    }

    pub fn del_char(&mut self) {
        if self.cy == self.rows.len() {
            return;
        }
        if self.cx == 0 && self.cy == 0 {
            return;
        }
        if self.cx > 0 {
            self.rows[self.cy].del_char(self.cx - 1);
            self.cx -= 1;
        } else {
            let prev_len = self.rows[self.cy - 1].chars.len();
            let cur = self.rows.remove(self.cy);
            self.rows[self.cy - 1].append(&cur.chars);
            self.cy -= 1;
            self.cx = prev_len;
        }
        self.dirty = true;
    }

    pub fn consume_quit_attempt(&mut self) -> bool {
        if self.dirty && self.quit_times > 0 {
            self.set_status(&format!(
                "WARNING! File has unsaved changes. Press Ctrl-X {} more times to quit.",
                self.quit_times
            ));
            self.quit_times -= 1;
            false
        } else {
            true
        }
    }

    pub fn reset_quit(&mut self) {
        self.quit_times = QUIT_TIMES;
    }
}

fn strip_cr(line: &[u8]) -> &[u8] {
    if line.last() == Some(&b'\r') {
        &line[..line.len() - 1]
    } else {
        line
    }
}
