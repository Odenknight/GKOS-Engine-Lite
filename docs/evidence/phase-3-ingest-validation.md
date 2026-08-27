# GKOS-Engine-Lite Phase 3 ingest-validation evidence

Qualification date: 2026-08-22

Repository: Odenknight/GKOS-Engine-Lite

Phase scope: exact delegation of Full's Phase 3 validate/index/search CLI,
private verification of Full-produced ingest envelopes, and a cross-runtime
legacy retrieval-writer guard that prevents the frozen Phase 1/2 writers from
racing or downgrading Full's owner authority.

Lite state: **DONE** for the exact qualified implementation commit below. At
its reciprocal-review freeze, the one-file evidence-only closeout was
unstaged, uncommitted, and unpushed; Full approved that exact freeze before it
was published as the signed evidence commit below. This later wording-only
correction was not assigned a commit at its own review freeze, so the document
does not assert its own SHA. Pull request #18 remains draft, open, and
unmerged. No Lite merge, tag, release, deployment, package publication, or
artifact publication is claimed.

## Exact coordinates

| Coordinate | Value |
| --- | --- |
| Phase 2 evidence base | `45bfc3c4b66ae2978ddae814022d8c861076eec3` |
| Working branch | `codex/phase-3-ingest-validation` |
| Lite qualified implementation | `9e2a1cbd070e7b2d08aa094e692b65eb50213ccd` (ED25519 signature and DCO verified; direct child of the Phase 2 evidence base) |
| Lite first evidence-only closeout | `c31c1369dea710dbbcc6e67ca00c2608ac2d33b7` (ED25519 signature and DCO verified; direct child of the qualified implementation) |
| Lite wording-only correction | Not assigned at correction-review freeze time; it does not supersede the implementation or first evidence coordinate |
| Lite pull request | Draft [#18](https://github.com/Odenknight/GKOS-Engine-Lite/pull/18), open against `codex/phase-2-lineage-citations` |
| Lite reciprocal review | PASS; Full read-only review approved the exact 42-path local implementation freeze with no blocker, HIGH, or MEDIUM finding |
| Hosted Lite CI | PASS; [PR run 32558687019](https://github.com/Odenknight/GKOS-Engine-Lite/actions/runs/32558687019), 8/8 jobs successful |
| Full qualified implementation | `e7cc0dd478af3d0bda216c5258dec5f77932def7` (ED25519 signature and DCO verified) |
| Full package | `gkos-engine` 2.1.2 |
| Full Phase 3 pull request | [#28](https://github.com/Odenknight/GKOS-Engine/pull/28); qualified implementation remains the exact code pin even though later evidence/provenance-only commits advanced and merged the phase branch |
| Full hosted qualification | PASS; [push run 32552881178](https://github.com/Odenknight/GKOS-Engine/actions/runs/32552881178) and [PR run 32552883201](https://github.com/Odenknight/GKOS-Engine/actions/runs/32552883201) |
| Ingest contract | `gkos-ingest-validation/1.0.0-draft.1` |
| Built-in profile selector | `gkos:frontmatter-profile/current` |
| Local Node runtimes | 22.23.2, 23.11.1, 24.18.0 |
| Rust latest / MSRV | 1.98.0 / 1.85.0 |

The dependency, lockfile, and `FULL-PIN.json` bind the exact qualified Full
implementation commit, not a later documentation-only or merge coordinate.
The two terminal Full runs tested that implementation before the later
evidence/provenance-only branch advancement. Their successful job IDs were:

- push: `96982351940`, `96982352049`, `96982352050`, `96982352053`,
  `96982352070`, and `96982352140`;
- pull request: `96982356658`, `96982356791`, `96982356798`,
  `96982356833`, `96982356836`, and `96982356844`.

The Lite workflow triggers branch pushes only on `main`, so the implementation
push to `codex/phase-3-ingest-validation` created no separate push-event run.
Opening draft PR #18 created the sole hosted run for the exact implementation
SHA. All eight jobs reached terminal success with zero failed or skipped jobs:

- `retrieval-rust-latest`: `96996771579`;
- `test (24)`: `96996771672`;
- `desktop`: `96996771677`;
- `desktop-native`: `96996771691`;
- `retrieval-rust-msrv`: `96996771696`;
- `test (23)`: `96996771702`;
- `retrieval-rust-windows-msvc`: `96996771705`;
- `test (22)`: `96996771714`.

The Windows MSVC job performed the mandatory all-target tests with
`GKOS_REQUIRE_ALIAS_FIXTURE=1` and the compile-fail documentation lane. The
Node jobs exercised exact wrapper/CLI bytes, metadata, dependency lock,
package/static checks, and the supported Node 22/23/24 matrix. The remaining
Rust and desktop jobs qualified latest, MSRV, desktop, and native desktop
surfaces on the same implementation SHA.

After Full approved the one-file DONE closeout, signed DCO evidence commit
`c31c1369dea710dbbcc6e67ca00c2608ac2d33b7` advanced the draft PR without
changing the qualified implementation coordinate. Fresh PR run
[32559214040](https://github.com/Odenknight/GKOS-Engine-Lite/actions/runs/32559214040)
requalified that exact evidence head with 8/8 terminal-success jobs:

- `test (23)`: `96998104143`;
- `retrieval-rust-msrv`: `96998104163`;
- `desktop`: `96998104191`;
- `test (22)`: `96998104193`;
- `retrieval-rust-latest`: `96998104203`;
- `retrieval-rust-windows-msvc`: `96998104209`;
- `desktop-native`: `96998104214`;
- `test (24)`: `96998104223`.

## Frozen 21-file ingest pack

The exact copied pack is 21 files and 248079 bytes. Every file is UTF-8 with
no BOM or CR and exactly one terminal LF. `contract.json` is `status:"frozen"`,
`frozen:true`, and `hash_manifest_issued:true`. The separate `FULL-PIN.json`
matches every byte below.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `README.md` | 17680 | `c3acdf6b0870694ca9bd63a25da0fbe1bccd757ee76521284f048888f302ac5b` |
| `activation-root.schema.json` | 1607 | `59383a035ad0edc25e087ac5a94f1a5564002d849e1d6116ec814c1322608bfe` |
| `active-pointer.schema.json` | 884 | `7f877c9dc3827356b097a7635965b63a64349ffb4331efa3be720629f9347a7d` |
| `attempt-status.schema.json` | 1391 | `d474ba6e10f8636334ea6c63fccb8ea06708cbd947ee20934c38e716e2c80b26` |
| `authority-lock.schema.json` | 2725 | `1f8505291e2be7abb5e59b301beb410dd4f903d010623de461ba84da9ed58603` |
| `authority-witness.schema.json` | 1762 | `372f25fbe7c83fce7be5185c40e43866d8e667406bb8afafdc39ad5a45876962` |
| `cli-conformance-fixture.json` | 14150 | `567f7d3371ddfe33ec314b05959f90b90080d55519e1a1a2618ce74798d6e680` |
| `conformance-fixture.json` | 33037 | `920ae8bdd54a633e490a6e6efa34f73adfd55bb6786562da08655797931e26c8` |
| `contract.json` | 29295 | `ee62ad7f1b0d2ae9a626680bbdca20feca443a52d7ba0d09b58b07979c3e0061` |
| `finding.schema.json` | 15817 | `8aee60ee2a2906081a244421664da564867a7c8fa71d8aebe775968d359756f1` |
| `index-result.schema.json` | 3788 | `ccd3361cec041be4d220ab6e10833ee48d7b46255a3463341d9e553454627bae` |
| `legacy-tombstone.schema.json` | 1046 | `c7bff930b5fa0fa174cd13f47ac9d9f1b19b7f1adadb2fa5c1292ff32924256e` |
| `migration.schema.json` | 1486 | `680d3f9d87bcbd637c772886333f7870e6f0b2b7bd76d0a047c77060e7314fcf` |
| `normalized-profile.schema.json` | 17631 | `016e3673f7db116ef53f0ce32e4faeb572eb1f5505c016ccd1b26e407e93c9c4` |
| `owner-generation.schema.json` | 2738 | `4c70f4a84240ad712c0ef943801332c5339e9021dd65c561563e03f173bd1ea8` |
| `profile-coordinate.schema.json` | 2498 | `1a4c2f7102c957aa6e1a9afdc6ab935ff1c5d942f1557272f9a151becef3b3db` |
| `rejection-journal.schema.json` | 1124 | `abf8e836ab7f2a4ec968c8c19ceef250265020c53e10ec84684a57edc006ca37` |
| `rejection.schema.json` | 2780 | `0d318333b02b8fdf1654ffff52f75fa3aa67640ee7d692f4e759a67ec00432c5` |
| `result.schema.json` | 5030 | `0a4ce21898d14a1cbaef3cf4f37b963cd361e547d2c68395578c241e9d0c50ed` |
| `state-common.schema.json` | 3331 | `6cccb19ede2357782af5a330e07745954da4b4bed2881470a6a04371bdf6a58e` |
| `storage-conformance-fixture.json` | 88279 | `35a91dd23354fe0d5b2ad5a1d6f68019a84c1070f1d3cb45910d939927eb03e0` |

## Lite implementation and authority boundary

- The JavaScript wrapper admits only the six delegated Lite commands and
  forwards the original validate/index/search argument arrays unchanged.
  Fixture-driven subprocess tests compare exit status, stdout, and stderr
  byte-for-byte against the exact pinned Full executable. Only the already
  approved PID field in Node's exact SQLite experimental-warning line is
  normalized, and source-note bytes are sealed before and after execution.
- The Rust ingest module parses only strict canonical JSON envelopes produced
  by Full. It verifies normalized profiles, safe findings, validation results,
  rejections, journals, owner manifests, state unions, index results, retrieval
  manifests, digests, counts, ordering, and cross-envelope bindings. It does
  not parse YAML or TOML and does not mint GKX identity, profile, relationship,
  temporal, sensitivity, or Decision-A authority.
- The Rust raw ingest module and envelope capabilities are crate-private, with
  downstream compile-fail tests. Lite has no Phase 3 outer filesystem staging,
  reopen, pointer, witness, tombstone, status, journal, or activation writer;
  delegated Full remains the sole Phase 3 owner-plane writer.
- The frozen Phase 1 public schema-2 build/index/activate APIs and private
  schema-3 path are protected by Full's exact six-field durable legacy-writer
  lock/recovery handshake before the first provider trait call. Every guarded
  pointer publication is exact Full two-key JSON; the one approved historical
  three-key current-engine Lite pointer converges on the next activation.
- Phase 3 authority, a held writer, malformed/case-aliased state, or widened
  immutable state fails before provider identity/embed, cache, database, or
  pointer work. Cancellation and unwind release only the exact owned lock.
  Full-compatible stale recovery handles the shared lock/pointer temp grammar;
  a later Lite writer removes exact private SQLite build orphans only after
  acquiring and revalidating a fresh shared capability.
- The frozen predicate, sensitivity, Unicode ordering, Decision-A, storage
  union, semantic-negative, presentation, path, index-status, and
  search-routing matrices are consumed with exact row/class-set equality. No
  fixture row is merely copied without an executable assertion.

## Qualification

| Runtime / gate | Exact result |
| --- | --- |
| Node 22.23.2 root suite | PASS; 38 passed, 0 failed, 0 skipped |
| Node 23.11.1 root suite | PASS; 38 passed, 0 failed, 0 skipped |
| Node 24.18.0 root suite | PASS; 38 passed, 0 failed, 0 skipped |
| Desktop typecheck, unit suite, and production build | PASS; 20 passed, 0 failed, 0 skipped |
| Rust 1.98.0 Windows GNU | PASS; 155 library + 11 conformance + 3 compile-fail docs; fmt and clippy `-D warnings` PASS |
| Rust 1.98.0 native Linux GNU | PASS; 159 library + 11 conformance + 3 compile-fail docs; check and clippy `-D warnings` PASS |
| Rust 1.85.0 native Linux GNU MSRV | PASS; check; 159 library + 11 conformance + 3 compile-fail docs |
| Frozen ingest pack | PASS; 21 files / 248079 bytes / zero hash, BOM, CR, or terminal-LF mismatches |
| Root metadata and dependency lock | PASS; package 2.1.2 and exact Full SHA `e7cc0dd…`; one immutable 40-hex Git dependency |
| Root dry package | PASS; 5 files / 9067 packed / 23397 unpacked / SHA-1 `217934ab74fb0c353da61c2b4e4d6ab8daac8692` / SHA-512 `GcXBPMwWG1sKtX6rSXcqRP+K3qpZawUKQCMG6MTIXPqMuN8XRouyDMRnNA9qPp4H0GZYNVU6e9npj3p7FF+aWQ==` |
| Phase 0–2 contract/evidence/compatibility bytes | PASS; no diff from `45bfc3c4b66ae2978ddae814022d8c861076eec3` |
| Static boundary, forbidden-provider, merge-marker, diff, and staging scans | PASS on the qualified implementation; no external ingest module, no named-provider restriction, no merge marker, no whitespace error, staging empty |
| Qualified implementation scope | PASS; the signed commit contains exactly the 42 reciprocally approved paths and no `.tgz` artifact |
| Evidence-only closeout scope | PASS; exactly this one unstaged file, with staging empty |

The local machine has the Rust MSVC target but not Visual Studio Build Tools or
the Windows SDK linker, so its local MSVC invocation stopped before compiling
this crate with `link.exe not found`. That remains an honestly recorded host
capability absence, not a local pass. Hosted job `96996771705` supplied the
mandatory Windows MSVC qualification and reached terminal success.

## Closeout boundary

The implementation publication gates are complete: reciprocal approval, the
signed DCO implementation commit, exact remote/PR-head equality, and all eight
hosted jobs are green. The qualified implementation SHA remains
`9e2a1cbd070e7b2d08aa094e692b65eb50213ccd` even after a later evidence-only
commit advances the draft PR head.

The first closeout file received a separate reciprocal read-only review before
it was committed as `c31c1369dea710dbbcc6e67ca00c2608ac2d33b7`; its fresh
hosted run is recorded separately above. This later wording-only correction
changes no implementation, contract, pin, package, or qualification claim and
cannot supersede the qualified implementation or its run coordinates.

No merge, tag, release, deployment, npm/package publication, or artifact
publication occurred or is authorized by this closeout.
