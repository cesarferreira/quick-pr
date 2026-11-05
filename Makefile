.PHONY: build run test check package install clean
build:
	cargo build
run:
	cargo run
test:
	cargo test
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
