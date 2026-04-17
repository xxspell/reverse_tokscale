#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/docker-compose.yml"
RUN_ID="tokscale-smoke-$(date +%s)"
HOST_CLAUDE_MARKER="$HOME/.claude/projects/${RUN_ID}.marker"
HOST_CODEX_MARKER="$HOME/.codex/sessions/${RUN_ID}.marker"

cleanup() {
  docker compose -f "$COMPOSE_FILE" down -v >/dev/null 2>&1 || true
}
trap cleanup EXIT

if ! docker info >/dev/null 2>&1; then
  echo "Docker daemon is not available."
  docker info
fi

if [ -e "$HOST_CLAUDE_MARKER" ] || [ -e "$HOST_CODEX_MARKER" ]; then
  echo "Host marker already exists for RUN_ID=$RUN_ID, aborting."
  exit 1
fi

docker compose -f "$COMPOSE_FILE" run --rm --build -e TOKSCALE_SMOKE_RUN_ID="$RUN_ID" emulator

docker compose -f "$COMPOSE_FILE" run --rm -e TOKSCALE_SMOKE_RUN_ID="$RUN_ID" --entrypoint /bin/sh emulator -lc "test -f /root/.claude/projects/${RUN_ID}.marker && test -f /root/.codex/sessions/${RUN_ID}.marker"

docker compose -f "$COMPOSE_FILE" run --rm --entrypoint /bin/sh emulator -lc "command -v tokscale >/dev/null"

if [ -e "$HOST_CLAUDE_MARKER" ] || [ -e "$HOST_CODEX_MARKER" ]; then
  echo "Host home was touched unexpectedly: marker exists under ~/.claude or ~/.codex"
  exit 1
fi

echo "docker_e2e_smoke: PASS (container volume used; host home untouched)"
