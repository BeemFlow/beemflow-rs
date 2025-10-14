# BeemFlow Rust - Makefile
# Build, test, and development automation for BeemFlow

BINARY := flow
RELEASE_BINARY := target/release/$(BINARY)

# Auto-discover flow files
INTEGRATION_FLOWS := $(shell find flows/integration -name "*.flow.yaml" 2>/dev/null)
E2E_FLOWS := $(shell find flows/e2e -name "*.flow.yaml" 2>/dev/null)
EXAMPLE_FLOWS := $(shell find flows/examples -name "*.flow.yaml" 2>/dev/null)

.PHONY: all clean build build-static install test test-verbose coverage e2e integration examples test-all check fmt lint fix release

all: clean test build install

clean:
	cargo clean
	rm -f test_all_flows.sh test_registry test_registry.rs flows/test_fetch.flow.yaml

build:
	cargo build --release

build-static:
	cargo build --release --target x86_64-unknown-linux-musl

install: build
	cargo install --path .

serve:
	cargo run --release -- serve

# ────────────────────────────────────────────────────────────────────────────
# Tests
# ────────────────────────────────────────────────────────────────────────────

test:
	cargo test

test-verbose:
	cargo test -- --nocapture

test-race:
	cargo test -- --test-threads=1

coverage:
	cargo tarpaulin --out Html --output-dir coverage

# ────────────────────────────────────────────────────────────────────────────
# Flow execution tests (auto-discovers all flows)
# ────────────────────────────────────────────────────────────────────────────

examples:
	@echo "📖 Example flows (reference only, may require additional setup):"
	@for flow in $(EXAMPLE_FLOWS); do \
		echo "  - $$flow"; \
	done

e2e:
	@echo "🧪 Running end-to-end tests (via CLI)..."
	@echo "Building release binary first..."
	@cargo build --release
	@echo "These flows are functional and should run with proper .env configuration"
	@echo ""
	@mkdir -p /tmp/beemflow-e2e
	@for flow in $(E2E_FLOWS); do \
		timestamp=$$(date +%s); \
		echo "▶ Running $$flow"; \
		$(RELEASE_BINARY) run --event "{\"timestamp\":\"$$timestamp\"}" $$flow || echo "  ❌ Flow failed"; \
		echo ""; \
	done
	@echo "✅ E2E tests complete!"

integration:
	@echo "🧪 Running integration tests..."
	cargo test --test integration_test
	cargo test --test flows_integration_test
	@for flow in $(INTEGRATION_FLOWS); do \
		echo "Running $$flow"; \
		cargo run --release -- run $$flow || echo "Flow $$flow failed, continuing..."; \
	done

# Full test suite (unit + integration + e2e CLI tests)
test-all: test integration e2e

# ────────────────────────────────────────────────────────────────────────────
# Code quality
# ────────────────────────────────────────────────────────────────────────────

# Run all checks (matches CI pipeline)
check:
	@echo "🔍 Running all quality checks..."
	@echo ""
	@echo "📋 Step 1/4: Checking formatting..."
	@cargo fmt -- --check
	@echo "✅ Formatting OK"
	@echo ""
	@echo "📋 Step 2/4: Running clippy..."
	@cargo clippy --all-targets --all-features -- -D warnings
	@echo "✅ Clippy OK"
	@echo ""
	@echo "📋 Step 3/4: Running unit tests..."
	@cargo test --lib --quiet
	@echo "✅ Unit tests OK"
	@echo ""
	@echo "📋 Step 4/4: Running integration tests..."
	@cargo test --test integration_test --quiet
	@cargo test --test flows_integration_test --quiet
	@echo "✅ Integration tests OK"
	@echo ""
	@echo "🎉 All checks passed! Ready to commit."

# Quick check (formatting + clippy only, no tests)
check-quick:
	@echo "⚡ Running quick checks (no tests)..."
	@cargo fmt -- --check
	@cargo clippy --all-targets --all-features -- -D warnings
	@echo "✅ Quick checks passed!"

fmt:
	cargo fmt

fmt-check:
	cargo fmt -- --check

lint:
	cargo clippy --all-targets --all-features -- -D warnings

# Auto-fix all issues (format + clippy --fix)
fix:
	@echo "🔧 Auto-fixing all issues..."
	cargo fix --allow-dirty --allow-staged
	cargo clippy --fix --allow-dirty --allow-staged
	cargo fmt
	@echo "✅ Auto-fix complete!"

# ────────────────────────────────────────────────────────────────────────────
# Release
# ────────────────────────────────────────────────────────────────────────────

release:
	@if [ -z "$(TAG)" ]; then echo "Usage: make release TAG=v0.2.1"; exit 1; fi
	@echo "Creating and pushing tag $(TAG)..."
	git tag $(TAG)
	git push origin $(TAG)
	@echo "✅ Tag $(TAG) pushed! Check GitHub Actions for release progress."

# ────────────────────────────────────────────────────────────────────────────
# Development helpers
# ────────────────────────────────────────────────────────────────────────────

# Run a specific flow
run:
	@if [ -z "$(FLOW)" ]; then echo "Usage: make run FLOW=path/to/flow.yaml"; exit 1; fi
	cargo run --release -- run $(FLOW)

# Run with debug logging
debug:
	RUST_LOG=debug cargo run --release -- run $(FLOW)

# List all available tools
tools:
	@echo "Registered tools:"
	@cat src/registry/default.json | jq -r '.[] | select(.type == "tool") | "  - " + .name' | head -20
	@echo "  ..."

# Show test results summary
test-summary:
	@./test_all_flows.sh 2>/dev/null || echo "Run 'make build' first"

# Watch mode for development
watch:
	cargo watch -x 'build' -x 'test'

# Generate documentation
docs:
	cargo doc --no-deps --open

# Benchmark
bench:
	cargo bench

# Security audit
audit:
	cargo audit

