# Security

## Secrets

- **Never commit** Razorpay live keys, webhook secrets, or production credentials.
- Use `.env` locally (not committed). Template: `.env.example`.
- Test Mode keys only for this repository (`rzp_test_…`).

## Razorpay

- Webhook signature verification when `RAZORPAY_WEBHOOK_SECRET` is set.
- Test cards / UPI IDs only in Test Mode checkout (see payment simulator UI).

## Agent execution restrictions

- Agents **cannot** invoke shell, `kubectl`, or arbitrary infrastructure APIs directly.
- All mutations go through **typed `ActionType`** proposals and the **Action Gateway**.
- High/Critical risk actions require approval verdict (`RequiresApproval`).

## Data minimization (demo scope)

- No raw card numbers stored in state fabric or agent prompts.
- Payment simulator uses Razorpay Checkout; ESA ingests aggregated workload metrics and payment events.

## Kubernetes (optional)

- When `KUBERNETES_ENABLED` is true (default unset = enabled in code), gateway may run `kubectl scale` for mapped deployments in `esa-workloads`.
- Requires local cluster access; not used when kubectl/namespace is unavailable (in-memory state still updates).

## Docker

- Compose services expose Postgres, Redis, NATS on localhost — suitable for development only.
- Do not expose Ollama or API without authentication in untrusted networks.

## Reporting

For buildathon / research prototype issues, open an issue in the repository with reproduction steps.
