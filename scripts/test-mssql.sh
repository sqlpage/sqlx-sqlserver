#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

if [[ "${MSSQL_DATABASE_URL:-}" != *[![:space:]]* ]]; then
    echo "set MSSQL_DATABASE_URL to run SQL Server integration tests" >&2
    exit 1
fi

test_args=(--locked --all-features --test mssql_smoke)
cargo test "${test_args[@]}" --no-run

deadline=$((SECONDS + 120))
while true
do
    output="$(cargo test "${test_args[@]}" \
        connects_and_pings_when_configured -- --exact --ignored 2>&1)" && {
        printf '%s\n' "$output"
        break
    }

    printf '%s\n' "$output"

    if ! grep -Eiq 'connection refused|connection reset|timed out|early eof|unexpected eof' <<<"$output"; then
        echo "SQL Server readiness check failed with a non-transient error" >&2
        exit 1
    fi

    if ((SECONDS >= deadline)); then
        echo "SQL Server did not become ready before the timeout" >&2
        exit 1
    fi

    echo "waiting for SQL Server to accept connections..."
    sleep 5
done

cargo test "${test_args[@]}" -- --include-ignored
