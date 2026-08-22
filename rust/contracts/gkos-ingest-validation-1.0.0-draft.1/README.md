# GKOS ingest validation draft 1

This locally frozen integration pack defines the Phase 3 owner-plane validation
boundary. It is not a GKOS Standard claim and remains unpublished. Exact byte
sizes and SHA-256 values for all 21 files are recorded outside the pack in
`evidence/2026-08-21-functional-uplift-phase-3.md`, avoiding a self-hash
dependency. Final reciprocal review and hosted CI are `UNASSIGNED`. The
parser/profile/validator, owner-storage protocol, and Phase 3 CLI orchestration
are implemented.
GKOS-Engine is the only YAML/frontmatter and profile authority.
GKOS-Engine-Lite may verify normalized and owner-state envelopes and delegate
exact command arguments, but it never parses YAML or TOML and never resolves
GKX identity or relationships.

The built-in selector is `gkos:frontmatter-profile/current`. It binds Standard
study commit `a2a2a6ca5c4dac32c6d9dc985ed7460f5f4350c6`, Engine profile
`gkx-2.3-validating-projection`, and the existing canonical parser. An
operator may explicitly select a bounded local strict-TOML overlay. There is
no network reference, vault discovery, default injection, coercion, source
rewrite, autofix, identity creation, reference resolution, or sensitivity
promotion. The raw overlay SHA-256 binds exact bytes; a separately normalized,
path-free effective-profile envelope binds the semantic digest consumed by
conformant adapters.

The strict TOML subset treats only ASCII U+0020 as syntax whitespace; line
separation is LF or CRLF. A terminal line terminator closes the preceding
logical line and does not add a synthetic empty line; every earlier blank or
comment line counts toward the 2,048-line limit. Other Unicode whitespace,
including NBSP and FEFF, is rejected outside quoted strings and comments.
Valid Unicode remains permitted inside a quoted value or comment, subject to
the stated value bounds. An explicit profile selector is rejected lexically before filesystem
access if it contains any C0 control, DEL, an unpaired surrogate, a network or
device namespace, a drive-relative spelling, or an alternate-data-stream
colon. Valid non-control Unicode local filenames remain permitted and sealed.

Phase 3 `--kb-path` uses the same pre-I/O local-authority principle. Both the
raw argument and its resolved absolute spelling are checked before profile or
filesystem access. UNC, extended-local or extended-UNC, device, NT
object-manager, drive-relative, alternate-data-stream, reserved-device,
control/DEL, unpaired-surrogate, trailing-dot/space, empty, and filesystem-root
spellings reject as usage. Ordinary local absolute or relative paths remain
valid. On Windows, ordinary drive, case, and 8.3 spellings converge through
native canonicalization; extended namespaces never become accepted aliases.
After profile selection, the host mints one opaque identity capability before
scan, scans that capability's canonical root, and consumes the same capability
through state-authority acquisition and the final pre-provider boundary. A
renamed or replaced root therefore reaches neither state creation nor a
provider.

Raw `required_fields` contains at most 128 unique, nonempty items of at most
128 UTF-16 code units; every item must then satisfy the finite canonical-field
set or the no-dot extension grammar. A present `enum` contains one through 256
unique, nonempty decoded strings of at most 512 UTF-16 code units. The only
recognized basic-string escapes after a backslash are `b`, `t`, `n`, `f`, `r`,
double quote, backslash, and `u` followed by exactly four ASCII hexadecimal
digits. Recognition does not bypass decoded-value safety: C0 controls, DEL,
unpaired surrogates, and the applicable decoded-string bound still reject.

Schema identifiers use the stable project-owned
`https://github.com/Odenknight/GKOS-Engine/contracts/ingest/...` namespace.
Draft 2020-12 validation is necessary but not sufficient: every consumer must
also execute the frozen semantic seal. That seal rejects duplicate or
out-of-order profile coordinates, widening algebra, noncanonical portable
paths, forged finding classifications or severity floors, mismatched temporal
or content bindings, noncanonical nested finding order, digest drift, and
result predicate/count/partition drift. Lite verifies this normalized envelope
and its digests; it does not read TOML or create GKX meaning.

