# GKOS-Engine-Lite Roadmap

Engine-Lite keeps ordinary operation simple: choose a notes folder, choose a
privacy default, and optionally connect a local AI app. DSPy and protocol
details remain hidden from normal users.

## Delivered in 1.1 / Desktop 0.2

- Friendly assistance actions: explain, improve, repair, find links, find
  claims, check conflicts, and check privacy.
- Engine-validated, proposal-only intelligence responses.
- Loopback-only connections with optional bearer authentication.
- A single **Share with local AI apps** control.
- Advanced ports, tokens, endpoints, and raw setup hidden by default.
- Reproducible CLI packaging and Windows/macOS desktop installer workflows.

## Next: 1.1.x / Desktop 0.2.x

- Add an in-app assistance panel so users do not need the command line.
- Detect sidecar readiness and show one-step setup guidance.
- Add accessible loading, success, unavailable, and retry states.
- Add installer smoke tests on clean Windows and macOS virtual machines.
- Add signed checksums and software-bill-of-materials files to releases.

## Later: 1.2 / Desktop 0.3

- Guided local-model setup with conservative defaults.
- Review queue for accepting, rejecting, or editing suggestions.
- Plain-language evidence and contradiction views.
- Optional per-folder assistance preferences.
- Signed and notarized desktop installers when signing infrastructure is
  available.

## Product guardrails

- The four deterministic commands always work without AI.
- Notes are never changed automatically.
- Suggestions never become approved facts by themselves.
- Sensitivity can be raised automatically, never lowered.
- Advanced implementation terminology stays out of the everyday interface.
