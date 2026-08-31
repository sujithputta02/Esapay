# Agent model

ESA uses four bounded agents. **None execute infrastructure mutations.**

```text
Agent ≠ Execution authority
```

Agents emit diagnoses and proposals. The **Action Gateway** is the only execution path.

## Agents

| Agent | Doc |
|-------|-----|
| Monitor | [agents/monitor.md](agents/monitor.md) |
| Diagnosis | [agents/diagnosis.md](agents/diagnosis.md) |
| Planning | [agents/planning.md](agents/planning.md) |
| Safety | [agents/safety.md](agents/safety.md) |

## Orchestration

- Implemented in `crates/esa-runtime/src/orchestrator.rs`.
- Rate limiter on agent calls (10/sec configured in orchestrator).
- Events published to WebSocket (`/ws/telemetry`) for UI.

## LLM usage

- **Diagnosis** uses Ollama when reachable (`OLLAMA_URL`, `OLLAMA_MODEL`).
- Planning is rule/structure-driven from diagnosis (not free-form shell commands).
- On LLM timeout or error → rule-based diagnosis; **no unsafe execution** path.

## What agents cannot do

- Run shell or `kubectl`
- Bypass policy or gateway
- Mutate state without `state_version` on proposal
- Execute `ActionType` not defined in `esa-core`