Overlay algebra is monotone. Required fields are unioned over the requireable
field domain; `sensitivity` is deliberately outside that domain, so
`sensitivity.required` is always false and `sensitivity` cannot occur in
`required_fields`. Canonical types and shapes must remain exact; canonical
enums may only narrow; string bounds may only intersect; minimum sensitivity
may only rise; and unknown-field handling may only tighten. Only the finite
profile-raiseable diagnostic set may rise in severity.
`GKX-SENSITIVITY-001` is immutable at `warning`, so missing sensitivity remains
accepted with effective-secret handling under every profile. An explicit
sensitivity enum must retain at least one value at or above the minimum even
though the field is optional. A validation-only extension uses the exact
no-dot ID grammar `x-[a-z0-9][a-z0-9_-]{0,62}` (dots are structural TOML
separators) and begins with one explicit finite string, integer, boolean, or
array-of-string domain. It can validate authored data, but cannot create or
transform a canonical GKX value. Canonical fields without a frozen domain are
not overlay-addressable. An explicitly authored `null` is a present value,
never absence: it fails the finite field type at its exact parser-owned line.
Only a truly absent required field uses `missing_field` with `line:null`.

All source meaning comes from the one canonical parser pass. Parser-owned
location receipts use one-based document lines. A finding without such a
receipt has `line:null` and an honest finite coordinate basis; line 1 is never
invented. Paths are normalized portable vault-relative paths. Findings contain
only finite codes and coordinates—never parser messages, raw values, raw
references, absolute paths, internal record keys, or source content.

`validate` is invalid when any intrinsic or cross-record report-only finding
has severity `error` or `critical`. Physical ingest eligibility is narrower:
only intrinsic `error` or `critical` findings reject or block a source.
Decision-A identity collisions, endpoint ambiguity/unresolved state,
declaration reconciliation, and branch/cycle/order facts remain
authorization-scoped query authority. Validation may report their single
generic corpus conflict, but they never enter a rejection row and never remove
a physical candidate.

Each intrinsically invalid observed source yields exactly one sealed rejection.
Every physical candidate and scanner rejection appears in the complete owner
`observations` array. Observations sharing a source path receive unique
zero-based ordinals in this exact seed order: source SHA (null as empty), UTF-8
byte size (null as -1), kind (`candidate` before `scan_rejection`), and safe
detail (empty for candidates; canonical JSON of the sorted scanner reason set
for scanner rejections), followed only by a private deterministic occurrence
tie-break. Byte-identical candidates or scanner receipts remain a multiset;
swapping indistinguishable private ties cannot change sealed bytes. Every
source-scoped finding carries the observation ordinal. A rejection binds the
safely observed path, content SHA and UTF-8 byte size when available,
canonical authored assertion/validity when available, all sorted safe
intrinsic findings, and the effective profile.
Only `error` or `critical` findings control the rejection predicate.
Filesystem creation/modification times, wall clock, raw bytes, raw diagnostic
messages, and internal record keys are absent. Missing sensitivity continues
to use the existing accepted effective-secret warning. An explicit sensitivity
below the overlay floor is an intrinsic blocker; it is never silently raised.

Safely identified child-note UTF-8, size, alias/reparse/hardlink, read, or
identity-race failures become one intrinsic rejection with every applicable
safe reason. Root enumeration/containment authority, schema, state/pointer, and
owner-plane permission failures are operational errors and never synthetic source
rejections. No source bytes are parsed after a failed scan seal.

