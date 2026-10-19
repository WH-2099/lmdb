#!/usr/bin/env python3
"""Package and safely unpack the exact files named by an evidence manifest."""

import hashlib
import io
import ipaddress
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import sys
import tarfile
from urllib.parse import urlsplit


MAX_FILE = 64 * 1024 * 1024
MAX_MANIFEST = 4 * 1024 * 1024
MAX_ARCHIVE = 256 * 1024 * 1024
MAX_MEMBERS = 10_000
HASH = re.compile(r"[0-9a-f]{64}")
MEMBER = re.compile(r"(?:snapshots/[0-9a-f]{64}\.json|documents/[0-9a-f]{64}\.bin)")
MANIFEST_FIELDS = {
    "schema_version", "data_tree_sha256", "data_files", "snapshots", "documents"
}


class BundleError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise BundleError(message)


def is_hash(value):
    return isinstance(value, str) and HASH.fullmatch(value) is not None


def read_regular(path, limit):
    metadata = path.lstat()
    require(stat.S_ISREG(metadata.st_mode), "input must be a regular file, without symbolic links")
    require(metadata.st_size <= limit, "file exceeds its size limit")
    with path.open("rb") as source:
        metadata = os.fstat(source.fileno())
        require(stat.S_ISREG(metadata.st_mode), "input must be a regular file")
        require(metadata.st_size <= limit, "file exceeds its size limit")
        contents = source.read(limit + 1)
    require(len(contents) <= limit, "file exceeds its size limit")
    return contents


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate manifest object key")
        result[key] = value
    return result


def parse_manifest(contents):
    require(len(contents) <= MAX_MANIFEST, "manifest exceeds the 4 MiB limit")
    try:
        manifest = json.loads(contents, object_pairs_hook=unique_object)
    except (UnicodeError, ValueError, RecursionError) as error:
        raise BundleError("invalid JSON manifest") from error
    require(isinstance(manifest, dict) and set(manifest) == MANIFEST_FIELDS,
            "manifest must contain exactly the supported fields")
    require(type(manifest["schema_version"]) is int and manifest["schema_version"] == 1,
            "unsupported manifest schema version")
    require(is_hash(manifest["data_tree_sha256"]), "invalid data tree SHA-256")
    data_files = manifest["data_files"]
    require(isinstance(data_files, dict), "data_files must be an object")
    for name, digest in data_files.items():
        require(isinstance(name, str) and name and "\\" not in name
                and not any(ord(char) < 32 or ord(char) == 127 for char in name)
                and not PurePosixPath(name).is_absolute()
                and all(part not in ("", ".", "..") for part in name.split("/")),
                "invalid data file path")
        require(is_hash(digest), "invalid data file SHA-256")
    expected = {"manifest.json": None}
    for directory, extension in (("snapshots", "json"), ("documents", "bin")):
        hashes = manifest[directory]
        require(isinstance(hashes, list) and all(is_hash(value) for value in hashes),
                "manifest file lists must contain SHA-256 values")
        require(len(set(hashes)) == len(hashes), "duplicate manifest file hash")
        expected.update((f"{directory}/{digest}.{extension}", digest) for digest in hashes)
    require(len(expected) <= MAX_MEMBERS, "too many archive members")
    return expected


def verify_contents(entries, expected):
    require(set(entries) == set(expected), "archive members do not match the manifest")
    for name, digest in expected.items():
        if digest is not None:
            require(hashlib.sha256(entries[name]).hexdigest() == digest,
                    "evidence content SHA-256 mismatch")


