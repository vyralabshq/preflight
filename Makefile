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
	@echo "  Pass flags:  make check ARGS=\"--profile mainnet\""
	@echo "  Everything:  cargo run -- --help"
	@echo

## run every test
test:
	cargo test

## run preflight against this machine
check:
	@cargo run --quiet -- $(ARGS); c=$$?; echo; echo "exit code $$c"; \
	  exit $$c

## print every check and where its requirement comes from
registry:
	@cargo run --quiet -- --dump-registry

## put preflight on your PATH
install:
	cargo install --path .
	@echo
	@echo "now just run:  preflight"

## remove build artefacts
clean:
	cargo clean
