# GKOS retrieval draft 2

This pack adds canonical GKX lineage provenance and point-in-time selection to
the Phase 1 reference contract. It is an integration draft, not a GKOS
Standard claim. GKOS-Engine TypeScript remains the reference implementation;
GKOS-Engine-Lite is a pin-bound conformant adapter and never a second GKX
authority.

The canonical mapping is fixed: `source_id` is the authored GKX `uid` and
`valid_from` is the resolved canonical `GkxNode.validAt`. The stored accepted
projection derives `valid_to` from the accepted canonical edge set; each public
request recomputes it by suppressing (never creating) edges outside the
authorization-scoped view using the existing temporal projector. Intervals are
half-open `[valid_from,valid_to)`. The pinned current Standard profile has no
canonical serialized `lineage_id`, so it is always null and a derived host may
not mint one. Authored `valid_from`/`valid_to` aliases,
path identity, rank identity, timestamp proximity, and inferred lineage are
never accepted.

The schema-3 manifest and projection digest, as well as the trusted projection
configuration digest, bind Standard study commit
`a2a2a6ca5c4dac32c6d9dc985ed7460f5f4350c6` and Engine projection profile
`gkx-2.3-validating-projection`. A change to either authority coordinate cannot
reuse or reopen the same schema-3 generation identity. Schema 2 remains
unchanged.

The host creates one `GkxIndex` and calls `setFiles` exactly once per corpus
projection. Point-in-time eligibility delegates to the existing
`projectAtTime()` implementation. An explicit `as_of` uses the existing
Engine/GKX timestamp grammar: `T`, optional seconds/fraction, and mandatory `Z`
or numeric offset. Accepted values are echoed and persisted as normalized UTC
ISO strings (`YYYY-MM-DDTHH:mm:ss.sssZ`). Inputs whose offset normalization
would escape that four-digit UTC year range are rejected with
`RETRIEVAL_AS_OF_INVALID`; stored canonical timestamps must already equal their
normalized UTC spelling. Eligibility is applied after
source-level discoverability, typed filters, and the existing chunk-level
discoverability policy (all chunks of a source must allow), but before
live reads, lexical/vector access, provider invocation, ranking, counts,
confidence, citations, or parent expansion.

When source created/modified stats are both absent, a manifest-bound
`projection_reference_time` is required even if authored `created_at` exists,
because the canonical graph's generic file metadata otherwise uses index time.
Without that reference the intrinsic candidate is rejected before the single
canonical index projection and cannot change unrelated candidate bytes; an
unrecorded index wall clock is never used. Unknown or incomplete lineage
validity is nullable, honest, and ineligible for `as_of` rather than treated as
current or all-time. A nonempty authorized and typed-filtered corpus with any
unknown canonical interval or no temporally valid source
returns zero hits and `TEMPORAL_COVERAGE_INSUFFICIENT`. An empty authorized or
typed-filtered corpus remains the ordinary non-oracular `NO_ELIGIBLE_RESULTS`
case with temporal coverage `not_evaluated`.

The physical `source_snapshot_digest` binds every safely observed note (also
rejected notes, by path/digest/byte-size/source times and deterministic
classification), normalized folder/attachment topology, canonical accepted
source coordinates, and projection reference/options. Rejected bytes remain
unindexed. These physical snapshot inputs can advance a generation without
changing the authorization-scoped ordinary result.

Every hit contains the flat required provenance fields plus assertion binding.
Stored source provenance requires a nonempty title, sensitivity, and
`authoritative:true`; title is bounded to 512 ECMAScript UTF-16 code units.
`assertion_time` is present exactly when `assertion_origin` is
`gkx_created_at` and `source_metadata.authored_at` is present with the exact
same normalized UTC value. A null assertion requires `authored_at` to be
absent. When `assertion_time` and `valid_from` are both non-null,
`validity_origin` is `gkx_authored_timestamp` and `valid_from` exactly equals
`assertion_time`; that origin in turn requires both values. An accepted
lineage-incomplete record may retain a canonical assertion while validity is
null/`unknown`, so assertion presence alone never fabricates validity. JSON
Schema enforces the required metadata strength and the expressible
assertion/origin/presence cases; exact cross-field equality and the UTF-16
bound are frozen here and enforced by both runtimes' executable conformance
gates.
Content-bearing parent expansion contains its own provenance assertion bound to
the parent chunk/content digest. The complete canonical UTF-8 serialized result
envelope—not only note text—is bounded; when optional parent context would cross
the bound, the parent is omitted before its already-selected child.
Only authorized canonical UIDs that are not future at an explicit `as_of` may
appear in `supersedes` or `superseded_by`. Thus a selected successor may expose
its authorized historical predecessor, while a selected predecessor never
exposes a future successor. Public `lineage_neutral` and participant
reasoning are derived only from those visible endpoint arrays, so a suppressed
edge cannot create an existence oracle. Raw authored title/path/wikilink references
remain internal to the disposable projection. Endpoint suppression is silent;
the invariant reason `LINEAGE_VIEW_AUTHORIZED_ONLY` is present regardless of
whether any endpoint was suppressed. The public provenance digest is computed
only over that redacted result envelope and never commits to hidden raw refs.

