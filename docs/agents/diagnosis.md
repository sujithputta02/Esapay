# Diagnosis Agent

## Purpose

Analyze monitor conditions and produce a root-cause hypothesis with recommended action class.

## Inputs

- `Vec<Condition>` from Monitor
- Ollama client (optional)

## Outputs

- `Diagnosis`: hypothesis, `RootCause`, confidence, evidence_refs, optional `recommended_action` (`CREATE_REPLICA` | `SHIFT_ROUTE`)

## Responsibilities

- Build diagnosis prompt from conditions
- Call Ollama when available
- Parse JSON response or fall back to rules

## Can do

- Classify: hot partition, capacity, traffic spike, node degradation (rule/LLM)
- Recommend action **class** for Planning

## Cannot do

- Execute actions or call gateway
- Access raw payment card data (conditions are aggregated metrics)

## Failure behavior

| Failure | Response |
|---------|----------|
| Ollama unreachable | Rule-based `diagnose()` |
| Invalid LLM JSON | Rule-based fallback |
| Empty conditions | "No conditions" diagnosis |

## Latency

- **~1–2 s** with Ollama (dominates agent cycle in full mode)
- **&lt; 5 ms** on rule-only fallback

## Example (rule fallback)

```text
Conditions: HIGH_LATENCY + QUEUE_BACKLOG on payment-upi-india-south
→ RootCause: HOT_PARTITION
→ recommended_action: CREATE_REPLICA
```
