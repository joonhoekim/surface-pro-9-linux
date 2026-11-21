#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Create or update the repository's labels from .github/labels.tsv (labels not listed are left alone).
# usage: tools/labels-sync.sh [OWNER/REPO]      (needs gh, logged in with write access)
set -eu
repo=${1:-joonhoekim/surface-pro-9-linux}
cd "$(dirname "$0")/.."
grep -v '^#' .github/labels.tsv | while IFS='	' read -r name color desc; do
	[ -n "$name" ] || continue
	gh label create "$name" --repo "$repo" --color "$color" --description "$desc" --force
done
