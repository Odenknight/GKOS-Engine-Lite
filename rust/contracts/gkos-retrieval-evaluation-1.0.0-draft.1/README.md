# GKOS retrieval evaluation draft 1

This provisional additive pack defines the deterministic, cross-language
portion of Phase 4 retrieval evaluation. It does not freeze final hashes and it
does not add a search executor, command-line implementation, output publisher,
workflow, or performance lane. All Phase 0–3 contracts remain byte-for-byte
unchanged.

GKOS-Engine Full is the sole authority for parsing human `eval/golden.toml`,
opening governed retrieval state, executing public searches, evaluating or
tuning configurations, and publishing candidate output. The Lite adaptation
may only perform crate-private, pure verification and independent recomputation
from the normalized JSON fixtures. Neither runtime may treat this pack as GKX,
YAML, TOML, policy, provider, or corpus authority.

JSON Schema validation is a mandatory structural gate, but it is not the whole
acceptance contract. Every accepted envelope must also pass the runtime's pure
semantic sealer, and Lite must reproduce that seal. Portable JSON Schema counts
Unicode scalar values, so it cannot express the frozen 1..512 UTF-16-code-unit
provider/model identity bound or UTF-8 byte bounds such as the 1024-byte source
path ceiling. The semantic seal enforces those exact units, well-formed Unicode,
ECMAScript-trim non-emptiness, no C0/DEL identity controls, and every frozen
UTF-8 byte cap. The conformance matrices deliberately mark the 257-astral
identity and 1025-byte multibyte source path as structurally schema-valid but
semantically invalid. Schema acceptance alone never authorizes an envelope.

## Human golden subset

The only accepted top-level assignment is the exact contract version followed
by one through 256 `[[query]]` array-table entries. Each entry has the exact
keys shown in `golden-fixture.toml`; `as_of` is the only optional key. Unknown
or duplicate keys/tables and duplicate array members are rejected. Values are
single-line TOML basic strings, single-line literal strings, one-line string
arrays without trailing commas, or canonical non-negative decimal integers.
Comments begin with `#` outside strings. UTF-8 input is at most 1 MiB, has no
BOM, accepts LF or CRLF, rejects bare CR, and preserves string semantics without
Unicode normalization. Basic strings accept only `b`, `t`, `n`, `f`, `r`,
quote, backslash, `u` and `U` escapes; decoded controls and ill-formed Unicode
are rejected. Authored query text is 1..4096 UTF-8 bytes, is never normalized or
trimmed in the normalized golden row, and remains part of that row's
`query_digest` exactly as authored.

Execution has a separate, derived `effective_query_text`: the existing public
coordinator's exact ECMAScript edge trim removes SP, NBSP, U+1680,
U+2000–U+200A, U+2028, U+2029, U+202F, U+205F, U+3000, and U+FEFF at the two
edges. C0 TAB/VT/FF/LF/CR are also ECMAScript trim characters but cannot occur
in a normalized authored query. U+0085, U+180E, and U+200B are not trimmed.
An all-trim query is invalid. Provider input/request digests and the public
result `query_digest` use only `effective_query_text` (plus `as_of` for the
public-result digest); they never replace the authored text or golden digest.
Before fixture, provider, state, or output work, normalization reuses the exact
public `lexicalQueryClauses(authored_text)` grammar: a query must produce at
least one clause, may produce at most 64 clauses, and each clause is at most 256
UTF-8 bytes. Failures are exactly `GKX_EVAL_QUERY_LEXICAL_INVALID`,
`GKX_EVAL_QUERY_LEXICAL_CLAUSE_COUNT_INVALID`, or
`GKX_EVAL_QUERY_LEXICAL_CLAUSE_SIZE_INVALID`; Slice B maps them to fixture/usage
exit 2. This semantic-only check is not a second search or ranking authority.

Normalized queries are sorted by ID; set-like arrays are sorted by ECMAScript
UTF-16 code-unit order. The semantic digest excludes raw TOML bytes, whitespace,
comments, newline spelling, and any filesystem path. `expected_source_ids` and
`forbidden_source_ids` are canonical authored GKX UIDs. Under the pinned current
profile both lineage arrays are required and empty because public
`lineage_id` is null. Source and lineage identifiers are never aliased or used
as fallbacks for one another. A future nonempty lineage expectation requires a
new explicitly versioned profile and contract.

