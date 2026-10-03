# Native release dependency inventories (#1128)

The six-target `rust-prerelease.yml` candidate builds with pinned
`cargo-auditable 0.7.7 --locked`. Each actual Rust executable embeds its compiled
dependency graph in `.dep-v0` (ELF/PE) or the corresponding Mach-O section.
Pinned `rust-audit-info 0.5.4 --locked` reads those bytes; no source-only
inventory is substituted. Native host, linkage, version/help and the actual
Go-fallback dispatch probes remain mandatory on each architecture.

The generator reads native Go `version -m` output, including dependencies,
effective replacements, module checksums, target, exact source revision and
`vcs.modified=false`. It canonicalizes only the first-line executable pathname.
The CycloneDX 1.6 documents bind the archive and both executables by SHA-256,
preserve Cargo dependency edges and attach the measured source revision. Go's
embedded inventory reports a module set, so only root-to-module edges are
asserted; no unmeasured transitive Go edges are invented.

`verify_rust_artifact_sbom.py` independently reads the two explicit executable
members from each packed archive into a fresh temporary directory, using
bounded streaming and fixed filenames. It extracts the Rust inventory again,
reads Go build info again from the packed executable, then reproduces the
entire SBOM. Archive/member/checksum validation and exact comparison reject
changed binaries, dependency lists, source identities or SBOM bytes.
`checksums.txt` retains its exact six-archive format; `sbom-checksums.txt`
contains exactly the eighteen inventory/SBOM sidecars.

Local verification on Linux amd64: six controls pass, including mismatched
packed/unpacked bytes, malformed/cyclic/root-invalid dependency graphs and
effective Go replacement identities. A separately compiled auditable native
ELF smoke executable yielded six actual embedded packages, accepted by the
same extractor and graph validator. This smoke is a tool compatibility check;
it is not a six-target product release result.

At clean source `aa2dd0f31d30061b5ed66e7638a0dcddde060009`, a native
Linux amd64 debug build of the actual `symeraseme-cli 0.13.0` yielded 229
embedded packages through the same pinned recorder/extractor and validator.
Its binary SHA-256 is
`a754ddd0a6dcf0f2487353abda684242772cc1bd462f04fafa897281fa849180`;
the extracted inventory SHA-256 is
`7cd26fd98fabbc36d31db51ab4939d5dcb30a860a4ff83189be8dea3df86cb83`.
The compiler reports native `x86_64-unknown-linux-gnu`, Rust 1.98.0. This
validates the extractor on the nontrivial product graph; it does not establish
release-mode musl linkage or other native targets. The workflow additionally
requires the actual Go fallback's version JSON to match the Rust release's
tool name, version and schema before comparing fallback dispatch output.

REL-010 remains PARTIAL until native product archive execution, signatures,
provenance, current audit/deny results and independently verified published
assets exist. REL-001..005 retain their existing status. Release routing,
Homebrew publication, GUI/notarization and default-backend cutover still need
their dependency-ready implementation and concrete native evidence.
