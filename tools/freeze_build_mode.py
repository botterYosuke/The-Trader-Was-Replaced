#!/usr/bin/env python3
"""Freeze _build_mode.IS_DEBUG_BUILD=False for release packaging.

Usage: python tools/freeze_build_mode.py --release
       python tools/freeze_build_mode.py --debug   (restore dev default)
"""
import argparse
import re
from pathlib import Path

ROOT = Path(__file__).parent.parent
BUILD_MODE_FILE = ROOT / "python" / "engine" / "live" / "_build_mode.py"


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--release", action="store_true")
    group.add_argument("--debug", action="store_true")
    args = parser.parse_args()

    text = BUILD_MODE_FILE.read_text(encoding="utf-8")
    if args.release:
        new_text = re.sub(r"IS_DEBUG_BUILD\s*=\s*True", "IS_DEBUG_BUILD = False", text)
    else:
        new_text = re.sub(r"IS_DEBUG_BUILD\s*=\s*False", "IS_DEBUG_BUILD = True", text)
    BUILD_MODE_FILE.write_text(new_text, encoding="utf-8")
    flag = "False (release)" if args.release else "True (debug)"
    print(f"_build_mode.IS_DEBUG_BUILD = {flag}")


if __name__ == "__main__":
    main()
