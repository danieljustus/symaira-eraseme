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

REL-010 remains PARTIAL until native product archive execution, signatures,
provenance, current audit/deny results and independently verified published
assets exist. REL-001..005 retain their existing status. Release routing,
Homebrew publication, GUI/notarization and default-backend cutover still need
their dependency-ready implementation and concrete native evidence.
