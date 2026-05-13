use std::os::raw::{c_char, c_int, c_uchar};
use std::ptr;
use std::slice;

const HL_NORMAL: u8 = 0;
const HL_NONPRINT: u8 = 1;
const HL_COMMENT_U8: u8 = 2;
const HL_MLCOMMENT_U8: u8 = 3;
const HL_KEYWORD1_U8: u8 = 4;
const HL_KEYWORD2_U8: u8 = 5;
const HL_STRING_U8: u8 = 6;
const HL_NUMBER_U8: u8 = 7;

const HL_COMMENT: c_int = HL_COMMENT_U8 as c_int;
const HL_MLCOMMENT: c_int = HL_MLCOMMENT_U8 as c_int;
const HL_KEYWORD1: c_int = HL_KEYWORD1_U8 as c_int;
const HL_KEYWORD2: c_int = HL_KEYWORD2_U8 as c_int;
const HL_STRING: c_int = HL_STRING_U8 as c_int;
const HL_NUMBER: c_int = HL_NUMBER_U8 as c_int;
const HL_MATCH: c_int = 8;

#[no_mangle]
pub extern "C" fn editorSyntaxToColor(hl: c_int) -> c_int {
    match hl {
        HL_COMMENT | HL_MLCOMMENT => 36,
        HL_KEYWORD1 => 33,
        HL_KEYWORD2 => 32,
        HL_STRING => 35,
        HL_NUMBER => 31,
        HL_MATCH => 34,
        _ => 37,
    }
}

/// Mirrors `struct erow` from kilo.c. Layout must match exactly.
#[repr(C)]
pub struct Erow {
    pub idx: c_int,
    pub size: c_int,
    pub rsize: c_int,
    pub chars: *mut c_char,
    pub render: *mut c_char,
    pub hl: *mut c_uchar,
    pub hl_oc: c_int,
}

/// Walks `numrows` rows, joining their `chars` fields with `\n` separators,
/// returns a libc-malloc'd nul-terminated buffer that the C caller frees with
/// `free()`. Writes total bytes (excluding the nul) to *buflen.
///
/// # Safety
/// - `rows` must point to `numrows` valid `Erow` values.
/// - Each row's `chars` must point to at least `size` valid bytes (or be NULL when size==0).
/// - `buflen` must be a valid `*mut c_int`.
/// - Caller must release the returned pointer with libc `free()`.
#[no_mangle]
pub unsafe extern "C" fn editorRowsToString(
    rows: *const Erow,
    numrows: c_int,
    buflen: *mut c_int,
) -> *mut c_char {
    let n = numrows.max(0) as usize;
    let rows_slice = if n == 0 || rows.is_null() {
        &[][..]
    } else {
        slice::from_raw_parts(rows, n)
    };

    let total: usize = rows_slice
        .iter()
        .map(|r| r.size.max(0) as usize + 1) // +1 for '\n' per row
        .sum();

    // libc::malloc so the C caller's free() is symmetric.
    let buf = libc::malloc(total + 1) as *mut u8;
    if buf.is_null() {
        if !buflen.is_null() {
            *buflen = 0;
        }
        return ptr::null_mut();
    }

    let mut p = buf;
    for r in rows_slice {
        let sz = r.size.max(0) as usize;
        if sz > 0 && !r.chars.is_null() {
            ptr::copy_nonoverlapping(r.chars as *const u8, p, sz);
            p = p.add(sz);
        }
        *p = b'\n';
        p = p.add(1);
    }
    *p = 0;

    if !buflen.is_null() {
        *buflen = total as c_int;
    }
    buf as *mut c_char
}

/// Mirrors `struct editorSyntax` from kilo.c. Layout must match exactly.
#[repr(C)]
pub struct EditorSyntax {
    pub filematch: *mut *mut c_char,
    pub keywords: *mut *mut c_char,
    pub singleline_comment_start: [c_char; 2],
    pub multiline_comment_start: [c_char; 3],
    pub multiline_comment_end: [c_char; 3],
    pub flags: c_int,
}

fn is_separator(c: u8) -> bool {
    c == 0 || c.is_ascii_whitespace() || b",.()+-/*=~%[];".contains(&c)
}

fn is_printable(c: u8) -> bool {
    (0x20..0x7f).contains(&c)
}

