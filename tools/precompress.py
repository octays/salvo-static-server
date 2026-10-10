#!/usr/bin/env python3
"""Generate deterministic gzip (and optional Brotli) variants for StaticDir."""

import argparse
import gzip
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

EXTENSIONS = {".html", ".css", ".js", ".json", ".svg", ".xml", ".txt", ".wasm"}


def write_atomic(path, data):
    # Replace only after the full variant is written, so running servers see a complete file.
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as stream:
        temporary = Path(stream.name)
        try:
            stream.write(data)
            stream.flush()
            os.chmod(temporary, 0o644)
            os.replace(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", nargs="?", type=Path, default=Path("static"))
    parser.add_argument("--brotli", action="store_true", help="also generate .br using the brotli CLI")
    args = parser.parse_args()
    if not args.directory.is_dir():
        parser.error(f"not a directory: {args.directory}")
    brotli = shutil.which("brotli") if args.brotli else None
    if args.brotli and brotli is None:
        parser.error("--brotli requires the brotli CLI")

    count = original_size = gzip_size = 0
    for path in sorted(args.directory.rglob("*")):
        if path.is_symlink() or not path.is_file() or path.suffix.lower() not in EXTENSIONS:
            continue
        data = path.read_bytes()
        compressed = gzip.compress(data, compresslevel=9, mtime=0)
        write_atomic(path.with_name(path.name + ".gz"), compressed)
        if brotli:
            compressed_br = subprocess.run(
                [brotli, "--quality=11", "--stdout"], input=data, capture_output=True, check=True
            ).stdout
            write_atomic(path.with_name(path.name + ".br"), compressed_br)
        count += 1
        original_size += len(data)
        gzip_size += len(compressed)
    print(f"{count} files: {original_size:,} bytes -> {gzip_size:,} gzip bytes")


if __name__ == "__main__":
    main()
