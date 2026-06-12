#!/usr/bin/env sh
set -eu

REPO_URL="${REPO_URL:?REPO_URL is required}"
REPO_BRANCH="${REPO_BRANCH:-main}"
APP_DIR="${APP_DIR:-/workspace/app}"
COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.yml}"
COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-flux-generated}"

if [ ! -d "$APP_DIR/.git" ]; then
  echo "Cloning $REPO_URL into $APP_DIR"
  git clone --branch "$REPO_BRANCH" --depth 1 "$REPO_URL" "$APP_DIR"
else
  echo "Updating repository in $APP_DIR"
  cd "$APP_DIR"
  git fetch origin "$REPO_BRANCH"
  git checkout "$REPO_BRANCH"
  git pull --ff-only origin "$REPO_BRANCH"
fi

cd "$APP_DIR"

echo "Starting services with $COMPOSE_FILE"
docker compose -p "$COMPOSE_PROJECT_NAME" -f "$COMPOSE_FILE" up -d --build
