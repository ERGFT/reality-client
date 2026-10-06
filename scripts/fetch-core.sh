#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Скачивает исходники vpn-core ровно той версии, что записана в
# third_party/vpn-core.rev (хеш коммита), в указанный пустой каталог.
# Хеш коммита сам гарантирует содержимое, отдельная контрольная сумма не нужна.
# Если в каталоге уже лежит чистая копия нужной версии, ничего не делает.
#
# Переменная REALITY_CORE_URL подменяет адрес репозитория (например, на
# локальную копию для разработки).
set -euo pipefail

destination="${1:?Использование: fetch-core.sh <каталог>}"
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
revision="$(tr -d '[:space:]' < "$repo_root/third_party/vpn-core.rev")"
url="${REALITY_CORE_URL:-https://github.com/ERGFT/vpn-core.git}"

[[ "$revision" =~ ^[0-9a-f]{40}$ ]] || {
    echo "third_party/vpn-core.rev должен содержать полный хеш коммита, получено: '$revision'" >&2
    exit 1
}
if [[ -e "$destination" ]] && [[ -n "$(ls -A "$destination" 2>/dev/null)" ]]; then
    # Уже скачанное ядро нужной версии (например, third_party/vpn-core) не трогаем.
    if [[ "$(git -C "$destination" rev-parse HEAD 2>/dev/null || true)" == "$revision" ]] \
        && [[ -z "$(git -C "$destination" status --porcelain 2>/dev/null)" ]]; then
        echo "CORE_SOURCE_COMMIT=$revision"
        exit 0
    fi
    echo "Каталог не пуст и содержит не ту версию ядра: $destination" >&2
    exit 1
fi

mkdir -p "$destination"
git -C "$destination" init -q
git -C "$destination" remote add origin "$url"
git -C "$destination" fetch -q --depth 1 origin "$revision"
git -C "$destination" checkout -q FETCH_HEAD

actual="$(git -C "$destination" rev-parse HEAD)"
[[ "$actual" == "$revision" ]] || {
    echo "Скачана другая версия ядра: $actual вместо $revision" >&2
    exit 1
}
[[ -f "$destination/Cargo.toml" ]] || {
    echo 'В скачанных исходниках нет корневого Cargo.toml ядра.' >&2
    exit 1
}
echo "CORE_SOURCE_COMMIT=$revision"
