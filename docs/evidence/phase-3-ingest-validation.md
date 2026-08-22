# GKOS-Engine-Lite Phase 3 ingest-validation evidence

Local freeze date: 2026-08-22

Repository: Odenknight/GKOS-Engine-Lite

Phase scope: exact delegation of Full's Phase 3 validate/index/search CLI,
private verification of Full-produced ingest envelopes, and a cross-runtime
legacy retrieval-writer guard that prevents the frozen Phase 1/2 writers from
racing or downgrading Full's owner authority.

Lite state: **FROZEN_LOCAL**. This is an unstaged, uncommitted, unpushed local
freeze for reciprocal Full review. The Lite implementation commit, pull
request, and hosted CI coordinates remain `UNASSIGNED`. No Lite merge, tag,
release, deployment, package publication, or artifact publication is claimed.

## Exact coordinates

| Coordinate | Value |
| --- | --- |
| Phase 2 evidence base | `45bfc3c4b66ae2978ddae814022d8c861076eec3` |
| Working branch | `codex/phase-3-ingest-validation` |
| Lite implementation commit | `UNASSIGNED` |
| Lite pull request | `UNASSIGNED` |
| Lite reciprocal review | `UNASSIGNED` |
| Hosted Lite CI | `UNASSIGNED` |
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

## Local qualification

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
| Static boundary, forbidden-provider, merge-marker, diff, and staging scans | PASS; no external ingest module, no named-provider restriction, no merge marker, no whitespace error, staging empty |
| Local freeze scope | PASS; exactly 42 unstaged paths after this evidence file; no `.tgz` artifact |

The local machine has the Rust MSVC target but not Visual Studio Build Tools or
the Windows SDK linker, so a local MSVC invocation stops before compiling this
crate with `link.exe not found`. This is recorded as host capability absence,
not a pass. The mandatory `retrieval-rust-windows-msvc` hosted job performs
check, all-target tests with `GKOS_REQUIRE_ALIAS_FIXTURE=1`, and compile-fail
docs on `windows-latest`; its Phase 3 run remains `UNASSIGNED` until the
approved implementation is committed and pushed.

## Remaining publication gates

1. Full must complete read-only reciprocal review of this exact unstaged
   implementation and evidence freeze.
2. Only after explicit authorization may Lite create and push one signed DCO
   implementation commit and open/update its Phase 3 pull request.
3. Node 22/23/24, Rust latest/MSRV, Windows MSVC including the mandatory alias
   fixture, desktop, and desktop-native hosted jobs must all reach terminal
   success on that exact commit.
4. A later evidence-only closeout must distinguish the qualified implementation
   SHA from its own evidence head and record exact Lite run/job coordinates.

Until those gates complete, no merge, tag, release, deployment, npm/package
publication, or artifact publication is authorized or claimed.
