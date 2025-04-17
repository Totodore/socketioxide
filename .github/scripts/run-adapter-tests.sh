#!/bin/bash

set -euo pipefail

SOCKETIO_VERSIONS=("v4" "v4-msgpack" "v5" "v5-msgpack")
ADAPTERS=("fred-e2e" "redis-e2e" "redis-cluster-e2e" "fred-cluster-e2e" "mongodb-ttl-e2e" "mongodb-capped-e2e")
EXIT_CODE=0

# ========= 2. Run tests in parallel =========
echo "🚀 Running all tests..."

for VERSION in "${SOCKETIO_VERSIONS[@]}"; do
  for ADAPTER in "${ADAPTERS[@]}"; do
    (
      echo "::group:: 🔬 Testing $ADAPTER ($VERSION)"

      PARSER=$(echo "$VERSION" | cut -d'-' -f2 -s)
      BASE_VERSION=$(echo "$VERSION" | cut -d'-' -f1)
      if ! cargo build -p adapter-e2e --bin "$ADAPTER" --features "$BASE_VERSION${PARSER:+,$PARSER}"; then
        echo "❌ BUILD FAILED for $ADAPTER ($VERSION)"
        echo "::endgroup::"
        exit 1
      else
        echo "✅ BUILD SUCCESS for $ADAPTER ($VERSION)"
      fi

      CMD="cargo run -p adapter-e2e --bin $ADAPTER --features $BASE_VERSION${PARSER:+,$PARSER}"
      if ! CMD="$CMD" node --experimental-strip-types --test-reporter=spec --test e2e/adapter/client.ts; then
        echo "❌ TEST FAILED for $ADAPTER ($VERSION)"
        echo "::endgroup::"
        exit 1
      else
        echo "✅ PASSED: $ADAPTER ($VERSION)"
      fi
      echo "::endgroup::"
    )
  done
done

exit $EXIT_CODE