/// True if `row` ends inside an unterminated /* ... */ comment that should
/// continue into the next row. Mirrors `editorRowHasOpenComment`.
unsafe fn row_has_open_comment(row: &Erow) -> bool {
    if row.hl.is_null() || row.rsize <= 0 {
        return false;
    }
    let rsize = row.rsize as usize;
    let hl = slice::from_raw_parts(row.hl, rsize);
    if hl[rsize - 1] != HL_MLCOMMENT_U8 {
        return false;
    }
    if rsize < 2 {
        return true;
    }
    let render = slice::from_raw_parts(row.render as *const u8, rsize);
    !(render[rsize - 2] == b'*' && render[rsize - 1] == b'/')
}

/// Walk a NULL-terminated C string and return it as a byte slice. Lifetime is
/// up to the caller — used here as a borrow tied to the loop iteration.
unsafe fn cstr_bytes<'a>(ptr: *const c_char) -> &'a [u8] {
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    slice::from_raw_parts(ptr as *const u8, len)
}

/// Match `filename` against an HLDB. Patterns starting with '.' match only as
/// a suffix (extension); others match as a plain substring. Returns a pointer
/// to the matched `EditorSyntax` entry, or NULL.
///
/// # Safety
/// - `hldb` must point to `hldb_count` valid `EditorSyntax` entries.
/// - Each entry's `filematch` must be a NULL-terminated array of valid C strings.
/// - `filename` must be a valid C string.
#[no_mangle]
pub unsafe extern "C" fn editorSelectSyntaxHighlight(
    hldb: *mut EditorSyntax,
    hldb_count: c_int,
    filename: *const c_char,
) -> *mut EditorSyntax {
    if hldb.is_null() || hldb_count <= 0 || filename.is_null() {
        return ptr::null_mut();
    }
    let fname = cstr_bytes(filename);

    for j in 0..hldb_count as usize {
        let s = hldb.add(j);
        let filematch = (*s).filematch;
        if filematch.is_null() {
            continue;
        }
        let mut i = 0usize;
        loop {
            let pat_ptr = *filematch.add(i);
            if pat_ptr.is_null() {
                break;
            }
            let pat = cstr_bytes::<'_>(pat_ptr);
            if let Some(pos) = fname.windows(pat.len()).position(|w| w == pat) {
                // Extension patterns must match at end; others match anywhere.
                let is_extension = pat.first() == Some(&b'.');
                if !is_extension || pos + pat.len() == fname.len() {
                    return s;
                }
            }
            i += 1;
        }
    }
    ptr::null_mut()
}

unsafe fn keyword_slice(ptr: *const c_char) -> &'static [u8] {
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    slice::from_raw_parts(ptr as *const u8, len)
}

/// Private helper: free a row's three heap-allocated fields. Was
/// `editorFreeRow` in kilo.c; its only caller (editorDelRow) is now Rust too,
/// so this is no longer exported.
unsafe fn free_row_internal(row: *mut Erow) {
    if row.is_null() {
        return;
    }
    let r = &mut *row;
    if !r.render.is_null() {
        libc::free(r.render as *mut libc::c_void);
        r.render = ptr::null_mut();
    }
    if !r.chars.is_null() {
        libc::free(r.chars as *mut libc::c_void);
        r.chars = ptr::null_mut();
    }
    if !r.hl.is_null() {
        libc::free(r.hl as *mut libc::c_void);
        r.hl = ptr::null_mut();
    }
}

