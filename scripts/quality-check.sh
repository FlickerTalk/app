#!/usr/bin/env bash
set -eu

base="${1:-origin/main}"
max_file_lines=500

cd "$(git rev-parse --show-toplevel)"
root="$(pwd -P)"
merge_base="$(git merge-base "$base" HEAD)"

changed="$(
  {
    git diff --name-only --diff-filter=d "$merge_base"
    git ls-files --others --exclude-standard
  } | grep -E '\.(rs|ts|vue)$' | grep -v '^src-tauri/gen/' | sort -u
)"

if [ -z "$changed" ]; then
  echo "Quality check passed: no Rust, TypeScript or Vue file changed."
  exit 0
fi

problems=""

report() {
  problems="$problems$1"$'\n'
}

is_changed() {
  grep -qxF -- "$1" <<< "$changed"
}

if grep -q '\.rs$' <<< "$changed"; then
  fmt_status=0
  unformatted="$(cargo fmt --all -- --check -l)" || fmt_status=$?
  if [ "$fmt_status" -ne 0 ] && [ -z "$unformatted" ]; then
    echo "cargo fmt failed" >&2
    exit 1
  fi
  while IFS= read -r path; do
    file="${path#"$root/"}"
    if [ -n "$file" ] && is_changed "$file"; then
      report "$file: not formatted, run cargo fmt"
    fi
  done <<< "$unformatted"

  clippy_status=0
  # The limits live in scripts/clippy.toml, not at the root: with a threshold set, excessive_nesting
  # warns by default and would fail a plain cargo clippy -D warnings on old code.
  # The output is parsed below, so colour must stay off even where CARGO_TERM_COLOR=always (as in CI).
  clippy="$(CLIPPY_CONF_DIR="$root/scripts" cargo clippy --workspace --all-targets --locked \
    --color never --message-format=short \
    -- -W clippy::too_many_lines -W clippy::excessive_nesting 2>&1)" || clippy_status=$?
  if [ "$clippy_status" -ne 0 ]; then
    echo "$clippy" >&2
    echo "cargo clippy failed" >&2
    exit 1
  fi
  while IFS= read -r line; do
    case "$line" in
      *": warning: this function has too many lines"* | *": warning: this block is too nested"*) ;;
      *) continue ;;
    esac
    file="${line%%:*}"
    position="${line#*:}"
    if is_changed "$file"; then
      report "$file:${position%%:*}: ${line#*: warning: }"
    fi
  done <<< "$clippy"
fi

if grep -qE '\.(ts|vue)$' <<< "$changed"; then
  lint="$(grep -E '\.(ts|vue)$' <<< "$changed" | node --input-type=module -e '
    import { readFileSync } from "node:fs";
    import { relative } from "node:path";
    import { ESLint } from "eslint";
    const files = readFileSync(0, "utf8").split("\n").filter(Boolean);
    for (const { filePath, messages } of await new ESLint({ warnIgnored: false }).lintFiles(files)) {
      for (const { line, message } of messages) console.log(`${relative(".", filePath)}:${line}: ${message}`);
    }
  ')" || {
    echo "eslint failed" >&2
    exit 1
  }
  if [ -n "$lint" ]; then
    report "$lint"
  fi
fi

while IFS= read -r file; do
  case "$file" in
    tests/* | */tests/* | e2e/* | */__tests__/* | *.test.ts) continue ;;
  esac
  lines="$(wc -l < "$file" | tr -d ' ')"
  if [ "$lines" -le "$max_file_lines" ]; then
    continue
  fi
  if git cat-file -e "$merge_base:$file" 2> /dev/null; then
    old_lines="$(git show "$merge_base:$file" | wc -l | tr -d ' ')"
    if [ "$lines" -gt "$old_lines" ]; then
      report "$file: grew from $old_lines to $lines lines; a file over $max_file_lines lines may not grow"
    fi
  else
    report "$file: new file has $lines lines; the limit is $max_file_lines"
  fi
done <<< "$changed"

if [ -n "$problems" ]; then
  printf '%s' "$problems" | sort -u
  exit 1
fi
echo "Quality check passed."
