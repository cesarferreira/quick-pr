LEVEL ?= minor
GIT_CLIFF_VERSION ?= 2.14.2
CARGO_RELEASE_VERSION ?= 1.1.2

.PHONY: build run test check package install clean
build:
	cargo build
run:
	cargo run
test:
	cargo test --locked
	python3 scripts/test_release.py
check:
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings
package:
	./scripts/package.sh
install: package
	-pkill -x quick-pr
	@mkdir -p "$(HOME)/Applications"
	ditto "dist/Quick PR.app" "$(HOME)/Applications/Quick PR.app"
	open "$(HOME)/Applications/Quick PR.app"
clean:
	cargo clean -p quick-pr
	rm -rf dist

.PHONY: release release-dry-run ensure-release-tools
release: ensure-release-tools
	./scripts/release.sh "$(LEVEL)" --execute

release-dry-run: ensure-release-tools
	./scripts/release.sh "$(LEVEL)"

ensure-release-tools:
	@command -v cargo-release >/dev/null 2>&1 || cargo install cargo-release --version $(CARGO_RELEASE_VERSION) --locked
	@installed="$$(git-cliff --version 2>/dev/null | awk '{print $$2}')"; \
	if [ "$$installed" != "$(GIT_CLIFF_VERSION)" ]; then \
		cargo install git-cliff --version $(GIT_CLIFF_VERSION) --locked --force; \
	fi