/// Insert a new row at position `at`. Reallocates the row array (so we take
/// `rows: **Erow` and write back the new base pointer), copies `len` bytes
/// from `s` into the new row's chars, then runs editorUpdateRow on it.
///
/// # Safety
/// - `rows` must point at the location holding the C-side `E.row` pointer.
/// - `numrows` must point at the C-side `E.numrows`.
/// - `s` must be readable for `len` bytes (or NULL when len==0).
/// - All existing rows in the array must be valid `Erow` values.
#[no_mangle]
pub unsafe extern "C" fn editorInsertRow(
    rows: *mut *mut Erow,
    numrows: *mut c_int,
    syntax: *const EditorSyntax,
    at: c_int,
    s: *const c_char,
    len: usize,
) {
    if rows.is_null() || numrows.is_null() {
        return;
    }
    let n = *numrows;
    if at < 0 || at > n {
        return;
    }

    let new_n = n + 1;
    let new_size = (new_n as usize) * core::mem::size_of::<Erow>();
    let new_rows = libc::realloc(*rows as *mut libc::c_void, new_size) as *mut Erow;
    if new_rows.is_null() {
        return;
    }
    *rows = new_rows;

    if at != n {
        // shift rows [at..n] down by one
        let src = new_rows.add(at as usize);
        let dst = new_rows.add((at + 1) as usize);
        ptr::copy(src, dst, (n - at) as usize);
        // bump idx of the shifted rows
        for j in (at + 1)..=n {
            (*new_rows.add(j as usize)).idx += 1;
        }
    }

    let new_row = &mut *new_rows.add(at as usize);
    new_row.idx = at;
    new_row.size = len as c_int;
    new_row.rsize = 0;
    new_row.hl_oc = 0;
    new_row.hl = ptr::null_mut();
    new_row.render = ptr::null_mut();

    let chars_buf = libc::malloc(len + 1) as *mut c_char;
    new_row.chars = chars_buf;
    if !chars_buf.is_null() {
        if !s.is_null() && len > 0 {
            ptr::copy_nonoverlapping(s, chars_buf, len);
        }
        *chars_buf.add(len) = 0;
    }

    *numrows = new_n;
    editorUpdateRow(new_rows, new_n, syntax, at);
}

/// Remove row at `at`. Frees its heap buffers, shifts subsequent rows up.
///
/// Fixes a kilo bug: the C original bumps `idx++` for shifted rows when it
/// should bump `idx--`. The shifted rows moved up in the array, so their
/// stored index should decrease, not increase.
#[no_mangle]
pub unsafe extern "C" fn editorDelRow(
    rows: *mut *mut Erow,
    numrows: *mut c_int,
    at: c_int,
) {
    if rows.is_null() || numrows.is_null() {
        return;
    }
    let n = *numrows;
    if at < 0 || at >= n {
        return;
    }
    let arr = *rows;
    free_row_internal(arr.add(at as usize));

    if at + 1 < n {
        let src = arr.add((at + 1) as usize);
        let dst = arr.add(at as usize);
        ptr::copy(src, dst, (n - at - 1) as usize);
    }
    // bug fix: decrement idx for rows that moved up (kilo.c had ++ here)
    for j in at..(n - 1) {
        (*arr.add(j as usize)).idx -= 1;
    }
    *numrows = n - 1;
}

/// Insert byte `c` into the row at `row_idx`, at position `at`. If `at` is
/// past the end of the row, pads with spaces first. Reruns editorUpdateRow.
#[no_mangle]
pub unsafe extern "C" fn editorRowInsertChar(
    rows: *mut Erow,
    numrows: c_int,
    syntax: *const EditorSyntax,
    row_idx: c_int,
    at: c_int,
    c: c_int,
) {
    if rows.is_null() || row_idx < 0 || row_idx >= numrows {
        return;
    }
    let row = &mut *rows.add(row_idx as usize);
    let size = row.size.max(0) as usize;
    let at = at.max(0) as usize;

    if at > size {
        let padlen = at - size;
        let new_chars = libc::realloc(row.chars as *mut libc::c_void, size + padlen + 2) as *mut c_char;
        if new_chars.is_null() {
            return;
        }
        row.chars = new_chars;
        libc::memset(new_chars.add(size) as *mut libc::c_void, b' ' as c_int, padlen);
        *new_chars.add(size + padlen + 1) = 0;
        row.size = (size + padlen + 1) as c_int;
    } else {
        let new_chars = libc::realloc(row.chars as *mut libc::c_void, size + 2) as *mut c_char;
        if new_chars.is_null() {
            return;
        }
        row.chars = new_chars;
        // shift [at..=size] -> [at+1..=size+1] (includes the nul at index size)
        ptr::copy(new_chars.add(at), new_chars.add(at + 1), size - at + 1);
        row.size = (size + 1) as c_int;
    }
    *row.chars.add(at) = c as c_char;

    editorUpdateRow(rows, numrows, syntax, row_idx);
}

/// Delete byte at position `at` from the row at `row_idx`.
#[no_mangle]
pub unsafe extern "C" fn editorRowDelChar(
    rows: *mut Erow,
    numrows: c_int,
    syntax: *const EditorSyntax,
    row_idx: c_int,
    at: c_int,
) {
    if rows.is_null() || row_idx < 0 || row_idx >= numrows {
        return;
    }
    let row = &mut *rows.add(row_idx as usize);
    let size = row.size.max(0) as usize;
    let at = at.max(0) as usize;
    if at >= size {
        return;
    }
    // shift down [at+1..=size] -> [at..size]; size-at+1 bytes including nul
    ptr::copy(row.chars.add(at + 1), row.chars.add(at), size - at);
    row.size = (size - 1) as c_int;
    editorUpdateRow(rows, numrows, syntax, row_idx);
}

