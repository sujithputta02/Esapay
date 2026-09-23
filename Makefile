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

# 24/7 Live Dockerized Ollama targets
ollama-up:
	docker-compose up -d ollama
	@echo "⏳ Waiting for Ollama model warm-up..."
	@until curl -s http://localhost:11434/api/tags > /dev/null 2>&1; do sleep 1; done
	@echo "✅ Ollama is live 24/7 with models loaded into memory!"

ollama-status:
	@echo "=== Ollama Container Status ==="
	@docker ps --filter "name=esa-ollama" --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"
	@echo "\n=== Loaded Models (Ollama API) ==="
	@curl -s http://localhost:11434/api/tags | python3 -m json.tool 2>/dev/null || curl -s http://localhost:11434/api/tags || echo "Ollama not reachable"

ollama-logs:
	docker logs -f esa-ollama

ollama-down:
	docker-compose stop ollama

# ESA CLI & SDK Targets
cli-build:
	cargo build --bin esa
	@echo "✅ Built ESA CLI binary at target/debug/esa"

cli-install:
	cargo install --path crates/esa-cli --bin esa --force
	@echo "🎉 Installed 'esa' command to ~/.cargo/bin/esa! Run 'esa --help' to get started."

sdk-build:
	cd sdk/typescript && bun x tsc
	@echo "✅ Built ESA TypeScript SDK in sdk/typescript/dist"

