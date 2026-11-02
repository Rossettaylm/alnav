#!/usr/bin/env bash
exec "$(git rev-parse --show-toplevel)/.github/scripts/preflight.sh" "$@"
