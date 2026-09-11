.PHONY: help build release install test check clean run panel

PREFIX ?= $(HOME)/.local
BINDIR = $(PREFIX)/bin

help:
	@echo "wizctl (Rust + egui) build & development targets:"
	@echo "  make build       - Build release binary (target/release/wizctl)"
	@echo "  make test        - Run complete unit and integration test suite"
	@echo "  make check       - Fast compile check via cargo check"
	@echo "  make install     - Install binary to ~/.local/bin and install panel widget"
	@echo "  make panel       - Configure and reload XFCE4 panel launcher"
	@echo "  make clean       - Remove cargo build artifacts"
	@echo "  make run         - Run the popover widget"

build:
	cargo build --release

release: build

check:
	cargo check

test:
	cargo test

install: build
	mkdir -p $(BINDIR)
	cp target/release/wizctl $(BINDIR)/wizctl
	chmod +x $(BINDIR)/wizctl
	./scripts/setup_panel_widget.sh

panel:
	./scripts/setup_panel_widget.sh

clean:
	cargo clean

run:
	cargo run -- widget
