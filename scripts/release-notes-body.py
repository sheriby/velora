#!/usr/bin/env python3
"""本地发布说明保留 H1；GitHub 页面由 Release 标题提供同一层标题。"""

import argparse
from pathlib import Path
import sys


def release_body(notes: bytes) -> bytes:
    heading, _, body = notes.partition(b"\n")
    return body.lstrip(b"\r\n") if heading.startswith(b"# ") else notes


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("notes_file", type=Path)
    arguments = parser.parse_args()
    sys.stdout.buffer.write(release_body(arguments.notes_file.read_bytes()))
