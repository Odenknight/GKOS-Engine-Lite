# GKOS retrieval draft 1

This pack freezes the Phase 1 cross-language integration surface at
`gkos-retrieval/1.0.0-draft.1`. It is a draft integration contract, not a GKOS
Standard conformance claim and not a new source authority.

`source_id` is the valid canonical authored GKX `uid`. `lineage_id` remains
`null` unless a canonical profile or projection supplies it; implementations
must never synthesize lineage from paths, rank, or time proximity.

Run the repository retrieval tests to verify exact chunk bytes, identities,
RRF arithmetic, duplicate collapse, and MMR ordering against
`conformance-fixture.json`. The same fixture freezes the accent-insensitive
lexical signal and every indexed field weight, including topic and category.

Projection schema 2 binds the actual lexical backend into the manifest,
projection digest, and projection identity. Implementations probe the bundled
SQLite module by creating and dropping a real temporary FTS5 virtual table;
they never infer capability from a Node/runtime version. `sqlite_fts5` remains
the preferred backend when that probe succeeds. When it does not,
`sqlite_lexical_scan` uses an ordinary SQLite projection and joins only the
policy-eligible temporary ID set before any row reaches deterministic
in-process matching or scoring. The scan is reported as degraded with
`SQLITE_LEXICAL_SCAN_ACTIVE`, `SQLITE_LEXICAL_SCAN_APPROXIMATION`, and, when
applicable, `SQLITE_FTS5_UNAVAILABLE`; it is never reported as FTS5. The
versioned `gkos-unicode61-subset-scan/1` grammar uses the host runtime's Unicode
property tables for letters, numbers, and private-use scalars (underscore,
hyphen, and other punctuation remain separators), folds diacritics only for
Latin-base clusters, folds Greek final sigma to sigma, preserves non-Latin
canonical-form distinctions, and treats punctuation as separators. Each
normalized clause must occur as one consecutive token sequence within one indexed field;
separate clauses may match separate fields. The conformance fixture pins the
stage envelopes and the differential query set. C0/DEL controls (including
NUL), unmatched or embedded quotes, empty phrases, and punctuation-only clauses are rejected before corpus or
store access so FTS5 and scan parsing cannot diverge.

The scan does not claim exhaustive unicode61/Unicode-6.1 equivalence. Runtime
Unicode tables and SQLite's historical unicode61/remove-diacritics behavior can
differ outside the frozen differential corpus; that broader parity remains
unqualified and is why the compatibility stage remains degraded.

Unquoted lexical scoring and citation terms use the same letter, number, and
private-use token boundaries, with underscore, hyphen, and other punctuation
as separators. Greek final sigma folds to ordinary sigma for candidate
matching, scoring, and citation lookup. A quoted phrase returns the exact
verified phrase span when those bytes exist; when punctuation tokenization made
the candidate match but the raw phrase bytes do not exist, only exact
component-token spans are returned. A rank-one candidate with no positive
lexical, semantic, or reranker signal is low confidence with
`WEAK_LEXICAL_MATCH`; rank alone never creates high confidence.

Portable path and canonical JSON edge cases are pinned in
`canonical-fixture.json`. Negative cosine similarity contributes zero to the
MMR redundancy penalty. When reranking is active, MMR relevance is reciprocal
reranker rank, preserving the rerank-then-diversify stage order.

Trusted host configuration uses the dependency-light `gkos-toml-subset/1`
profile pinned by `gkos-toml-lexical-fixture.json`. The accepted grammar is
limited to simple section headings and bare keys, TOML-valid one-line basic or
literal strings, lowercase booleans, plain decimal integers/floats, and dense
string arrays. Numeric underscores/exponents/special floats, leading-zero
integers, JSON-only escapes, invalid Unicode scalar escapes, trailing array
commas, dotted keys, repeated sections, inline tables, and multiline forms are rejected before
provider selection. This lexical subset does not impose any endpoint, vendor,
route, model, local-runtime, or upstream-MCP preference.

Canonical JSON accepts only the interoperable JSON data model: dense arrays
and plain enumerable data objects. Undefined values, sparse arrays, accessors,
symbol keys, cycles, exotic prototypes, unsafe integer-valued numbers, and
unpaired UTF-16 surrogates are rejected; valid Unicode pairs are preserved
without normalization.

Overlapping chunks are de-duplicated after final ranking. A matched source span
may be claimed only once by exact `(source_id,start_byte,end_byte)` identity;
later hits keep only still-unclaimed spans and disappear if none remain. A hit
with no matched spans is instead suppressed when its half-open source interval
overlaps an already accepted interval from the same source. Adjacent intervals
do not overlap, and skipped hits claim nothing.

Typed path globs are matched with memoized Unicode-code-point semantics: `?`
matches one non-slash Unicode scalar, `*` matches zero or more non-slash
scalars, and `**` may cross slash boundaries. Matching always consumes the
entire normalized path, avoiding regex backtracking and UTF-16 surrogate drift.

Citation matching uses the same NFD, combining-mark removal, and lowercase
normalization as lexical scoring, but maps each normalized unit back to its
complete original UTF-8 base-plus-combining-mark cluster. Thus a `cafe` match
cites the exact original `Café` or decomposed `Café` bytes; normalized text is
never returned as a quotation. `citation_normalization` pins these mappings.
Live verification also recomputes absolute one-based line coordinates from the
exact source bytes using the chunker's LF/CRLF rules; persisted line numbers
are never trusted on their own.

Phase 1 stores relationship and author-agent metadata for deterministic
projection binding but suppresses it from results. The `moc_relationships` and
`author_agent_ids` filters are rejected until later authorized endpoint
resolvers can prevent guessed-identifier existence oracles.

Every configured or injected provider call is bounded by the coordinator, not
only by a concrete adapter: index embeddings, query embeddings, and reranking
use the trusted effective timeout (`15000` ms by default; inclusive range
`1..300000`) and receive an abort signal. An operational vector timeout degrades
the request or generation to reported lexical-only behavior; an optional reranker
timeout skips only that reported stage. Provider/model/dimension mismatches are
controlled-rebuild errors and never degrade into a mixed embedding space. These
resource bounds do not restrict the operator's endpoint, vendor, route, model,
local runtime, or upstream MCP tool choice.
