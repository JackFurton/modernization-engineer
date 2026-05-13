# modernization-engineer

A practice project for porting C code to Rust the way a "modernization engineer"
would in a real codebase: incrementally, with the C build still running, swapping
one function at a time through FFI.

Target: [antirez's `kilo`](https://github.com/antirez/kilo) — a ~1.3 kloc terminal
text editor in a single C file.

## Layout

```
kilo-c/         antirez's kilo, partially gutted as functions move to Rust.
                Still builds and runs identically — just links a Rust staticlib.

kilo-rs/        A from-scratch idiomatic Rust port of the same editor.
                Independent binary; used as a reference design.

kilo-syntax/    The Rust staticlib that kilo-c links in. Each function here
                replaces a deleted C function. Crate name is historical — it
                covers more than syntax now.
```

## What's been ported (C → Rust via FFI)

All of these live in `kilo-syntax/src/lib.rs` and are linked into the C binary:

| Function                  | Lines (C) | Notable                                            |
|---------------------------|-----------|----------------------------------------------------|
| `editorSyntaxToColor`     | ~11       | Pure function, warm-up port                        |
| `editorRowsToString`      | ~22       | Returns libc-malloc'd buffer; C free()s it         |
| `is_separator`            | ~3        | Internalized as Rust-private helper                |
| `editorRowHasOpenComment` | ~6        | Internalized as Rust-private helper                |
| `editorUpdateSyntax`      | ~140      | State machine + recursion across rows              |
| `editorUpdateRow`         | ~30       | Tab expansion + delegates to `editorUpdateSyntax`  |

The C side keeps prototype declarations near the top of `kilo.c`; the deleted
bodies are replaced with breadcrumb comments pointing at the Rust source.

## Build & run

```sh
# the partially-Rust C binary:
cd kilo-c && make && ./kilo kilo.c

# the from-scratch Rust binary:
cd kilo-rs && cargo run -- path/to/file

# Rust-side unit tests:
cd kilo-syntax && cargo test
```

## Notes from the port

A few things that surfaced during this work, kept here because they're the
kind of thing that's easy to forget by the next port:

- **Allocator symmetry across the FFI.** Rust's default global allocator on
  macOS/Linux *happens to be* libc malloc — so `Vec::into_raw_parts()` round-trips
  through C's `free()` by luck. We use `libc::malloc/free/realloc` directly to
  make the contract explicit. Swap in `jemalloc` via `#[global_allocator]` and
  the lucky version corrupts the heap.

- **`#[repr(C)]` is the ABI contract.** Field order, types, and alignment must
  match the C struct byte for byte. The C compiler computes layout; Rust with
  `#[repr(C)]` agrees. Get one field wrong and you'll read garbage silently —
  no compile error, no runtime check.

- **Porting away from globals forces honest signatures.** kilo's C functions
  read `E.row`, `E.numrows`, `E.syntax` invisibly. Porting forces them into
  explicit params. Diffs at call sites are the visible scar of how much hidden
  coupling was there.

- **Fidelity beats fix.** kilo's tab expansion is slightly off-by-one from
  the standard convention (a leading tab produces 7 spaces, not 8). The port
  preserves the quirk. Modernization is behavior-preserving by default;
  "improvements" need authorization.

- **Verification harnesses live and die by you writing them.** Compile-time
  guarantees can't catch ABI drift or behavioral divergence across an FFI.
  Tiny standalone C programs that diff old-vs-new outputs are how this work
  actually gets verified.
