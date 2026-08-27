# GKOS-Engine-Lite Rust retrieval core

This workspace is the owner-ratified path toward a single statically linked
GKOS-Engine-Lite binary. It is separate from the existing Tauri presentation
shell and does not change the published legacy JavaScript compatibility
wrapper in Phase 1.

The Full repository owns the reference retrieval contract. Lite consumes a
byte-pinned copy under `contracts/gkos-retrieval-1.0.0-draft.1/` and implements
that contract in Rust. This crate is never a second GKX authority: callers must
supply canonical, policy-evaluated `RetrievalSource` envelopes. The crate does
not parse frontmatter or assign identity, lineage, temporal validity,
sensitivity, governance standing, or discoverability.

Phase 1 provides:

- deterministic heading chunks with exact UTF-8 byte and line coordinates;
- RFC 8785/JCS-compatible digests and UTF-16 code-unit tie-breaking;
- bundled SQLite with a runtime `ENABLE_FTS5` assertion;
- typed filters, RRF, MMR, confidence, and bounded parent expansion;
- immutable verified generations, atomic active pointers, and corruption
  quarantine without source-note writes;
- embedding reuse only from a verified active generation with the exact
  provider/model/dimensions/content-digest tuple;
- provider-neutral traits and nonsecret configuration identities for
  `openai_compatible`, `local_onnx`, and `mcp`;
- provider calls bounded by the trusted timeout (15 seconds by default), with
  timeout degradation and drop-cancellation at the adapter future boundary;
- mandatory FTS-only operation, with no silent provider or embedding-space
  fallback.

The minimum supported Rust compiler is 1.85 because the pinned JCS dependency
declares that MSRV. CI tests both 1.85.0 and the reviewed 1.98.0 toolchain.
The bundled SQLite path is a build strategy for the future static frontend; it
is not yet a qualified release artifact or a claim that every final binary
dependency is static.

Run locally where a native linker is installed:

```text
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
cargo test --manifest-path rust/Cargo.toml --workspace --all-targets --locked
cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
```