The validation-result semantic seal verifies that occurrence ordinals are the
contiguous sequence `0..n-1` across *all* observations at each source path.
The rejected-observation partition and rejection rows are exactly one-to-one,
so rejection ordinals may have gaps when accepted observations share a path.
Each rejection carries exactly all top-level intrinsic findings for its
`(path, ordinal)` observation. It also binds every finding code with a
canonical field authority to that exact safe field coordinate; codes with no
field authority must carry `field:null`. The fail-closed unmapped canonical
diagnostic uses only the fixed redacted shape `intrinsic/error/frontmatter/
file_observation`, with `line:null` and `field:null`; future diagnostic fields
or messages never enter the envelope. These cross-row and code-to-field
invariants are mandatory even where Draft 2020-12 cannot express the complete
relationship.

All canonical array ordering uses ECMAScript UTF-16 code-unit lexical order,
never locale, Unicode-scalar, UTF-8, filesystem, or database order. Normalized
severity, field, required-field, and enum arrays sort by their string value.
Findings sort by `(source_path|null="", ordinal|null=-1,
line|null=9007199254740991, code, field|null="", finding_id)`; observations by
`(source_path, ordinal)`; rejections by
`(source_path, ordinal, rejection_digest)`; and finding-ID arrays by digest.
Every digest uses the frozen retrieval draft.1 canonical JSON algorithm:
UTF-16-sorted object keys, dense array order, JSON-compatible finite-number
rendering, no Unicode normalization, and rejection of unpaired surrogates.
Ingest additionally rejects unsafe integers and negative zero. `finding_id`
omits only itself, `rejection_digest` omits only itself, the effective-profile
digest covers the complete normalized envelope, and the observation snapshot
covers `{contract_version,effective_profile_digest,sources}`. SHA-256 is over
the canonical JSON UTF-8 bytes; only the overlay SHA hashes raw sealed bytes.

The composite publisher has one authority pointer and one cross-writer guard.
The guard is acquired before cache, provider, or derived-state work and is
shared with legacy Phase 1/2 pointer writers through their actual commit.
Under that guard, Full derives an opaque accepted-source-only chunk plan,
optionally embeds only those chunks, builds a schema-3 inner database without
activating the legacy pointer, then publishes the content-addressed inner DB,
rejection journal, and owner manifest no-replace. Every artifact is reopened
and verified before activation. A standalone crash-orphan journal carries its
normalized profile and is fully safe-sealed; an owner reopen additionally
binds the exact accepted `(source_path, source_digest)` multiset to persisted
candidate-source rows.

First activation then binds the exact target and its public-safe inner
coordinate into the authority lock, preserves any verified legacy pointer in
a content-addressed migration record, and publishes a retained activation
root. The activation root—not the prepared migration or lock intent—is the
irreversible no-downgrade boundary. It binds the first owner generation,
`inner {database_file, manifest_digest, projection_id, projection_digest}`,
migration and prior legacy digest, tombstone, active-pointer file digest, and
authority-lock digest. Only after that root exists may recovery complete the
activating witness, typed legacy tombstone, sole `active-ingest.json` pointer
(published last), and active witness. The safe `inner` object is also required
in every ingest `prior_active`, including blocked-attempt status and authority
locks; the legacy prior shape remains projection ID/digest/pointer digest.

Before the activation root, a live capability may abort only when the exact
prior authority and exact in-memory namespace are unchanged. At or after the
root, active-pointer commit, or blocked-status commit, generic release refuses
and retains the intent. Stale recovery exclusively claims the exact guard by a
hard link, accepts only the exact prior or exact bound target, and semantically
seals every controlled artifact before removing the canonical lock and then
the claim. Fully sealed content-addressed pre-activation artifacts may remain
as non-authoritative crash orphans. Alias/hardlink/reparse, permission,
case-spelling, containment, descriptor-identity, timestamp, size, or canonical
JSON failures are operational and fail before provider work. On POSIX, every
controlled JSON and SQLite artifact must be mode `0600` before/open/after the
read; public search, owner reopen, and pre-provider cache lookup never repair a
widened mode. Windows ACL qualification remains host-specific. Pointer/root/
witness/migration/status/lock JSON is capped at 1 MiB; owner manifests and
journals are capped at 512 MiB; every controlled artifact, including the
active SQLite database, has a shared 16 GiB pre-open ceiling.