def pack(archive_directory, output_tar):
    root = Path(archive_directory)
    require(root.is_dir() and not root.is_symlink(), "archive source must be a directory")
    manifest = read_regular(root / "manifest.json", MAX_MANIFEST)
    expected = parse_manifest(manifest)
    entries = {"manifest.json": manifest}
    total_size = 512 + ((len(manifest) + 511) // 512) * 512 + 1024
    for name in sorted(set(expected) - {"manifest.json"}):
        require(not (root / name).parent.is_symlink(), "symbolic links are not allowed")
        contents = read_regular(root / name, MAX_FILE)
        total_size += 512 + ((len(contents) + 511) // 512) * 512
        require(total_size <= MAX_ARCHIVE, "archive exceeds the 256 MiB limit")
        entries[name] = contents
    total_size = ((total_size + tarfile.RECORDSIZE - 1) // tarfile.RECORDSIZE) * tarfile.RECORDSIZE
    require(total_size <= MAX_ARCHIVE, "archive exceeds the 256 MiB limit")
    verify_contents(entries, expected)
    with Path(output_tar).open("x+b") as output:
        with tarfile.open(fileobj=output, mode="w", format=tarfile.USTAR_FORMAT) as archive:
            for name, contents in entries.items():
                header = tarfile.TarInfo(name)
                header.size = len(contents)
                header.mode = 0o644
                header.mtime = 0
                archive.addfile(header, io.BytesIO(contents))
        output.seek(0)
        return hashlib.file_digest(output, "sha256").hexdigest()


def read_tar(contents):
    require(len(contents) % 512 == 0, "invalid tar block size")
    entries = {}
    view = memoryview(contents)
    offset = 0
    while offset + 512 <= len(contents):
        block = contents[offset:offset + 512]
        if block == bytes(512):
            require(len(contents) - offset >= 1024 and not any(view[offset:]),
                    "invalid tar end marker or trailing content")
            return entries
        require(len(entries) < MAX_MEMBERS, "too many archive members")
        # Check physical headers before tarfile can consume PAX or GNU extensions.
        require(block[257:265] == b"ustar\x0000" and block[156:157] == b"0",
                "only USTAR regular file members are allowed")
        for start, end in ((100, 108), (108, 116), (116, 124), (124, 136),
                           (136, 148), (148, 156), (329, 337), (337, 345)):
            require(all(char in b"01234567\x00 " for char in block[start:end]),
                    "only USTAR octal numeric fields are allowed")
        try:
            header = tarfile.TarInfo.frombuf(block, "utf-8", "strict")
        except (tarfile.HeaderError, UnicodeError, ValueError) as error:
            raise BundleError("invalid USTAR header") from error
        name = header.name
        require(not header.linkname and (name == "manifest.json" or MEMBER.fullmatch(name)),
                "unexpected archive member path")
        require(name not in entries, "duplicate archive member")
        limit = MAX_MANIFEST if name == "manifest.json" else MAX_FILE
        require(0 <= header.size <= limit, "archive member exceeds its size limit")
        start = offset + 512
        end = start + header.size
        offset = start + ((header.size + 511) // 512) * 512
        require(offset <= len(contents), "truncated archive member")
        require(not any(view[end:offset]), "invalid tar member padding")
        entries[name] = view[start:end]
    raise BundleError("missing tar end marker")


def unpack(input_tar, output_directory, expected_sha256):
    require(is_hash(expected_sha256), "expected SHA-256 must be 64 lowercase hexadecimal characters")
    contents = read_regular(Path(input_tar), MAX_ARCHIVE)
    require(hashlib.sha256(contents).hexdigest() == expected_sha256, "archive SHA-256 mismatch")
    entries = read_tar(contents)
    require("manifest.json" in entries, "missing manifest.json")
    expected = parse_manifest(bytes(entries["manifest.json"]))
    verify_contents(entries, expected)
    destination = Path(output_directory)
    destination.mkdir()
    try:
        for name, contents in entries.items():
            target = destination / name
            if name != "manifest.json":
                target.parent.mkdir(exist_ok=True)
            with target.open("xb") as output:
                output.write(contents)
    except BaseException:
        shutil.rmtree(destination)
        raise


def validate_url(url):
    try:
        require(not any(ord(char) <= 32 or ord(char) == 127 for char in url)
                and "\\" not in url and "#" not in url, "invalid bundle URL")
        parsed = urlsplit(url)
        require(parsed.scheme == "https" and parsed.hostname and parsed.username is None
                and parsed.password is None and parsed.port in (None, 443), "invalid bundle URL")
        host = parsed.hostname
        require("%" not in host, "invalid bundle URL")
        try:
            ipaddress.ip_address(host)
        except ValueError:
            host = host.encode("idna").decode("ascii").removesuffix(".")
            require(len(host) <= 253 and all(
                re.fullmatch(r"[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?", label)
                for label in host.split(".")), "invalid bundle URL")
    except (ValueError, UnicodeError):
        raise BundleError("invalid bundle URL") from None


def main(arguments):
    try:
        if len(arguments) == 3 and arguments[0] == "pack":
            print(pack(*arguments[1:]))
        elif len(arguments) == 4 and arguments[0] == "unpack":
            unpack(*arguments[1:])
        elif len(arguments) == 2 and arguments[0] == "validate-url":
            validate_url(arguments[1])
        else:
            raise BundleError("usage: evidence_bundle.py pack ARCHIVE_DIRECTORY OUTPUT_TAR | "
                              "unpack INPUT_TAR OUTPUT_DIRECTORY EXPECTED_SHA256 | validate-url URL")
    except (BundleError, OSError) as error:
        print(f"evidence bundle: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