/// Append `len` bytes from `s` to the row at `row_idx`.
#[no_mangle]
pub unsafe extern "C" fn editorRowAppendString(
    rows: *mut Erow,
    numrows: c_int,
    syntax: *const EditorSyntax,
    row_idx: c_int,
    s: *const c_char,
    len: usize,
) {
    if rows.is_null() || row_idx < 0 || row_idx >= numrows {
        return;
    }
    let row = &mut *rows.add(row_idx as usize);
    let size = row.size.max(0) as usize;
    let new_chars = libc::realloc(row.chars as *mut libc::c_void, size + len + 1) as *mut c_char;
    if new_chars.is_null() {
        return;
    }
    row.chars = new_chars;
    if !s.is_null() && len > 0 {
        ptr::copy_nonoverlapping(s, new_chars.add(size), len);
    }
    *new_chars.add(size + len) = 0;
    row.size = (size + len) as c_int;
    editorUpdateRow(rows, numrows, syntax, row_idx);
}

/// Rebuild `row->render` (with tabs expanded to spaces aligned to TAB_STOP=8)
/// and recompute its syntax highlighting. `row->render` is libc-malloc'd so
/// `editorFreeRow`'s `free()` remains symmetric.
///
/// # Safety
/// - `rows` must point to `numrows` valid `Erow` values.
/// - The target row's `chars` field must be valid for `size` bytes (or NULL when size==0).
/// - Caller must ensure `row_idx < numrows` (the row exists).
#[no_mangle]
pub unsafe extern "C" fn editorUpdateRow(
    rows: *mut Erow,
    numrows: c_int,
    syntax: *const EditorSyntax,
    row_idx: c_int,
) {
    if rows.is_null() || row_idx < 0 || row_idx >= numrows {
        return;
    }
    let row = &mut *rows.add(row_idx as usize);
    let size = row.size.max(0) as usize;

    if !row.render.is_null() {
        libc::free(row.render as *mut libc::c_void);
        row.render = ptr::null_mut();
    }

    let chars = if size > 0 && !row.chars.is_null() {
        slice::from_raw_parts(row.chars as *const u8, size)
    } else {
        &[][..]
    };
    let tabs = chars.iter().filter(|&&c| c == b'\t').count();
    let alloc_size = size + tabs * 8 + 1;

    let render = libc::malloc(alloc_size) as *mut u8;
    if render.is_null() {
        row.rsize = 0;
        editorUpdateSyntax(rows, numrows, syntax, row_idx);
        return;
    }

    let mut idx = 0usize;
    for &c in chars {
        if c == b'\t' {
            *render.add(idx) = b' ';
            idx += 1;
            while (idx + 1) % 8 != 0 {
                *render.add(idx) = b' ';
                idx += 1;
            }
        } else {
            *render.add(idx) = c;
            idx += 1;
        }
    }
    *render.add(idx) = 0;

    row.render = render as *mut c_char;
    row.rsize = idx as c_int;

    editorUpdateSyntax(rows, numrows, syntax, row_idx);
}

