# Full pin/tag/main comparison — September 14, 2026

Status: HOLD for complete behavioral equivalence; source comparison only.
No repin is made. Matching parser bytes do not prove CLI or artifact equality.

| Coordinate | Package | Parser SHA-256 | CLI SHA-256 |
| --- | --- | --- | --- |
| `e7cc0dd478af3d0bda216c5258dec5f77932def7` | 2.1.2 | `6232618d9e964d7e96f17daef714726c3c1ca35ec325c87e80dd1c21222592ef` | `333bfff7455c94d1f98f4490adfb3b09d8a9511211b953b2b156f9c186d294a2` |
| `7bf14b481e78c5ae9d1e14661602be4f24559d0e` | 2.1.2 | `6232618d9e964d7e96f17daef714726c3c1ca35ec325c87e80dd1c21222592ef` | `2fee824856d7f8e2505f7406d1a937d10d4b4d79188debe5d7209fed4bda37a1` |
| `91433b158b4b9bc30bd4d43b7dd8f61363647960` | 2.2.0 | `6232618d9e964d7e96f17daef714726c3c1ca35ec325c87e80dd1c21222592ef` | `97805319a12d92460887200226cde405c3429558e240572caf527d7cae7f4d29` |

Rows are respectively the current Lite dependency pin, Full v2.1.2 tag target,
and inspected Full main. Generated using `git show <commit>:<path>` and SHA-256.
The parser hashes match; the three CLI hashes differ. Current Full source
identifies itself as 2.2.0 while the historical pin and tag identify as 2.1.2.

A complete dossier still requires isolated builds at all three coordinates,
identical command fixtures for validate/index/assess/search/graph/export,
exit/stdout/stderr and output-file comparison, source-byte preservation,
capability-refusal cases, packed-artifact inventories and Rust bridge evidence.
Compare producer-version changes explicitly; do not normalize arbitrary
result differences away. The Lite wrapper's own help and refusal surfaces
must be assessed separately from delegated Full output.

Current Lite hosted CI passed at
`7e7f1fbee4034d2edd97d3d8b0d75da9bf95921b`:
[run 34737925269](https://github.com/Odenknight/GKOS-Engine-Lite/actions/runs/34737925269).
This does not establish cross-version equivalence or installer qualification.
See [current capabilities](CURRENT_CAPABILITIES.md) and
[PR disposition ledger](PR-DISPOSITION-20260914.md).
