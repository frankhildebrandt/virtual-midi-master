.DEFAULT_GOAL := help

CARGO ?= cargo
NPM ?= npm
RUSTC ?= rustc

DESKTOP_DIR := apps/desktop
TAURI_DIR := $(DESKTOP_DIR)/src-tauri
HOST_TRIPLE := $(shell $(RUSTC) -vV | sed -n 's/^host: //p')
WINDOWS_SUFFIX := $(if $(findstring windows,$(HOST_TRIPLE)),.exe,)
ENGINE_DEBUG := target/debug/virtual-midi-engine$(WINDOWS_SUFFIX)
ENGINE_RELEASE := target/release/virtual-midi-engine$(WINDOWS_SUFFIX)
SIDECAR := $(TAURI_DIR)/bin/virtual-midi-engine-$(HOST_TRIPLE)$(WINDOWS_SUFFIX)
NODE_MODULES_STAMP := $(DESKTOP_DIR)/node_modules/.package-lock.json

ifneq ($(findstring windows,$(HOST_TRIPLE)),)
CARGO_PLATFORM := powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/with-windows-midi-sdk.ps1 $(CARGO)
TAURI := powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/with-windows-midi-sdk.ps1 $(NPM) --prefix $(DESKTOP_DIR) run tauri --
DEBUG_BUNDLE_ARGS := --bundles nsis
else
CARGO_PLATFORM := $(CARGO)
TAURI := cd $(DESKTOP_DIR) && $(NPM) run tauri --
ifneq ($(findstring apple-darwin,$(HOST_TRIPLE)),)
DEBUG_BUNDLE_ARGS := --bundles app
endif
endif

.PHONY: help setup setup-platform setup-macos setup-windows deps \
	dev dev-ui dev-engine preview-ui \
	build build-all build-engine build-engine-release build-ui \
	prepare-sidecar prepare-sidecar-release build-app build-app-debug \
	package package-macos package-windows \
	check ci cargo-check fmt fmt-check clippy test test-soak \
	clean clean-rust clean-ui

help: ## Zeigt alle verfügbaren Make-Targets
	@printf 'Virtual MIDI Master\n\n'
	@printf 'Verwendung: make <target>\n\n'
	@awk 'BEGIN { FS = ":.*## "; } /^[a-zA-Z0-9_.-]+:.*## / { printf "  %-25s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

setup: deps setup-platform ## Installiert Node- und plattformspezifische Build-Abhängigkeiten

setup-platform: ## Installiert die Build-Abhängigkeiten der aktuellen Plattform
	@case "$(HOST_TRIPLE)" in \
		*-apple-darwin) $(MAKE) setup-macos ;; \
		*-pc-windows-msvc) $(MAKE) setup-windows ;; \
		*) printf 'Keine zusätzlichen Plattform-Abhängigkeiten für %s.\n' "$(HOST_TRIPLE)" ;; \
	esac

setup-macos: ## Installiert die Rust-Targets für universelle macOS-Builds
	rustup target add aarch64-apple-darwin x86_64-apple-darwin

setup-windows: ## Installiert das gepinnte Windows MIDI Services App SDK
	powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/prepare-windows-midi-sdk.ps1

deps: $(NODE_MODULES_STAMP) ## Installiert reproduzierbar die Node-Abhängigkeiten

$(NODE_MODULES_STAMP): $(DESKTOP_DIR)/package.json $(DESKTOP_DIR)/package-lock.json
	cd $(DESKTOP_DIR) && $(NPM) ci

dev: deps prepare-sidecar ## Startet die vollständige Tauri-App im Entwicklungsmodus
	$(TAURI) dev

dev-ui: deps ## Startet nur das React/Vite-Frontend
	cd $(DESKTOP_DIR) && $(NPM) run dev

dev-engine: ## Startet nur die Rust-MIDI-Engine
	$(CARGO_PLATFORM) run -p virtual-midi-engine --bin virtual-midi-engine