## Relevance and deterministic metrics

Expected paths and expected source IDs resolve independently and exactly once
against a one-to-one source-observation catalog, then form a deduplicated binary
relevance union of source IDs. That union must be nonempty and disjoint from the
complete forbidden audit oracle. Each returned source is counted once at its
first physical hit position; duplicate hits are not removed from rank positions
and do not backfill the top-k window. Recall@k, MRR, and nDCG@k use integer
millionths with half-up rounding. Macro aggregates are the half-up mean of the
already-rounded per-query millionths.

Binary DCG uses the checked-in 100-row table. Its values are Decimal precision
80, ROUND_HALF_UP results for
`1_000_000_000_000 * ln(2) / ln(rank + 1)`. Runtime code never evaluates a
floating-point logarithm. Rank 1 is 1000000000000, rank 2 is 630929753571, and
rank 3 is 500000000000; all values through rank 100 are positive and strictly
decreasing. The evaluation coordinate binds the complete table envelope digest.

The audit oracle coherently classifies every observed source ID/path pair as
authorized or forbidden sets. Unknown-to-oracle values invalidate the oracle;
they are not implicitly treated as forbidden. Policy leakage counts exact
forbidden identity/path occurrences across every present, non-null audited
scalar in hit chunks, child citations, public provenance endpoints, parent
context, and the paired projection coordinate. Canonical null `lineage_id` is
absence and contributes to neither numerator nor denominator; a missing
optional field is equivalent. Malformed non-null audited values invalidate the
envelope rather than becoming metric occurrences. Text coincidence is not
identity leakage. The policy rate is leak occurrences divided by inspected
non-null identity fields in millionths.

Every child and parent citation is rebound to the sealed source observation,
source digest, byte and line coordinates, text/content digest, chunk identity,
and public provenance. Matched-span evidence is recomputed through the existing
production pipeline in public result order: `lexicalCitationSpans` derives each
hit's complete ordered raw evidence and `deduplicateOverlapEvidence` applies
cross-hit interval ownership and suppression. Stored evidence must equal that
complete production output before retained spans are independently checked
against fatal-decoded source bytes, UTF-8 scalar boundaries, and exact LF/CRLF
coordinates. Missing, extra, unrelated, reordered, or coordinate/text-substituted
evidence is a mismatch. A stale citation is a returned result/source-observation
digest or byte binding that no longer matches the sealed current source. A
wrong citation field against an otherwise current result is a citation mismatch,
not stale. A clean reviewed baseline has returned results and complete citation
correctness, but a current ordinary evaluation is still representable when it
regresses to zero hits: the canonical empty-authorized public-result branch
records `not_applicable`, zero citation counts, and null correctness. Its
recall/MRR/nDCG are zero; relevance captures the regression without fabricating
a citation-coverage failure. Nonempty ordinary results record `required` and
check at least one citation. Insufficient and conflict expectations remain in
the separate finite scenario union rather than human golden rows.

Stale-citation occurrences and the number/rate of queries containing at least
one stale citation are separate metrics. Stale projection and unverified
projection each have their own per-query count and aggregate query rate;
unverified confidence mirrors the public coordinator's exact downgrade and
reason algebra. `STALE_CITATION`, `STALE_PROJECTION`, and
`UNVERIFIED_PROJECTION` are independent deterministic zero gates for current,
baseline, and every tune candidate.

## Baselines and tuning

Comparability separates immutable environment and base-configuration digests
from the evaluated tuning-axes digest. Current and baseline query count,
environment, base configuration, golden semantics, and all non-tunable fields
must agree. Each backend scenario has its own baseline. A baseline with any
policy, citation, temporal, confidence, or freshness zero-gate failure is
`needs_human`, never an authorization to regress. Aggregate nDCG passes the
default budget exactly when `current * 100 >= baseline * 98`, including a zero
baseline.

The exact product grid is 6 RRF values × 6 MMR choices × 5 semantic top-k
values × 5 lexical top-k values = 900. A candidate is eligible only when both
top-k axes are at least the maximum expected top-k in the sealed golden set.
The immutable human-reviewed baseline envelope binds the complete environment
set, normalized golden, non-tunable base configuration, grid, selected axes and
candidate configuration digest, metrics set, literal 2/100 budget, metric/table
coordinates, derived counts, and its outer digest. Eval and tune consume that
single envelope; they never recompose or write baseline authority. Any
coordinate, historical expectation, threshold, or non-literal budget mismatch
is `needs_human`.

