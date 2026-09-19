.PHONY: demo benchmark benchmark-smoke benchmark-quick adversarial test audit-verify rbench rbench-locked rbench-smoke rbench-ablations

demo:
	./scripts/demo.sh

adversarial:
	cargo run --release --bin esa-benchmark -- --adversarial

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

rbench:
	PYTHONPATH=. python3 -m arc_reasoner.esa_rbench.runner --locked

rbench-locked:
	PYTHONPATH=. python3 -m arc_reasoner.esa_rbench.runner --locked

rbench-smoke:
	PYTHONPATH=. python3 -m arc_reasoner.esa_rbench.runner --smoke

rbench-ablations:
	PYTHONPATH=. python3 -m arc_reasoner.esa_rbench.runner --ablations

