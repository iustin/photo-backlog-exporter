# This exists as this is too small a project for using xtasks.

.PHONY: all

all: coverage

.PHONY: coverage
coverage:
	cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info
	cargo llvm-cov report
