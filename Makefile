# Split FE/BE build entry points. BE = crates/* libs, FE = storix (app/).
# Run `make` or `make help` for the target list.

BE_PKGS = dbm-core dbm-connstore dbm-driver-postgres dbm-driver-mysql dbm-driver-redis dbm-driver-mongo
FE_PKG  = storix
BE_FLAGS = $(addprefix -p ,$(BE_PKGS))

.DEFAULT_GOAL := help

.PHONY: help be-build be-test be-check fe-build fe-run fmt fmt-check lint test test-it build all clean

help: ## List available targets
	@awk 'BEGIN{FS=":.*## "} /^[a-z][a-zA-Z0-9_-]*:.*## /{printf "  %-12s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

be-build: ## Build backend crates only (no FE)
	cargo build $(BE_FLAGS)

be-test: ## Test backend crate unit tests (no FE, no Docker)
	cargo test --lib $(BE_FLAGS)

be-check: ## Type-check backend crates only
	cargo check $(BE_FLAGS)

fe-build: ## Build the storix UI binary
	cargo build -p $(FE_PKG)

fe-run: ## Run the storix UI binary
	cargo run -p $(FE_PKG)

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
