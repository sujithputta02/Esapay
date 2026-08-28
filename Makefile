.PHONY: demo benchmark benchmark-smoke benchmark-quick test audit-verify

demo:
	./scripts/demo.sh

benchmark:
	cargo run --bin esa-benchmark

benchmark-smoke:
	cargo run --bin esa-benchmark -- --smoke-full

benchmark-quick:
	cargo run --bin esa-benchmark -- --quick

test:
	cargo test --workspace

audit-verify:
	cargo test --test tamper_detection_test -- --nocapture
