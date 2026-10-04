"""Read bounded inline Go build metadata without installing or executing Go.

The retained Go 1.26.6 release uses the documented inline build-info format.
Older pointer-based formats are rejected. Artifact identity and native target
must be verified separately; metadata alone does not establish source identity.
"""
import hashlib
import json
import pathlib

MAGIC = b"\xff Go buildinf:"
MODULE_START = bytes.fromhex("3077af0c9274080241e1c107e6d618e6")
MODULE_END = bytes.fromhex("f932433186182072008242104116d8f2")
MANIFEST = pathlib.Path(__file__).resolve().parents[2] / "tests/fixtures/go-rollback/v0.12.1.json"
MANIFEST_SHA256 = "98cc9e2b0ec1e9c9d363363cef8302e806235294048d611b2271a3d5538b162f"


def release_manifest():
    raw = MANIFEST.read_bytes()
    assert len(raw) <= 16384 and hashlib.sha256(raw).hexdigest() == MANIFEST_SHA256, "actual published release manifest changed"
    document = json.loads(raw)
    assert document["schema"] == "symeraseme.actual-published-go-release.v1"
    assert document["tag"] == "v0.12.1" and document["go_version"] == "go1.26.6"
    assert document["source_revision"] == "240bf67cefa05e643e32611a02e6e7ed87a033ea"
    assert len(document["native_archives"]) == 6
    return document


def verify_release(path, native_target):
    """Verify the actual last-release binary identity before using its metadata."""
    document = release_manifest()
    records = [r for r in document["native_archives"] if r["native_target"] == native_target]
    assert len(records) == 1, "unrecorded retained release target"
    record = records[0]
    path = pathlib.Path(path)
    assert path.is_file() and not path.is_symlink(), "regular retained binary required"
    assert path.stat().st_size == record["binary_bytes"]
    assert hashlib.sha256(path.read_bytes()).hexdigest() == record["binary_sha256"], "published binary changed"
    metadata = read(path)
    assert metadata["native_target"] == native_target, "retained binary is not native"
    assert metadata["source_revision"] == document["source_revision"]
    assert hashlib.sha256(metadata["module"].encode()).hexdigest() == record["module_sha256"]
    return metadata, {"tag": document["tag"], "source_revision": document["source_revision"],
                      "archive": record, "published_checksums": document["checksums_asset"]}


def read(path):
    path = pathlib.Path(path)
    assert path.is_file() and not path.is_symlink(), "regular Go artifact required"
    assert 0 < path.stat().st_size <= 32 * 1024 * 1024, "bounded Go artifact"
    data = path.read_bytes()
    offset = data.find(MAGIC)
    assert offset >= 0 and offset % 16 == 0, "aligned Go build-info header missing"
    assert data[offset + 14:offset + 32] == bytes([8, 2]) + bytes(16), "unsupported Go build-info format"
    cursor = offset + 32

    def string(limit):
        nonlocal cursor
        length = 0
        for shift in range(0, 70, 7):
            assert cursor < len(data), "truncated build-info length"
            byte = data[cursor]
            cursor += 1
            assert shift != 63 or byte <= 1, "overflowing build-info length"
            length |= (byte & 127) << shift
            if byte < 128:
                break
        else:
            raise AssertionError("unterminated build-info length")
        assert 0 < length <= limit and cursor + length <= len(data), "bounded complete build-info string"
        value = data[cursor:cursor + length]
        cursor += length
        return value

    version = string(128).decode("ascii")
    assert version == "go1.26.6", "unexpected retained release toolchain"
    framed = string(1024 * 1024)
    assert framed.startswith(MODULE_START) and framed.endswith(MODULE_END), "invalid module-info frame"
    module = framed[16:-16].decode("utf-8")
    assert module.endswith("\n"), "complete module metadata required"
    lines = module.splitlines()
    assert lines[0] == "path\tgithub.com/danieljustus/symaira-eraseme/cmd/symeraseme", "unexpected Go entrypoint"
    assert lines[1].startswith("mod\tgithub.com/danieljustus/symaira-eraseme\t"), "unexpected Go module"
    settings = {}
    for line in lines:
        if line.startswith("build\t"):
            key, value = line[6:].split("=", 1)
            assert key not in settings, "duplicate Go build setting"
            settings[key] = value
    assert settings["CGO_ENABLED"] == "0", "retained Go artifact must be CGO-free"
    assert settings["GOOS"] in ("darwin", "linux", "windows")
    assert settings["GOARCH"] in ("amd64", "arm64")
    assert settings["vcs"] == "git" and settings["vcs.modified"] == "false", "clean Git artifact required"
    revision = settings["vcs.revision"]
    assert len(revision) == 40 and all(c in "0123456789abcdef" for c in revision)
    return {"version": version, "module": module, "settings": settings,
            "source_revision": revision, "native_target": settings["GOOS"] + "/" + settings["GOARCH"],
            "sdk_style_output": str(path) + ": " + version + "\n" + "".join("\t" + line + "\n" for line in lines)}
