# GKOS-Engine-Lite roadmap

**Ecosystem role:** thin CLI and desktop distribution of
[GKOS-Engine](https://github.com/Odenknight/GKOS-Engine). It does not fork the
engine, define GKX contracts, or introduce independent governance semantics.
Kosmos Research Studio Lite is frozen and is a compatibility companion, not the
forward roadmap for this active distribution.

## Delivered in 1.1 / Desktop 0.2

- Friendly assistance actions backed by engine-validated proposals.
- Loopback-only connections with optional bearer authentication.
- A simple local-AI sharing control with advanced setup hidden by default.
- Reproducible CLI packaging and Windows/macOS installer workflows.

## Next: 1.1.x / Desktop 0.2.x

- In-app assistance panel and one-step sidecar readiness guidance.
- Accessible loading, success, unavailable, and retry states.
- Clean-machine Windows and macOS installer smoke tests.
- Signed checksums and software bills of materials.
- Automated verification that the packaged engine version and compatibility
  fixtures match the declared upstream release.

## Later: 1.2 / Desktop 0.3

- Guided local-model setup with conservative defaults.
- Review queue for accepting, rejecting, or editing suggestions.
- Plain-language evidence and contradiction views.
- Optional per-folder assistance preferences.
- Signed and notarized installers when infrastructure is available.

## Scope gates

- Shared deterministic behavior is implemented in GKOS-Engine first.
- Normative or GKX contract questions are referred to gkos-standard.
- The four deterministic commands always work without AI.
- Notes are never changed automatically; sensitivity is never lowered
  automatically.
- This distribution does not count as an independent GKOS implementation.