preview-ui: deps build-ui ## Startet eine Vorschau des gebauten React-Frontends
	cd $(DESKTOP_DIR) && $(NPM) run preview

build: build-engine build-ui ## Baut Engine und Web-Frontend im Debug-/Produktionsmodus

build-all: build build-app ## Baut alle Komponenten und das native Release-App-Bundle

build-engine: ## Baut die Rust-Engine im Debug-Profil
	$(CARGO_PLATFORM) build -p virtual-midi-engine --bin virtual-midi-engine

build-engine-release: ## Baut die Rust-Engine im Release-Profil
	$(CARGO_PLATFORM) build --release -p virtual-midi-engine --bin virtual-midi-engine

build-ui: deps ## Prüft TypeScript und baut das React-Frontend
	cd $(DESKTOP_DIR) && $(NPM) run build

prepare-sidecar: build-engine ## Kopiert die Debug-Engine in Tauri mit dem Host-Targetnamen
	mkdir -p $(TAURI_DIR)/bin
	cp $(ENGINE_DEBUG) $(SIDECAR)

prepare-sidecar-release: build-engine-release ## Kopiert die Release-Engine in Tauri
	mkdir -p $(TAURI_DIR)/bin
	cp $(ENGINE_RELEASE) $(SIDECAR)

build-app: deps prepare-sidecar-release ## Baut das native Tauri-Release-Bundle für den aktuellen Host
	$(TAURI) build

build-app-debug: deps prepare-sidecar ## Baut native Tauri-Debug-Bundles für den aktuellen Host
	$(TAURI) build --debug $(DEBUG_BUNDLE_ARGS)

package: ## Erzeugt das installierbare Paket für das aktuelle Betriebssystem
	@case "$(HOST_TRIPLE)" in \
		*-apple-darwin) $(MAKE) package-macos ;; \
		*-pc-windows-msvc) $(MAKE) package-windows ;; \
		*) printf 'Kein Packaging für Host %s definiert.\n' "$(HOST_TRIPLE)" >&2; exit 1 ;; \
	esac

package-macos: ## Baut, kombiniert und signiert das universelle macOS-App-Paket
	./scripts/package-macos.sh

package-windows: ## Baut den Windows-x64-Installer inklusive MIDI-SDK-Bridge
	powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/package-windows.ps1

check: fmt-check clippy test build-ui ## Führt die vollständige lokale Qualitätsprüfung aus

ci: check ## Alias für die vollständige CI-Prüfung

cargo-check: ## Prüft alle Rust-Crates ohne vollständigen Build
	$(CARGO_PLATFORM) check --workspace

fmt: ## Formatiert den gesamten Rust-Workspace
	$(CARGO) fmt --all

fmt-check: ## Prüft die Rust-Formatierung
	$(CARGO) fmt --all -- --check

clippy: ## Führt Clippy für Workspace und Tests mit Warnings-as-Errors aus
	$(CARGO_PLATFORM) clippy --workspace --all-targets -- -D warnings

test: ## Führt alle automatisierten Rust-Tests aus
	$(CARGO_PLATFORM) test --workspace

test-soak: ## Führt den zehnminütigen 10k-Events/s-Akzeptanztest aus
	$(CARGO_PLATFORM) test -p virtual-midi-core --test performance routes_ten_thousand_events_per_second_for_ten_minutes -- --ignored --exact

clean: clean-rust clean-ui ## Entfernt alle generierten Rust-, UI- und Sidecar-Artefakte

clean-rust: ## Entfernt Rust-Build-Artefakte
	$(CARGO) clean

clean-ui: ## Entfernt UI-Build, Node-Module und generierte Sidecars
	rm -rf $(DESKTOP_DIR)/dist $(DESKTOP_DIR)/node_modules
	find $(TAURI_DIR)/bin -type f ! -name .gitkeep -delete
