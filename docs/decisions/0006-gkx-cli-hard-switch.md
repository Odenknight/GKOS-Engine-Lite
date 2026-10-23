# ADR-0006: Use gkx-lite without a legacy executable alias

## Context

[ADR-0005](0005-lite-packaging.md) retained `okf-lite` at its August baseline,
with removal requiring separate owner authority. The naming work in
[PR #28](https://github.com/Odenknight/GKOS-Engine-Lite/pull/28) records the
owner's October 19 hard-switch decision and implements that removal.

## Decision

Supersede only ADR-0005's executable-alias policy. The command is `gkx-lite`;
no `okf-lite` alias is installed. Update scripts and integrations explicitly.
The static Rust adapter direction and Full-owned semantic contracts in
ADR-0005 remain in force. Historical fixtures keep their original bytes.

## Alternatives rejected

- Retain an alias: conflicts with the recorded hard-switch decision.
- Rewrite ADR-0005: erases the baseline decision and its conditions.

## Consequences

The command rename is breaking. Follow [GKX-MIGRATION.md](../../GKX-MIGRATION.md).
This decision does not qualify the older Desktop sidecar for renamed note
metadata, update dependency pins, or authorize a release.

## Status

Accepted policy recorded in merged PR #28; reconciliation recorded 2026-10-23.
Supersedes the executable-alias paragraph of ADR-0005 only.

## Evidence

- PR #28, merged as `b177f6427b88dbe76f9544f2715b7a877e7e6f4c`.
- `package.json` exposes `gkx-lite` from `bin/gkx-lite.mjs` at that commit.
