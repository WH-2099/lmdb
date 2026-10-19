#!/usr/bin/env python3
"""Security and round-trip checks for the evidence bundle CLI."""

import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import evidence_bundle as bundle


class EvidenceBundleTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.snapshot = b'{"model": "example"}\n'
        self.document = b"# Official model card\n"
        self.snapshot_hash = hashlib.sha256(self.snapshot).hexdigest()
        self.document_hash = hashlib.sha256(self.document).hexdigest()
        self.manifest = {
            "schema_version": 1,
            "data_tree_sha256": "a" * 64,
            "data_files": {"catalogs/example.yaml": "b" * 64},
            "snapshots": [self.snapshot_hash],
            "documents": [self.document_hash],
        }
        self.entries = [
            ("manifest.json", json.dumps(self.manifest).encode()),
            (f"snapshots/{self.snapshot_hash}.json", self.snapshot),
            (f"documents/{self.document_hash}.bin", self.document),
        ]

    def make_tar(self, entries=None, format=tarfile.USTAR_FORMAT):
        target = self.root / "bundle.tar"
        with tarfile.open(target, "w", format=format) as archive:
            for name, contents in self.entries if entries is None else entries:
                header = name if isinstance(name, tarfile.TarInfo) else tarfile.TarInfo(name)
                header.size = len(contents)
                archive.addfile(header, io.BytesIO(contents))
        return target

    def unpack(self, path, digest=None):
        digest = digest or hashlib.sha256(path.read_bytes()).hexdigest()
        bundle.unpack(path, self.root / "restored", digest)

    def test_pack_roundtrip_is_deterministic_and_ignores_unreferenced_files(self):
        source = self.root / "source"
        source.mkdir()
        for name, contents in self.entries:
            target = source / name
            target.parent.mkdir(exist_ok=True)
            target.write_bytes(contents)
        (source / "documents" / "unrelated.bin").write_bytes(b"do not include")
        (source / "http").mkdir()
        (source / "http" / "receipt.json").write_bytes(b"sensitive receipt")
        first, second = self.root / "first.tar", self.root / "second.tar"
        result = subprocess.run([sys.executable, bundle.__file__, "pack", str(source), str(first)],
                                capture_output=True, text=True, check=True)
        digest = bundle.pack(source, second)
        self.assertEqual(first.read_bytes(), second.read_bytes())
        self.assertEqual(result.stdout.strip(), hashlib.sha256(first.read_bytes()).hexdigest())
        self.assertEqual(result.stdout.strip(), digest)
        self.unpack(first)
        for name, contents in self.entries:
            self.assertEqual((self.root / "restored" / name).read_bytes(), contents)
        self.assertFalse((self.root / "restored" / "http").exists())
        self.assertFalse((self.root / "restored" / "documents" / "unrelated.bin").exists())
        with self.assertRaises(FileExistsError):
            self.unpack(first)

    def test_package_sha_is_required_and_verified_before_tar_parsing(self):
        path = self.root / "invalid.tar"
        path.write_bytes(b"not even a tar archive")
        for digest, error in (("", "64 lowercase"), ("A" * 64, "64 lowercase"),
                              ("0" * 64, "archive SHA-256 mismatch")):
            with self.subTest(digest=digest), self.assertRaisesRegex(bundle.BundleError, error):
                bundle.unpack(path, self.root / "restored", digest)
        self.assertFalse((self.root / "restored").exists())

    def test_nonregular_input_is_rejected_without_opening_it(self):
        path = self.root / "pipe"
        os.mkfifo(path)
        with self.assertRaises(bundle.BundleError):
            bundle.read_regular(path, bundle.MAX_FILE)
        link = self.root / "link"
        link.symlink_to(path)
        with self.assertRaises(bundle.BundleError):
            bundle.read_regular(link, bundle.MAX_FILE)

    def test_traversal_absolute_and_unlisted_paths_are_rejected(self):
        for name in ("../escape", "/tmp/escape", "snapshots/../../escape", "http/receipt.json"):
            with self.subTest(name=name), self.assertRaises(bundle.BundleError):
                self.unpack(self.make_tar(self.entries + [(name, b"malicious")]))
        self.assertFalse((self.root / "restored").exists())

    def test_links_and_special_members_are_rejected(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.DIRTYPE, tarfile.FIFOTYPE,
                     tarfile.CHRTYPE, tarfile.BLKTYPE):
            header = tarfile.TarInfo(self.entries[1][0])
            header.type = kind
            header.linkname = "../../escape"
            with self.subTest(kind=kind), self.assertRaises(bundle.BundleError):
                self.unpack(self.make_tar([self.entries[0], (header, b""), self.entries[2]]))

    def test_duplicate_missing_extra_and_corrupt_members_are_rejected(self):
        examples = [self.entries + [self.entries[1]], self.entries[:-1],
                    self.entries + [("documents/" + "c" * 64 + ".bin", b"extra")],
                    [self.entries[0], (self.entries[1][0], b"corrupt"), self.entries[2]]]
        for entries in examples:
            with self.subTest(entries=entries), self.assertRaises(bundle.BundleError):
                self.unpack(self.make_tar(entries))
        self.assertFalse((self.root / "restored").exists())

    def test_pax_gnu_compression_and_trailing_archive_are_rejected(self):
        pax_header = tarfile.TarInfo("manifest.json")
        pax_header.pax_headers = {"comment": "extension"}
        with self.assertRaises(bundle.BundleError):
            self.unpack(self.make_tar([(pax_header, self.entries[0][1])] + self.entries[1:],
                                      format=tarfile.PAX_FORMAT))
        with self.assertRaises(bundle.BundleError):
            self.unpack(self.make_tar(format=tarfile.GNU_FORMAT))
        path = self.make_tar()
        path.write_bytes(path.read_bytes() + path.read_bytes())
        with self.assertRaises(bundle.BundleError):
            self.unpack(path)
        compressed = self.root / "compressed.tar.gz"
        with tarfile.open(compressed, "w:gz") as archive:
            archive.add(path, arcname="bundle.tar")
        with self.assertRaises(bundle.BundleError):
            self.unpack(compressed)

    def test_gnu_numeric_encoding_with_ustar_magic_is_rejected(self):
        path = self.make_tar()
        contents = bytearray(path.read_bytes())
        contents[124:136] = b"\x80" + len(self.entries[0][1]).to_bytes(11, "big")
        contents[148:156] = b"        "
        contents[148:156] = f"{sum(contents[:512]):06o}\0 ".encode()
        path.write_bytes(contents)
        with self.assertRaisesRegex(bundle.BundleError, "USTAR octal"):
            self.unpack(path)

    def test_manifest_shape_and_duplicate_keys_are_rejected(self):
        invalid = [dict(self.manifest, unexpected=True), dict(self.manifest, schema_version=True),
                   dict(self.manifest, snapshots=[self.snapshot_hash, self.snapshot_hash]),
                   dict(self.manifest, data_files={"../escape": "a" * 64})]
        for manifest in invalid:
            with self.subTest(manifest=manifest), self.assertRaises(bundle.BundleError):
                bundle.parse_manifest(json.dumps(manifest).encode())
        with self.assertRaises(bundle.BundleError):
            bundle.parse_manifest(b'{"schema_version": 1, "schema_version": 1}')

    def test_size_and_member_limits_are_enforced_before_extraction(self):
        path = self.make_tar()
        limits = [("MAX_ARCHIVE", len(path.read_bytes()) - 1), ("MAX_MANIFEST", 1),
                  ("MAX_FILE", 1), ("MAX_MEMBERS", 2)]
        for setting, value in limits:
            with self.subTest(setting=setting), patch.object(bundle, setting, value):
                with self.assertRaises(bundle.BundleError):
                    self.unpack(path)
        self.assertFalse((self.root / "restored").exists())

    def test_url_validation_does_not_echo_credentials(self):
        for url in ("https://example.com/bundle.tar", "https://example.com:443/bundle.tar?q=token",
                    "https://[::1]/bundle.tar"):
            bundle.validate_url(url)
        for url in ("http://example.com/bundle.tar", "https://user:SECRET@example.com/x",
                    "https://example.com:444/x", "https://example.com/x#fragment",
                    "https://example.com/x#", "https://example.com/\nx", "https:///x",
                    "https://bad%20host/x", "https://example.com\\@elsewhere/x"):
            with self.subTest(url=url), self.assertRaisesRegex(bundle.BundleError, "invalid bundle URL"):
                bundle.validate_url(url)
        result = subprocess.run([sys.executable, bundle.__file__, "validate-url",
                                 "https://user:SECRET@example.com/x"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertNotIn("SECRET", result.stdout + result.stderr)
        self.assertNotIn("example.com", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
