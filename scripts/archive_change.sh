#!/bin/bash
set -euo pipefail

# Archive a completed specmark change.
# Usage: archive_change.sh <name> [--sync] [--date YYYY-MM-DD]

NAME="${1:?Usage: archive_change.sh <name> [--sync] [--date YYYY-MM-DD]}"
SYNC=false
DATE=$(date -u +%Y-%m-%d)

shift
while [[ $# -gt 0 ]]; do
    case "$1" in
        --sync) SYNC=true; shift ;;
        --date) DATE="$2"; shift 2 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

ARCHIVE_ROOT="specmark/archive"
CHANGES_DIR="specmark/changes"
TARGET="${ARCHIVE_ROOT}/${DATE}-${NAME}"
SOURCE="${CHANGES_DIR}/${NAME}"
LOCK_DIR="specmark/.locks"
LOCK_FILE="${LOCK_DIR}/${NAME}.lock"

mkdir -p "$LOCK_DIR"

exec 9>"$LOCK_FILE"
if ! flock -n 9; then
    echo "Error: Could not acquire lock for ${NAME}. Another archive may be in progress." >&2
    exit 2
fi

if [ -d "$TARGET" ]; then
    echo "Error: Archive target ${TARGET} already exists. Refusing to overwrite." >&2
    exit 1
fi

mkdir -p "$ARCHIVE_ROOT"
if [ ! -f "${ARCHIVE_ROOT}/.readonly" ]; then
    echo "This directory is read-only history. Do not modify existing entries." > "${ARCHIVE_ROOT}/.readonly"
fi

if [ ! -d "$SOURCE" ]; then
    echo "Error: Change directory ${SOURCE} does not exist." >&2
    exit 1
fi

if [ "$SYNC" = true ]; then
    for delta_spec in "$SOURCE"/specs/*/spec.md; do
        if [ -f "$delta_spec" ]; then
            cap=$(basename "$(dirname "$delta_spec")")
            main_spec="specmark/specs/${cap}/spec.md"
            if [ -f "$main_spec" ]; then
                echo "Syncing delta spec: ${cap}"
                python3 scripts/merge_delta_spec.py --main "$main_spec" --delta "$delta_spec" || {
                    echo "Warning: Failed to sync ${cap} delta spec" >&2
                }
            fi
        fi
    done
fi

COMMIT_SHA=$(git rev-parse HEAD 2>/dev/null || echo "null")

mv "$SOURCE" "$TARGET"

cat > "${TARGET}/meta.json" << EOF
{
  "change": "${NAME}",
  "archived_at": "${DATE}",
  "commit_sha": "${COMMIT_SHA}",
  "synced": ${SYNC}
}
EOF

echo "Archived ${NAME} to ${TARGET}"
echo "Commit SHA: ${COMMIT_SHA}"
echo "Synced: ${SYNC}"
