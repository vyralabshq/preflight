.PHONY: help test check registry install clean

## show this list
help:
	@echo
	@echo "  preflight — read-only checks for Solana validator hosts"
	@echo
	@grep -E '^## |^[a-z-]+:' Makefile \
	  | sed -e 's/^## /@/' -e 's/:.*//' \
	  | awk '/^@/{d=substr($$0,2); next} {printf "    make %-10s %s\n", $$0, d}'
	@echo
	@echo "  Anything not listed:  cargo run -- --help"
	@echo

## run every test
test:
	cargo test

## run preflight against this machine
# preflight signals findings through its exit code, so a non-zero result is the
# tool working. Only an internal error (3) is a build failure.
check:
	@cargo run --quiet --; c=$$?; [ $$c -eq 3 ] && exit 3 || exit 0

## print every check and where its requirement comes from
registry:
	@cargo run --quiet -- --dump-registry

## build a release binary into target/release/preflight
install:
	cargo build --release
	@echo "binary: target/release/preflight"

## remove build artefacts
clean:
	cargo clean
