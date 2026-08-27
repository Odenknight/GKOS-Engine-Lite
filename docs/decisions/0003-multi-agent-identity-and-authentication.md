# ADR-0003: Bind stable local agent identity to credentials

## Context

The existing Lite desktop experience shares one Full sidecar bearer token. A multi-agent service needs stable attribution across reconnects, unique sessions and requests, immediate disable/rotation behavior, and safe operational activity records.

Kosmos-Oden commit `a7113c0ca3be8dd230a9549940e2f387d4cb2a96` derives a best-effort label from MCP `clientInfo.name` through a minted session ID, falling back to `User-Agent`. Its own documentation says correctness must not depend on that identity. It therefore does not supply an authenticated stable external subject suitable as GKOS's primary key.

## Decision

GKOS-Engine assigns a local UUIDv7 `agent_id` to each provisioned credential. Each successful MCP initialization receives a new UUIDv7 `session_id`; each operation receives a new UUIDv7 `request_id`. Credentials contain at least 256 random bits, are compared in constant time, and are stored only as a strong digest plus a nonsecret digest-addressed `credential_id`. A new plaintext credential is shown once.

The first-run desktop token migrates to a legacy/bootstrap local identity rather than being silently invalidated. Disable and credential rotation take effect on the next operation, including active sessions.

Identity, external mappings, sessions, and append-only activity are stored in an owner-protected local SQLite service store. Activity contains canonical input/result digests and bounded operational metadata, never credentials, queries, note bodies, snippets, or raw payloads. Operational telemetry is not automatically a GKOS State-Change Receipt.

Kosmos-Oden client information is retained only as bounded metadata until it provides a stable authenticated namespace and subject. If that later exists, an idempotent unique mapping links it to one local `agent_id`; the external raw identifier never becomes the local primary key. Caller-provided identity headers are never trusted without credential binding.

Streamable HTTP remains bearer-authenticated and loopback-only by default. Rate, concurrency, body-size, timeout, and cancellation bounds apply globally and per agent. Discoverability remains the content authorization layer.

## Alternatives rejected

- Use display name, IP address, PID, user agent, or MCP client name as stable identity. These are caller-controlled, unstable, or shared.
- Share one token among all agents. That prevents meaningful attribution and immediate per-agent disable.
- Persist plaintext credentials or raw requests. That creates unnecessary secret and content exposure.
- Reuse an unauthenticated external identifier as the local primary key. External naming does not establish GKOS identity.

## Consequences

Lite must migrate existing desktop state without rewriting source notes and expose provision/list/disable/rotate/activity operations through Full's additive CLI. Existing clients keep a bootstrap migration path. Concurrent identity tests and credential-leak tests become release gates.

## Status

Accepted — 2026-08-20.

## Evidence

- Kosmos-Oden `docs/AGENT-API-CONNECTOR-BUILD.md` at `a7113c0ca3be8dd230a9549940e2f387d4cb2a96`, especially its `clientInfo.name`/session/`User-Agent` precedence and best-effort warning.
- Current Lite client, token, settings, and sidecar boundary captured by `desktop/test/fixtures/phase0-desktop.json`.
- [Phase 0 baseline and reconciliation](../evidence/phase-0-baseline.md).
