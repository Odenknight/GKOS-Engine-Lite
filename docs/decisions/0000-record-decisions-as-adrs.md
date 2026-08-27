# ADR-0000: Record decisions as ADRs

## Context

The GKOS-Engine-Lite functional uplift changes public contracts, cross-repository ownership, derived storage, authentication, packaging, and compatibility behavior. Those choices need durable rationale tied to an exact implementation baseline. A prose plan alone does not show which alternatives were considered or when a decision changed.

## Decision

Record architecture decisions in `docs/decisions/` as numbered Markdown ADRs. Every ADR uses the sections `Context`, `Decision`, `Alternatives rejected`, `Consequences`, `Status`, and `Evidence`.

An accepted ADR is immutable in substance. A later change adds a new ADR that supersedes the old one and links both records. Evidence identifies exact commits, contract versions, qualification results, and owner decisions where applicable. ADRs do not amend GKX or grant governance authority.

The controlling work packet is “GKOS-Engine / GKOS-Engine-Lite Functional Uplift Build Instructions” dated 2026-08-20, together with the same-day provider-neutral clarification. When they conflict, the clarification controls provider routing; the current pinned GKOS Standard controls GKX fields and profiles.

## Alternatives rejected

- Keep decisions only in issues, chat, or pull-request descriptions. Those records are not reliably packaged with the code or tied to the released tree.
- Rewrite an old decision in place. That erases the decision trail.
- Treat an ADR as a GKX schema amendment. Only the authoritative Standard can define GKX.

## Consequences

Every material architectural change adds or supersedes an ADR. Reviewers can audit why Lite remains a consumer of Full and distinguish verified infrastructure from planned adapters. Documentation maintenance is an explicit delivery cost.

## Status

Accepted — 2026-08-20.

## Evidence

- Lite baseline: `2ebbf77583af3e83032054f1256188dc56376907`.
- [Phase 0 baseline and reconciliation](../evidence/phase-0-baseline.md).
- ADR format adopted from the clean-room study of GrooveSeek commit `313514b793d12ea5c3b8eedc32fd213212e38d75`.
