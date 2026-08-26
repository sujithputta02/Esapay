.PHONY: benchmark benchmark-quick test

benchmark-smoke:
	cargo run --release --bin esa-benchmark -- --smoke-full

benchmark:
	cargo run --release --bin esa-benchmark

benchmark-quick:
	cargo run --release --bin esa-benchmark -- --quick

test:
	cargo test --all
