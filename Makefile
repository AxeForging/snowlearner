MODEL ?= $(HOME)/.local/share/snowlearner/models/ggml-base.bin

.PHONY: build release test test-speech lint fmt snapshot

build:
	cargo build

release:
	cargo build --release

test:
	cargo test
	cargo test --no-default-features

test-speech:
	SNOWLEARNER_TEST_MODEL=$(MODEL) cargo test --test speech_e2e -- --ignored

lint:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo clippy --all-targets --no-default-features -- -D warnings

fmt:
	cargo fmt

snapshot:
	cargo run -- snapshot snapshot.png --seconds 120 --caption
