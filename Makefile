.PHONY: check test ui-build

check:
	cargo check --workspace
	cd apps/desktop && npm run build

test:
	cargo test --workspace

ui-build:
	cd apps/desktop && npm run build
