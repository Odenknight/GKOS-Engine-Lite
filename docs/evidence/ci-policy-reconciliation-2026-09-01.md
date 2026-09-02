# CI policy reconciliation evidence — 2026-09-01

## Scope and lineage

This evidence accompanies a CI-only successor to the exact recovered review
lineage. The preserved chain is:

1. `f5b06b4a4ea5020d62824ceffc51622269a52f19` — product, documentation,
   and tests aligning quick connect with the pinned REST sidecar;
2. `1fabff81caf2286f24d4cab45188eeb6a59d21ad` — exact recovered Windows
   qualification workflow, tree
   `e2a664036009675b270fa293302eddb0a4504518`;
3. `938a3ebc6b6f1001c7b70320bed803a24e71b606` — documentation-only
   qualification roadmap successor.

The reconstruction commit `634e483632c0e8c0145dbd40972487662dceab9b`
is not part of this lineage. It replaced the exact Windows `npm test` step with
a narrower direct packaged-runtime invocation. This successor retains the exact
recovered full Windows desktop test step, including the packaged REST boundary.

## Official action-tag resolution

The v5 action references were resolved directly from the official GitHub
repositories on 2026-09-01:

```text
git ls-remote --tags https://github.com/actions/checkout.git "refs/tags/v5*"
fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09  refs/tags/v5
fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09  refs/tags/v5.1.0

git ls-remote --tags https://github.com/actions/setup-node.git "refs/tags/v5*"
a0853c24544627f65ddf259abe73b1d18a591444  refs/tags/v5
a0853c24544627f65ddf259abe73b1d18a591444  refs/tags/v5.0.0
```

Every repository use of `actions/checkout` is pinned to the exact checkout
v5.1.0 commit. Every use of `actions/setup-node` is pinned to the exact
setup-node v5.0.0 commit. Both v5 releases use the Node 24 action runtime.

Official sources:

- <https://github.com/actions/checkout/releases/tag/v5.1.0>
- <https://github.com/actions/setup-node/releases/tag/v5.0.0>

## Effective policy

- Linux Node 22 and Node 24 are blocking matrix legs.
- Linux Node 23 repeats the root lockfile, metadata, install, test, and package
  smoke coverage in a separate job with `continue-on-error: true`.
- Windows desktop-native coverage is a blocking Node 22/24 matrix. Each leg
  verifies the digest-pinned sidecar and viewer, runs the full desktop `npm test`
  suite (including the Windows-only packaged REST test), and runs native Tauri
  tests.
- Existing Linux desktop and Linux/Windows retrieval-Rust jobs remain blocking.
- This change does not qualify installer launch, GUI end-to-end behavior,
  signing, notarization, or deployment.

## Local verification

The successor was verified on Windows with Node 24.18.0/npm 10.9.4, Rust
1.98.0 MSVC, and the installed Rust 1.85.0 GNU compatibility toolchain:

- workflow YAML parse and policy assertions: PASS for all three workflows;
- lockfile SHA guard and metadata consistency: PASS;
- root `npm test`: PASS, 38 tests;
- `npm pack --dry-run`: PASS;
- desktop typecheck and production build: PASS;
- desktop `npm test`: PASS, 17 tests, including the live Windows packaged REST
  boundary against the digest-pinned sidecar;
- desktop native `cargo test --locked`: PASS, 2 tests;
- Rust 1.98 MSVC format, check, all-target tests, doctests, and clippy with
  warnings denied: PASS (155 unit tests, 11 Full-contract tests, 3 doctests);
- Rust 1.85 GNU all-target tests and doctests: PASS with the same 155 + 11 + 3
  test counts.

The compile assets used by the packaged desktop test matched the workflow's
published SHA-256 values:

- sidecar: `29ab43c9ce79b8c14978594a18523a04e3f7518d87560f1e4c17744da08f12c2`;
- viewer: `11e004a5e500f5bb196ec321eefd0cd7f9dd332715edc4b1b71f286a10e768dd`.

Node 22, Node 23, Linux, and hosted matrix execution remain pending until this
unpublished successor is reviewed and explicitly authorized for push. Local
success is not presented as hosted qualification.
