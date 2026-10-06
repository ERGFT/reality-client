#!/usr/bin/env bash
# Создаёт релиз по тегу, если его ещё нет, и прикрепляет файлы.
# Три платформенных workflow публикуют параллельно и состязаются за создание релиза:
# проигравший просто видит уже созданный релиз. Теги с дефисом (v0.1.0-preview.1) — пре-релизы.
# Использование: publish-release.sh <тег> <файл>...
set -euo pipefail

tag="${1:?Использование: publish-release.sh <тег> <файл>...}"
shift
repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY не задан}"
notes="docs/releases/${tag}.md"

if ! gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
    args=(--repo "$repo" --verify-tag --title "Reality Client ${tag}")
    [[ "$tag" == *-* ]] && args+=(--prerelease)
    if [[ -f "$notes" ]]; then
        args+=(--notes-file "$notes")
    else
        args+=(--generate-notes)
    fi
    gh release create "$tag" "${args[@]}" || gh release view "$tag" --repo "$repo" >/dev/null
fi
gh release upload "$tag" "$@" --clobber --repo "$repo"
