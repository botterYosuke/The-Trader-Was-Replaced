#!/usr/bin/env bash
# check-design-system.sh — Issue #46 Slice H
#
# src/ui/**/*.rs を走査して生の Color 呼び出しを検出する。
# 0 件で exit 0、1 件以上で件数を表示して exit 1。
#
# 検査対象パターン:
#   Color::srgb(   Color::srgba(   Color::rgb(   Color::rgba(
#
# 使い方:
#   bash scripts/check-design-system.sh
#
# CI での組み込み例 (GitHub Actions):
#   - name: Check design system anti-patterns
#     run: bash scripts/check-design-system.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
UI_DIR="${REPO_ROOT}/src/ui"

PATTERNS=(
  "Color::srgb("
  "Color::srgba("
  "Color::rgb("
  "Color::rgba("
)

total=0
found_files=()

for pattern in "${PATTERNS[@]}"; do
  # grep -r: recursive, -n: line numbers, -l: only filenames for counting
  while IFS= read -r line; do
    echo "  $line" >&2
    ((total++)) || true
  done < <(grep -rn --include="*.rs" --exclude-dir=theme -F "$pattern" "$UI_DIR" 2>/dev/null || true)
done

if [[ $total -eq 0 ]]; then
  echo "✓ check-design-system: no raw Color literals found in src/ui/**" >&1
  exit 0
else
  echo "" >&2
  echo "✗ check-design-system: found ${total} raw Color literal(s) in src/ui/**" >&2
  echo "  → Replace with theme tokens (see docs/ui-theme.md §8)" >&2
  exit 1
fi
