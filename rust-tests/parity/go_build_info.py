"""Read bounded inline Go build metadata without installing or executing Go.

The retained Go 1.26.6 releases use the documented inline build-info format.
Older pointer-based formats are rejected. Artifact identity and native target
must be verified separately; metadata alone does not establish source identity.
Release tags are an allowlist; each manifest is pinned by its own SHA-256 digest
and source revision here, not by values read from the manifest itself.
"""
import hashlib
import json
import pathlib

MAGIC = b"\xff Go buildinf:"
MODULE_START = bytes.fromhex("3077af0c9274080241e1c107e6d618e6")
MODULE_END = bytes.fromhex("f932433186182072008242104116d8f2")
FIXTURE_DIR = pathlib.Path(__file__).resolve().parents[2] / "tests/fixtures/go-rollback"
DEFAULT_TAG = "v0.12.1"
RELEASES = {
    "v0.12.1": {"manifest_sha256": "98cc9e2b0ec1e9c9d363363cef8302e806235294048d611b2271a3d5538b162f",
                "source_revision": "240bf67cefa05e643e32611a02e6e7ed87a033ea", "member": "symeraseme"},
    # v0.13.0 archives also carry the Rust CLI as "symeraseme"; the Go binary is "symeraseme-go".
    "v0.13.0": {"manifest_sha256": "987815ffeb0a7a89b628395100530829d27ac4e68b5cb8ac5bbbd3e0e8c3302d",
                "source_revision": "780512f17d4dd434676d62539f81550ce048f77e", "member": "symeraseme-go"},
}
SUPPORTED_TAGS = tuple(RELEASES)


def require(condition, message="invalid retained Go artifact"):
    # Integrity checks must remain active under python -O and PYTHONOPTIMIZE.
    if not condition:
        raise AssertionError(message)


def release_manifest(tag=DEFAULT_TAG):
    require(tag in RELEASES, "unsupported retained release tag")
    pin = RELEASES[tag]
    raw = (FIXTURE_DIR / (tag + ".json")).read_bytes()
    require(len(raw) <= 16384 and hashlib.sha256(raw).hexdigest() == pin["manifest_sha256"], "actual published release manifest changed")
    document = json.loads(raw)
    require(document["schema"] == "symeraseme.actual-published-go-release.v1")
    require(document["tag"] == tag and document["go_version"] == "go1.26.6")
    require(document["source_revision"] == pin["source_revision"])
    require(len(document["native_archives"]) == 6)
    require(len({r["native_target"] for r in document["native_archives"]}) == 6)
    for record in document["native_archives"]:
        expected = binary_member(tag, record["native_target"])
        require(record.get("binary_member", expected) == expected, "unexpected Go binary member")
    return document


def binary_member(tag, native_target):
    """Archive member (and extracted file name) of the Go binary for a tag and target."""
    require(tag in RELEASES, "unsupported retained release tag")
    base = RELEASES[tag]["member"]
    return base + ".exe" if native_target.startswith("windows/") else base


def verify_release(path, native_target, tag=DEFAULT_TAG):
    """Verify the actual release binary identity before using its metadata."""
    document = release_manifest(tag)
    records = [r for r in document["native_archives"] if r["native_target"] == native_target]
    require(len(records) == 1, "unrecorded retained release target")
    record = records[0]
    path = pathlib.Path(path)
    require(path.is_file() and not path.is_symlink(), "regular retained binary required")
    require(path.stat().st_size == record["binary_bytes"])
    require(hashlib.sha256(path.read_bytes()).hexdigest() == record["binary_sha256"], "published binary changed")
    metadata = read(path)
    require(metadata["native_target"] == native_target, "retained binary is not native")
    require(metadata["source_revision"] == document["source_revision"])
    require(hashlib.sha256(metadata["module"].encode()).hexdigest() == record["module_sha256"])
    return metadata, {"tag": document["tag"], "source_revision": document["source_revision"],
                      "archive": record, "published_checksums": document["checksums_asset"]}


def read(path):
    path = pathlib.Path(path)
    require(path.is_file() and not path.is_symlink(), "regular Go artifact required")
    require(0 < path.stat().st_size <= 32 * 1024 * 1024, "bounded Go artifact")
    data = path.read_bytes()
    offset = data.find(MAGIC)
    require(offset >= 0 and offset % 16 == 0, "aligned Go build-info header missing")
    require(data[offset + 14:offset + 32] == bytes([8, 2]) + bytes(16), "unsupported Go build-info format")
    cursor = offset + 32

    def string(limit):
        nonlocal cursor
        length = 0
        for shift in range(0, 70, 7):
            require(cursor < len(data), "truncated build-info length")
            byte = data[cursor]
            cursor += 1
            require(shift != 63 or byte <= 1, "overflowing build-info length")
            length |= (byte & 127) << shift
            if byte < 128:
                break
        else:
            raise AssertionError("unterminated build-info length")
        require(0 < length <= limit and cursor + length <= len(data), "bounded complete build-info string")
        value = data[cursor:cursor + length]
        cursor += length
        return value

    version = string(128).decode("ascii")
    require(version == "go1.26.6", "unexpected retained release toolchain")
    framed = string(1024 * 1024)
    require(framed.startswith(MODULE_START) and framed.endswith(MODULE_END), "invalid module-info frame")
    module = framed[16:-16].decode("utf-8")
    require(module.endswith("\n"), "complete module metadata required")
    lines = module.splitlines()
    require(lines[0] == "path\tgithub.com/danieljustus/symaira-eraseme/cmd/symeraseme", "unexpected Go entrypoint")
    require(lines[1].startswith("mod\tgithub.com/danieljustus/symaira-eraseme\t"), "unexpected Go module")
    settings = {}
    for line in lines:
        if line.startswith("build\t"):
            key, value = line[6:].split("=", 1)
            require(key not in settings, "duplicate Go build setting")
            settings[key] = value
    require(settings["CGO_ENABLED"] == "0", "retained Go artifact must be CGO-free")
    require(settings["GOOS"] in ("darwin", "linux", "windows"))
    require(settings["GOARCH"] in ("amd64", "arm64"))
    require(settings["vcs"] == "git" and settings["vcs.modified"] == "false", "clean Git artifact required")
    revision = settings["vcs.revision"]
    require(len(revision) == 40 and all(c in "0123456789abcdef" for c in revision))
    return {"version": version, "module": module, "settings": settings,
            "source_revision": revision, "native_target": settings["GOOS"] + "/" + settings["GOARCH"],
            "sdk_style_output": str(path) + ": " + version + "\n" + "".join("\t" + line + "\n" for line in lines)}
