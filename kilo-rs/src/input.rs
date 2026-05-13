use std::io;
use std::os::fd::RawFd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(u8),
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Esc,
}

fn read_byte(fd: RawFd) -> io::Result<Option<u8>> {
    let mut c = 0u8;
    let n = unsafe { libc::read(fd, &mut c as *mut u8 as *mut libc::c_void, 1) };
    match n {
        -1 => Err(io::Error::last_os_error()),
        0 => Ok(None),
        _ => Ok(Some(c)),
    }
}

fn read_byte_blocking(fd: RawFd) -> io::Result<u8> {
    loop {
        if let Some(b) = read_byte(fd)? {
            return Ok(b);
        }
    }
}

pub fn read_key(fd: RawFd) -> io::Result<Key> {
    let c = read_byte_blocking(fd)?;
    if c != 0x1b {
        return Ok(Key::Char(c));
    }

    let seq0 = match read_byte(fd)? {
        Some(b) => b,
        None => return Ok(Key::Esc),
    };
    let seq1 = match read_byte(fd)? {
        Some(b) => b,
        None => return Ok(Key::Esc),
    };

    match seq0 {
        b'[' => {
            if seq1.is_ascii_digit() {
                let seq2 = match read_byte(fd)? {
                    Some(b) => b,
                    None => return Ok(Key::Esc),
                };
                if seq2 == b'~' {
                    return Ok(match seq1 {
                        b'3' => Key::Delete,
                        b'5' => Key::PageUp,
                        b'6' => Key::PageDown,
                        _ => Key::Esc,
                    });
                }
                Ok(Key::Esc)
            } else {
                Ok(match seq1 {
                    b'A' => Key::ArrowUp,
                    b'B' => Key::ArrowDown,
                    b'C' => Key::ArrowRight,
                    b'D' => Key::ArrowLeft,
                    b'H' => Key::Home,
                    b'F' => Key::End,
                    _ => Key::Esc,
                })
            }
        }
        b'O' => Ok(match seq1 {
            b'H' => Key::Home,
            b'F' => Key::End,
            _ => Key::Esc,
        }),
        _ => Ok(Key::Esc),
    }
}

pub fn is_ctrl(c: u8, letter: u8) -> bool {
    c == (letter & 0x1f)
}