The pure selector requires the complete eligible axes set and binds every
candidate evaluation digest, the evaluated/excluded counts, query count,
maximum expected top-k, and query-evaluation count. The shipped 24-query,
maximum-top-k-5 fixture evaluates 900 candidates, excludes 0, and performs
21600 query evaluations. At most 30 tuning queries and 900 candidates are
allowed, so the hard ceiling is 27000. Tune selection never changes active
configuration and cannot become a second authorization or retrieval path.

## Scenario and observational boundaries

`scenario-outcome.schema.json` is the exact finite result, insufficient,
authorized-conflict, and pre-snapshot operational-exclusion union. Its
`authority_input_snapshot_count` counts one successfully sealed candidate and
temporal metadata snapshot before reconciliation. Its
`retrieval_sql_stage_count` counts downstream lexical/vector candidate SQL
stage invocations, not mandatory metadata reads. Insufficient and conflict
outcomes have snapshot count one and every downstream counter zero;
operational exclusions occur before a snapshot and have every counter zero.
Provider call and item counts are distinct and a positive item count requires a
positive call count. A scenario-kind transition is a deterministic regression,
not an operational exception.

The ordinary companion catalog selects exactly one environment per golden
`vault_fixture`; environment members partition all golden queries exactly once
in normalized-golden order. Fixed-provider eval and tune schedules are scoped
per environment. Eval always replays its sealed index templates once and then
its query partition. Tune uses the complete eligible-axes-outer/query-inner
implicit matrix with independent dense embedding/reranker call streams; a
31..256-query golden has no tune schedule, while eval remains valid. Provider
templates bind authored/effective query coordinates and all fixed responses;
the audit oracle is never a runtime discoverability-policy input.

## Reviewed hermetic bundle

`reviewed-bundle.json` is the private, coherent Slice-A authority for the
shipped reviewed set. It binds exactly 24 normalized queries and exactly 24
ordered result origins to the same EnvironmentSet, baseline, catalog, canonical
source bytes, projection manifests, fixed-provider schedules, and recomputed
metric inputs. Both provider roles are active offline `local_onnx` identities
for these reviewed members; the identity is inert and never routes work.

Each origin carries two distinct projection coordinates. The manifest
projection ID/digest must equal its environment and schema-3 manifest. The
public result ID/digest is independently recomputed by the existing private
production `lineageResultCoordinate` helper over that manifest plus the exact
authorized temporal source view. The two coordinates are not aliases. Each
query-view audit oracle copies the catalog's complete source/path/endpoint
partitions and adds the expected public-result coordinate derived audit-only
from canonical corpus declarations and `as_of`. It is never derived from a
deliberately widened runtime policy, and is never passed to policy or search.
A coordinate mismatch remains a measurable policy-leak occurrence in generic
metric computation; exact equality is required only by this reviewed zero-gate
bundle.

The origin counter digest covers all eleven named attempt counters. Source
reads mean instrumented source-reader callbacks over the complete policy- and
time-eligible attempt; citation verification counts the actual search and
evaluator verification operations; ranking counts the production ranking
invocation. Index-only provider work is excluded. Slice A seals the expected
counter structure, while Slice B must replay the public coordinator and
exact-match every frozen result, request, stage, and counter observation.

The reviewed temporal proof contains exactly one physical-absence corpus and
one noninterference pair. The absent corpus is the canonical temporal corpus
minus exactly the named future successor, is independently parsed, chunked,
projected, and audited, and must produce the same pre-boundary public view,
query metrics, and query-attempt counters. Its full six-array query-view audit
oracle is independently exact-compared; endpoint classification never falls
back to the source class. Its index receipts are also independently rederived
with production content deduplication, representative order, batch bounds,
offsets, request IDs, and call/item totals. The present transcript supplies
only the fixed response oracle; removed-source responses cannot remain in the
absent receipt stream.

Deterministic result, metric, comparison, and tune bytes never contain timing,
host paths, usernames, machine names, environment maps, endpoints, secrets, or
artifact output paths. Observational timing is a separate safe report with only
bounded runtime version, OS, architecture, backend capability, and runner-class
labels. Fixed provider fixtures are offline, generic, and contain no route,
credential, network, model allowlist, or named-provider preference.