`temporal_state` is wall-clock-free: `current` means canonical `invalidAt` is
null, `historical` means it is non-null, and `unknown` means canonical temporal
classification is unavailable. `future` exists only as an excluded diagnostic
classification relative to explicit `as_of`; it is never a returned hit.
Equal endpoints form an honest empty half-open interval and are never eligible.

No durable ledger adapter is verified in this phase. Results therefore set
`ledger_binding_verified:false` and omit `ledger_entry_sha256`; a document or
projection digest is never relabeled as a ledger binding. Phase 1 ranking,
provider-neutral routing, policy, resource, path, lexical-backend, and live
UTF-8 citation rules remain in force and are not weakened by this overlay.

The physical SQLite generation coordinate is internal to ordinary draft.2
search. Returned `projection_id` and `projection_digest` identify only the
authorization-, filter-, and time-scoped view and bind the engine, schema,
provenance, chunker, tokenizer, lexical-backend, vector-space, and visible
temporal relationship coordinates. `projection_freshness` is `unverified`
unless the trusted host supplies view-scoped reconciliation evidence. A host
that has just scanned and atomically built the same vault snapshot may report
`fresh`; failed live byte/line citation verification reports `stale`. The
legacy service-global stale flag remains a draft.1 concern and cannot cause a
draft.2 result to claim freshness or reveal denied-source activity.

The executable projection fixture carries complete expected physical-manifest
and ordinary-result envelopes for both `sqlite_lexical_scan` and
`sqlite_fts5`. The scan result has complete variants for runtimes with and
without FTS5 capability because the latter must also report
`SQLITE_FTS5_UNAVAILABLE`. A runtime selects only the backend and capability
state it actually opened and must compare the entire envelope, including
physical and scoped projection coordinates, freshness, stages, scores,
citations, and provenance. Selective field comparison or advertising one
backend while opening the other is not conformance.

Phase 2 is additive at the public build boundary. The frozen
`buildRetrievalGeneration` input continues to create draft.1/schema-2 state.
`buildGkxRetrievalGeneration` instead requires the strict candidate-only
schema-3 input: `candidate_sources`, parser-owned
`candidate_declarations`, `candidate_chunks`, and the policy-digest-bound
`embedding_eligible_candidate_chunk_keys`. Missing candidate evidence is never
synthesized and the legacy schema-2 fields are not accepted on this path.
Every intrinsically accepted parser candidate, including duplicate public
UID/path identities and a frontmatter-only candidate that emits zero chunks,
has its own internal SHA-256-derived `record_key` row. Candidate source rows
bind only source-local facts: they never pre-certify `valid_to`, resolved
endpoints, `temporal_state`, or `lineage_neutral`. Raw declaration references,
resolver-tier record-key sets, candidate keys, physical counts, and the
physical generation coordinate remain trusted-host-only and cannot appear in
ordinary results or public projection coordinates. All candidate chunks remain
local for temporal/lexical integrity, but only candidate chunk keys in the
eligibility set may cross an embedding provider boundary or receive persisted
vectors. The manifest records explicit candidate source/declaration,
represented-source, candidate-chunk, and eligible-chunk counts and binds the
complete canonical physical multiset into the projection digest.
Both policy callbacks receive deep-frozen source-local copies with global
`valid_to`, relationship endpoints, hidden agent/MOC identifiers, and unknown
metadata extensions suppressed, so hidden records cannot influence an
allow/deny decision. The trusted runtime policy digest must exactly match the
manifest policy digest before either callback or any source/provider work.

Decision A is ratified for all four authorization-dependent cross-record
classes: canonical UID/path/parser-fingerprint/public-chunk identity,
title/wikilink/UID endpoint resolution, forward/inverse or conflicting
declarations, and multiple-successor/cycle/temporal-order topology. The single
canonical parse retains every candidate and its parser-owned resolver basis
sets. Retrieval first applies the runtime policy digest, source policy, typed
filters, and whole-source chunk policy. For an explicit `as_of`, unknown and
future candidates are then removed before any identity, endpoint, declaration,
self, topology, or order decision; unknown contributes only the coverage bit,
while future is silently indistinguishable from absence. Resolver precedence
is the exact existing Engine order and stops at the first nonempty scoped tier.

An authorization-hidden candidate is byte-for-byte indistinguishable from a
physically absent candidate in the complete ordinary result, including errors,
stages, counts, confidence, scoped projection coordinate, and citations. If
all conflicting known-created participants are authorized, retrieval fails
with the one stable `RETRIEVAL_AUTHORIZED_VIEW_CONFLICT` code before live
source reads, lexical/vector SQL, query embedding, reranking, ranking, counts,
confidence, citations, or parent expansion. Its message reveals no conflict
class, count, UID, path, raw reference, internal key, or physical coordinate;
that conflict wins over simultaneous unknown temporal coverage. Ordinary
broken or ambiguous Markdown/wiki links are not lineage conflicts. Intrinsic
source syntax/schema/timestamp errors remain per-candidate prepublication
fail-closed, and resolution-dependent self classification occurs only after
the scoped canonical tier is selected. The executable matrix freezes both the
hidden-versus-absent and all-authorized outcomes for every ratified class.