/// Recompute `row->hl` for the row at `row_idx`. Propagates to the next row
/// if the open-comment state changed.
///
/// # Safety
/// - `rows` must point to `numrows` valid `Erow` values.
/// - Each row's `render` must be a valid pointer of at least `rsize` bytes.
/// - `row->hl` is reallocated via libc::realloc; callers must free with libc::free.
/// - `syntax` may be NULL (no highlighting) or must point to a valid EditorSyntax
///   with `keywords` a NULL-terminated array of nul-terminated C strings.
#[no_mangle]
pub unsafe extern "C" fn editorUpdateSyntax(
    rows: *mut Erow,
    numrows: c_int,
    syntax: *const EditorSyntax,
    row_idx: c_int,
) {
    if rows.is_null() || row_idx < 0 || row_idx >= numrows {
        return;
    }
    let row = &mut *rows.add(row_idx as usize);
    let rsize = row.rsize.max(0) as usize;

    // realloc hl buffer to rsize bytes, all HL_NORMAL.
    row.hl = libc::realloc(row.hl as *mut libc::c_void, rsize) as *mut c_uchar;
    if rsize > 0 && !row.hl.is_null() {
        libc::memset(row.hl as *mut libc::c_void, HL_NORMAL as c_int, rsize);
    }

    if syntax.is_null() || rsize == 0 || row.render.is_null() {
        // no highlighting; still propagate hl_oc since it may have changed
        let oc = false;
        if row.hl_oc != 0 && (row_idx + 1) < numrows {
            row.hl_oc = oc as c_int;
            editorUpdateSyntax(rows, numrows, syntax, row_idx + 1);
        } else {
            row.hl_oc = oc as c_int;
        }
        return;
    }

    let syntax = &*syntax;
    let render = slice::from_raw_parts(row.render as *const u8, rsize);
    let hl = slice::from_raw_parts_mut(row.hl, rsize);

    let scs = [syntax.singleline_comment_start[0] as u8, syntax.singleline_comment_start[1] as u8];
    let mcs = [syntax.multiline_comment_start[0] as u8, syntax.multiline_comment_start[1] as u8];
    let mce = [syntax.multiline_comment_end[0] as u8, syntax.multiline_comment_end[1] as u8];

    // Start at first non-space char in render. Note: render uses the on-screen
    // (tab-expanded) text, so leading whitespace is just real spaces.
    let mut i = 0usize;
    while i < rsize && render[i].is_ascii_whitespace() {
        i += 1;
    }
    let mut prev_sep = true;
    let mut in_string: u8 = 0;
    let mut in_comment = false;

    // Inherit open-comment state from previous row.
    if row_idx > 0 {
        let prev = &*rows.add((row_idx - 1) as usize);
        if row_has_open_comment(prev) {
            in_comment = true;
        }
    }

    while i < rsize {
        let c = render[i];
        let next = if i + 1 < rsize { render[i + 1] } else { 0 };

        // single-line comment
        if !in_comment && in_string == 0 && prev_sep && c == scs[0] && next == scs[1] && scs[0] != 0 {
            for j in i..rsize {
                hl[j] = HL_COMMENT_U8;
            }
            break;
        }

        // multi-line comment handling
        if in_comment {
            hl[i] = HL_MLCOMMENT_U8;
            if c == mce[0] && next == mce[1] {
                if i + 1 < rsize {
                    hl[i + 1] = HL_MLCOMMENT_U8;
                }
                i += 2;
                in_comment = false;
                prev_sep = true;
                continue;
            } else {
                prev_sep = false;
                i += 1;
                continue;
            }
        } else if c == mcs[0] && next == mcs[1] && mcs[0] != 0 {
            hl[i] = HL_MLCOMMENT_U8;
            if i + 1 < rsize {
                hl[i + 1] = HL_MLCOMMENT_U8;
            }
            i += 2;
            in_comment = true;
            prev_sep = false;
            continue;
        }

        // string handling
        if in_string != 0 {
            hl[i] = HL_STRING_U8;
            if c == b'\\' {
                if i + 1 < rsize {
                    hl[i + 1] = HL_STRING_U8;
                }
                i += 2;
                prev_sep = false;
                continue;
            }
            if c == in_string {
                in_string = 0;
            }
            i += 1;
            continue;
        } else if c == b'"' || c == b'\'' {
            in_string = c;
            hl[i] = HL_STRING_U8;
            i += 1;
            prev_sep = false;
            continue;
        }

        // non-printable
        if !is_printable(c) {
            hl[i] = HL_NONPRINT;
            i += 1;
            prev_sep = false;
            continue;
        }

        // numbers
        let prev_was_number = i > 0 && hl[i - 1] == HL_NUMBER_U8;
        if (c.is_ascii_digit() && (prev_sep || prev_was_number))
            || (c == b'.' && prev_was_number)
        {
            hl[i] = HL_NUMBER_U8;
            i += 1;
            prev_sep = false;
            continue;
        }

        // keywords
        if prev_sep && !syntax.keywords.is_null() {
            let mut k = 0usize;
            let mut matched = false;
            loop {
                let kp = *syntax.keywords.add(k);
                if kp.is_null() {
                    break;
                }
                let kw_full = keyword_slice(kp);
                let kw2 = kw_full.last() == Some(&b'|');
                let kw = if kw2 { &kw_full[..kw_full.len() - 1] } else { kw_full };
                let klen = kw.len();

                if klen > 0
                    && i + klen <= rsize
                    && &render[i..i + klen] == kw
                {
                    let after = if i + klen < rsize { render[i + klen] } else { 0 };
                    if is_separator(after) {
                        let color = if kw2 { HL_KEYWORD2_U8 } else { HL_KEYWORD1_U8 };
                        for j in i..i + klen {
                            hl[j] = color;
                        }
                        i += klen;
                        matched = true;
                        break;
                    }
                }
                k += 1;
            }
            if matched {
                prev_sep = false;
                continue;
            }
        }

        prev_sep = is_separator(c);
        i += 1;
    }

    // propagate to next row if open-comment state changed
    let oc = row_has_open_comment(row);
    let oc_changed = (row.hl_oc != 0) != oc;
    row.hl_oc = oc as c_int;
    if oc_changed && (row_idx + 1) < numrows {
        editorUpdateSyntax(rows, numrows, syntax as *const EditorSyntax, row_idx + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn make_erow(idx: c_int, text: &[u8]) -> Erow {
        let chars = libc::malloc(text.len() + 1) as *mut c_char;
        if !text.is_empty() {
            ptr::copy_nonoverlapping(text.as_ptr() as *const c_char, chars, text.len());
        }
        *chars.add(text.len()) = 0;
        Erow {
            idx,
            size: text.len() as c_int,
            rsize: 0,
            chars,
            render: ptr::null_mut(),
            hl: ptr::null_mut(),
            hl_oc: 0,
        }
    }

    unsafe fn free_erow(row: &mut Erow) {
        if !row.chars.is_null() {
            libc::free(row.chars as *mut libc::c_void);
        }
        if !row.render.is_null() {
            libc::free(row.render as *mut libc::c_void);
        }
        if !row.hl.is_null() {
            libc::free(row.hl as *mut libc::c_void);
        }
    }

    #[test]
    fn syntax_to_color_default_is_white() {
        assert_eq!(editorSyntaxToColor(0), 37);
        assert_eq!(editorSyntaxToColor(999), 37);
    }

    #[test]
    fn syntax_to_color_comment_variants_both_cyan() {
        assert_eq!(editorSyntaxToColor(HL_COMMENT), 36);
        assert_eq!(editorSyntaxToColor(HL_MLCOMMENT), 36);
    }

    #[test]
    fn update_row_expands_tab_mid_line() {
        unsafe {
            let mut rows = [make_erow(0, b"abc\tx")];
            editorUpdateRow(rows.as_mut_ptr(), 1, ptr::null(), 0);
            // kilo's tab expansion: leading tab → 7 spaces (off-by-one from convention),
            // so "abc\tx" → "abc" + 4 spaces + "x" = 8 bytes.
            assert_eq!(rows[0].rsize, 8);
            let render = std::slice::from_raw_parts(rows[0].render as *const u8, 8);
            assert_eq!(render, b"abc    x");
            free_erow(&mut rows[0]);
        }
    }

    #[test]
    fn rows_to_string_joins_with_trailing_newlines() {
        unsafe {
            let mut rows = [make_erow(0, b"hello"), make_erow(1, b"world")];
            let mut len: c_int = -1;
            let buf = editorRowsToString(rows.as_ptr(), 2, &mut len);
            assert_eq!(len, 12); // "hello\nworld\n"
            let s = std::slice::from_raw_parts(buf as *const u8, len as usize);
            assert_eq!(s, b"hello\nworld\n");
            // Round-trip through C's free path:
            libc::free(buf as *mut libc::c_void);
            free_erow(&mut rows[0]);
            free_erow(&mut rows[1]);
        }
    }

    #[test]
    fn rows_to_string_empty_input() {
        unsafe {
            let mut len: c_int = -1;
            let buf = editorRowsToString(ptr::null(), 0, &mut len);
            assert_eq!(len, 0);
            assert_eq!(*buf, 0);
            libc::free(buf as *mut libc::c_void);
        }
    }

    #[test]
    fn is_separator_basics() {
        assert!(is_separator(b' '));
        assert!(is_separator(b','));
        assert!(is_separator(b';'));
        assert!(is_separator(0));
        assert!(!is_separator(b'a'));
        assert!(!is_separator(b'_'));
    }
}
