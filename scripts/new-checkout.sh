#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PARENT_DIR="$(dirname "$REPO_ROOT")"
REPO_BASENAME="$(basename "$REPO_ROOT")"

REMOTE_URL="$(git -C "$REPO_ROOT" remote get-url origin)"
CURRENT_BRANCH="$(git -C "$REPO_ROOT" rev-parse --abbrev-ref HEAD)"

n=1
while [[ -d "$PARENT_DIR/${REPO_BASENAME}-checkout-$n" ]]; do
  ((n++))
done

CHECKOUT_DIR="$PARENT_DIR/${REPO_BASENAME}-checkout-$n"

echo "==> Cloning $REMOTE_URL (branch: $CURRENT_BRANCH) into $CHECKOUT_DIR"
git clone --branch "$CURRENT_BRANCH" "$REMOTE_URL" "$CHECKOUT_DIR"

if [[ -f "$REPO_ROOT/.env" ]]; then
  echo "==> Copying .env"
  cp "$REPO_ROOT/.env" "$CHECKOUT_DIR/.env"
fi

echo "==> Running scripts/jig bootstrap in $CHECKOUT_DIR"
(cd "$CHECKOUT_DIR" && scripts/jig bootstrap)

echo
echo "Done! Checkout ready at: $CHECKOUT_DIR"
