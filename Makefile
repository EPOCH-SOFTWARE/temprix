# TEMPRIX
#
#   make build       Rust CLI + TEMPRIX.app (Finder menu item, CLI bundled inside)
#   make test        unit + integration tests (never touch your real ~/.temprix)
#   make install     install TEMPRIX.app, put `temprix` on PATH, enable the
#                    Finder menu item, (re)start the daemon
#   make uninstall   undo install; history in ~/.temprix is kept
#   make release     universal, Developer ID signed, notarized, stapled
#                    dist/TEMPRIX-<version>.zip (see macos/release.sh)
#
# Local builds are ad-hoc signed and run on this Mac only.

VERSION       := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
SIGN_IDENTITY ?= -
APP           := target/macos/TEMPRIX.app
INSTALL_APP   ?= $(HOME)/Applications/TEMPRIX.app
BIN_DIR       ?= $(HOME)/.local/bin
EXTENSION_ID  := com.temprix.app.finder
APPEX          = $(INSTALL_APP)/Contents/PlugIns/TemprixFinder.appex
LSREGISTER    := /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

.PHONY: build test install uninstall release

build:
	cargo build --release
	SIGN_IDENTITY="$(SIGN_IDENTITY)" macos/build-app.sh target/release/temprix "$(VERSION)" target/macos

test:
	cargo test

install: build
	mkdir -p "$(dir $(INSTALL_APP))" "$(BIN_DIR)"
	@# Unregister the old extension before replacing its bundle. Replacing
	@# it in place leaves the extension registry inconsistent and Finder
	@# drops the menu item. A fresh registration starts disabled (-), and
	@# the switch to enabled (+) is what makes Finder load the new build.
	-pluginkit -r "$(APPEX)" 2>/dev/null
	rm -rf "$(INSTALL_APP)"
	ditto "$(APP)" "$(INSTALL_APP)"
	@# Register the temprix:// handler the Finder extension forwards clicks to.
	$(LSREGISTER) -f "$(INSTALL_APP)"
	pluginkit -a "$(APPEX)"
	pluginkit -e use -i $(EXTENSION_ID)
	@# One binary: the CLI on PATH is the signed one inside the app.
	ln -sf "$(INSTALL_APP)/Contents/MacOS/temprix" "$(BIN_DIR)/temprix"
	"$(BIN_DIR)/temprix" install
	@echo "Finder: right-click any file or folder → Show TEMPRIX History"

uninstall:
	-"$(BIN_DIR)/temprix" uninstall
	-pluginkit -e ignore -i $(EXTENSION_ID)
	-pluginkit -r "$(APPEX)"
	rm -rf "$(INSTALL_APP)"
	rm -f "$(BIN_DIR)/temprix"

release:
	macos/release.sh "$(VERSION)"
