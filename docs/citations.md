# Citation and temporal provenance

Phase 2 is pin-bound to the signed, hosted-green Full implementation. GKOS-Engine owns the canonical GKX
lineage, temporal, and provenance projection. GKOS-Engine-Lite consumes the
versioned Full candidate envelopes and parser-owned resolver receipts; it
never parses GKX, assigns a source identity, reparses an authored reference,
invents a lineage ID, or treats retrieval rank as authority.

## Exact citation coordinates

Each result citation binds the stable source ID and normalized relative path,
the source and content SHA-256 digests, a zero-based half-open UTF-8 byte range
`[start_byte, end_byte)`, and one-based inclusive line coordinates. The cited
text must equal the exact live source-byte slice. Line coordinates are
recomputed from those live bytes, including LF, CRLF, and multibyte Unicode
cases. A stale digest or inconsistent byte, line, path, span, parent, or source
binding cannot be marked verified.

Read-only indexing and search do not rewrite source notes. Derived SQLite state
is disposable and cannot become GKX authority.

## Point-in-time selection

The additive syntax is:

```text
gkx search <query> --kb-path <dir> --as-of <GKX timestamp>
```

The accepted value uses the current GKX timestamp grammar: a calendar date and
time separated by `T`, with an explicit `Z` or numeric offset. Seconds are
optional; fractional seconds are permitted only when seconds are present,
according to that grammar. Full normalizes an accepted value to its UTC ISO
representation in the result. Lite preserves the raw flag and value at its
JavaScript delegation boundary; it does not normalize or reinterpret them
there.

Point-in-time eligibility uses the half-open canonical interval
`valid_from <= as_of < valid_to`, with a missing `valid_to` representing an
open end. The runtime policy digest is bound first, followed by source-level
discoverability policy, typed filters, whole-source chunk discoverability
policy, and then temporal eligibility. Temporal eligibility runs before
candidate generation, provider calls, ranking, counts, confidence, citation
assembly, or parent expansion. Unknown validity is not treated as all-time.
When an authorized corpus exists but its history cannot cover the requested
instant, the result is empty and reports `TEMPORAL_COVERAGE_INSUFFICIENT`; it
does not fall back to current content.

## Provenance and ledger status

Returned Phase 2 hits and content-bearing parents carry a digest-sealed,
authorization-scoped provenance view. It reports canonical source and
assertion bindings, the scoped validity interval and temporal state, and only
resolved relationship endpoints that are themselves authorized and non-future
at the requested time. An eligible successor may therefore expose its
authorized historical predecessor, while a historical predecessor suppresses
a future successor endpoint. Raw authored references remain internal. A
governed unresolved or ambiguous receipt returns only
`RETRIEVAL_AUTHORIZED_VIEW_CONFLICT` before query work; ordinary broken links
remain non-conflicting. No component identifier is synthesized.

`ledger_binding_verified` is `false` in the dependency-light Lite path, and no
ledger hash is emitted. A retrieval projection, vector score, timestamp, or
similarity result cannot create durable ledger authority.

## Ratified authorization boundary

The draft.2 pack and its Lite mirror are bound byte-for-byte to Full commit
`6e2df27d33ede62ee0d2e3cb7610df478a7d66ce`. Owner-ratified Decision A evaluates cross-record identity,
endpoint resolution, declaration reconciliation, and topology/order only over
the source-policy, typed-filter, whole-source-chunk-policy, and point-in-time
authorized candidate view. Hidden or future candidates are indistinguishable
from physical absence in the complete ordinary result. A conflict among
authorized known-created candidates fails before live reads, query providers,
lexical/vector SQL, ranking, counts, confidence, citations, or parent work with
the single non-content-bearing code `RETRIEVAL_AUTHORIZED_VIEW_CONFLICT`.
Unknown candidates contribute only the insufficient-coverage bit; future
candidates and endpoints are suppressed. Lite applies these frozen rules only
to host-supplied opaque candidate keys and canonical receipt tiers, never to
raw GKX text or a second resolver.

The original Phase 2 JavaScript wrapper was pinned to the Phase 2 Full commit.
The current wrapper pins the later Phase 3 Full revision recorded in
[versioning](../VERSIONING.md); historical parity evidence is revision-bound. Its
point-in-time command test executes an identical corpus/configuration through
both command boundaries and compares exit status, stdout, and stderr
byte-for-byte, apart from the already-approved PID normalization on the exact
Node SQLite experimental-warning line. Phase 2 hosted evidence is recorded in
[the Phase 2 dossier](evidence/phase-2-lineage-citations.md). Current hosted
results are recorded in [current capabilities](CURRENT_CAPABILITIES.md).
Neither establishes equivalence to another Full revision or qualifies a
static executable, installer, deployment, or published package.
