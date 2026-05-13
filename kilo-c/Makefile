RUST_LIB_DIR := ../kilo-syntax/target/release
RUST_LIB := $(RUST_LIB_DIR)/libkilo_syntax.a

.PHONY: all clean rust-lib

all: kilo

rust-lib:
	cd ../kilo-syntax && cargo build --release

kilo: kilo.c rust-lib
	$(CC) -o kilo kilo.c $(RUST_LIB) -Wall -W -pedantic -std=c99

clean:
	rm -f kilo
	cd ../kilo-syntax && cargo clean