Strict intrinsic failure publishes no immutable generation and only advances
an owner attempt status bound to the exact prior active coordinate, attempt
digest, profile digest, and observation snapshot. `prior_active:null` reports
unavailable; a verified legacy or ingest prior reports stale. A later pointer
advance makes an old status historical without rewriting it. Repeating the
same attempt against the same exact prior is a no-op: status bytes/mtime and
all prior active artifact bytes/mtimes remain unchanged. A strict plan with
`ingest_intrinsic_valid:true` still publishes every physical accepted source
and declaration when a Decision-A report-only conflict makes
`corpus_valid:false`. The path-free
index result contains exactly `{contract_version,status,mode,summary,active,
blocked_attempt}`. Its statuses are `published`,
`published_with_rejections`, `blocked_strict`, or `operational_failure`;
`blocked_attempt` carries only attempt/status digests. It cannot contain a
validation result, owner manifest, source path, finding, observation,
rejection, journal coordinate, or profile coordinate.

The executable storage fixture covers every discriminated branch: witness
`activating|active`; authority-lock `preflight|activation|blocked`; prior
active `null|legacy|ingest`; attempt availability `unavailable` for null and
`stale` for legacy/ingest; migration without/with legacy; and all four index
result statuses, including strict and non-strict publication.

Ordinary search verifies a legacy pointer before first activation or the
root/witness/migration/tombstone plus safe active pointer and inner DB after
activation. It never loads the owner manifest, rejection journal, normalized
profile, or attempt status. It continues to expose only the existing scoped
Phase 2 projection coordinate. A real vector-built differential proves that
visible-only and visible-plus-intrinsic-invalid corpora have byte-equal search
envelopes and identical source-reader, query-vector, and rerank call sequences;
rejected sentinel bytes never cross an indexing or query provider boundary.

A strict block before first activation is routed by the exact canonical
attempt-status *artifact metadata* only. Its JSON bytes are never opened,
hashed, parsed, or applicability-checked on the public path. That metadata
permanently suppresses legacy auto-reindex: ordinary search verifies and holds
the unchanged legacy prior, or reports safe operational unavailability if no
prior exists. Active and blocked-prior stores are verified and held before any
config, credential, query-provider, reranker, or source-reader preparation;
the configured coordinator consumes that same held store without reopening it
by path. Missing, corrupt, aliased, case-noncanonical, permission-invalid, or
raced authority is a finite path-free operational failure.

Both `gkx validate` and `gkx index` accept the same bounded `--schema`
selector: exact built-in ID `gkos:frontmatter-profile/current` or a local
overlay path. Omission selects that same built-in ID and coordinate. No
`current-standard` alias exists, so an ambiguous relative filename cannot
silently become a second ID. Neither command discovers a vault profile or
follows a network reference. CLI outcome codes are frozen as: 0 publication
success (including non-strict success with rejections), 1 validation/strict
block, 2 usage or profile selection failure, and 3 operational publication
failure. No contract in this pack authorizes a source write, merge, tag,
release, deployment, or package publication.

The Phase 3 and legacy validate forms are deliberately disjoint. Mixing a
positional directory with `--kb-path`, using Phase-3-only flags on the legacy
form, passing duplicate/missing flag values, or giving `index` a positional
path is a pre-I/O usage error. JSON output is exactly
`JSON.stringify(sealed_result,null,2)` plus one LF. Validate text is the exact
four-line-class renderer frozen in `cli-conformance-fixture.json`, followed by
one LF; finding rows repeat in sealed order. Every index status emits the exact
six-field path-free IndexResult JSON. Successful and validation/block outcomes
leave stderr empty; operational index failure adds only
`gkx index: operational failure` plus LF. The executable CLI fixture binds
these bytes, exits, safe fixed errors, path grammar, and public search routes.
