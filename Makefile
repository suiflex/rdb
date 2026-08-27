# Split FE/BE build entry points. BE = crates/* libs, FE = rdb (app/).
# Run `make` or `make help` for the target list.

BE_PKGS = rdb-core rdb-connstore rdb-driver-postgres rdb-driver-mysql rdb-driver-redis rdb-driver-mongo rdb-driver-sqlite rdb-driver-cassandra rdb-driver-mssql rdb-driver-clickhouse
FE_PKG  = rdb
BE_FLAGS = $(addprefix -p ,$(BE_PKGS))

.DEFAULT_GOAL := help

.PHONY: help be-build be-test be-check fe-build fe-run fe-run-mock fe-build-run fe-show fmt fmt-check lint test test-it build all clean

FE_BIN = target/debug/$(FE_PKG)

help: ## List available targets
	@awk 'BEGIN{FS=":.*## "} /^[a-z][a-zA-Z0-9_-]*:.*## /{printf "  %-12s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

be-build: ## Build backend crates only (no FE)
	cargo build $(BE_FLAGS)

be-test: ## Test backend crate unit tests (no FE, no Docker)
	cargo test --lib $(BE_FLAGS)

be-check: ## Type-check backend crates only
	cargo check $(BE_FLAGS)

fe-build: ## Build the rdb UI binary
	cargo build -p $(FE_PKG)

fe-run: ## Run the rdb UI binary
	cargo run -p $(FE_PKG)

fe-run-mock: ## Run the UI with the seeded design-mock data (RDB_MOCK=1)
	RDB_MOCK=1 cargo run -p $(FE_PKG) --features mock

# Slint's own embedded MCP server, for driving the UI from a test harness.
# SLINT_EMIT_DEBUG_INFO=1 is what keeps element metadata in the compiled UI —
# without it the introspection tools have nothing to find. `slint/mcp` has to be
# passed here rather than declared in app/Cargo.toml; Slint's docs are explicit
# about that. Override the port with `make fe-run-mcp SLINT_MCP_PORT=9001`.
SLINT_MCP_PORT ?= 8080
fe-run-mcp: ## Run the UI with Slint's MCP server on SLINT_MCP_PORT (default 8080)
	SLINT_EMIT_DEBUG_INFO=1 SLINT_MCP_PORT=$(SLINT_MCP_PORT) \
		cargo run -p $(FE_PKG) --features slint/mcp

ui-test: ## Drive the UI through Suitest and publish the run (needs `suitest up`)
	SLINT_EMIT_DEBUG_INFO=1 cargo build -p $(FE_PKG) --features "slint/mcp,mock"
	node scripts/suitest-run.mjs

fe-build-run: ## Build the rdb UI then launch it (GUI shows after build)
	cargo build -p $(FE_PKG)
	./$(FE_BIN)

fe-show: ## Launch the rdb UI; build it first only if missing
	@test -x $(FE_BIN) || cargo build -p $(FE_PKG)
	./$(FE_BIN)

fmt: ## Format the whole workspace
	cargo fmt --all

fmt-check: ## Check formatting without writing
	cargo fmt --all -- --check

lint: ## Clippy across all targets, warnings are errors
	cargo clippy --all-targets -- -D warnings

test: ## Run all unit tests (whole workspace, no Docker)
	cargo test --workspace --lib --bins

test-it: ## Run integration tests (requires a running Docker daemon)
	cargo test --workspace --test '*'

build: ## Build the whole workspace
	cargo build --workspace

all: fmt-check lint test build ## CI-style gate: fmt-check + lint + test + build

clean: ## Remove build artifacts
	cargo clean
