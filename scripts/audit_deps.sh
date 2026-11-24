#!/usr/bin/env bash
# Audits both dependency trees for network-capable, telemetry, analytics, ads or crash-reporting
# libraries. Complements `cargo deny check` (deny.toml) and the Gradle manifest check.
set -euo pipefail
cd "$(dirname "$0")/.."
rust_bad='^(reqwest|hyper|hyper-util|h2|h3|ureq|curl|curl-sys|isahc|surf|attohttpc|minreq|tungstenite|quinn|socket2|mio|tokio|async-std|smol|rustls|native-tls|openssl|openssl-sys|ring|sentry|sentry-core|opentelemetry|bugsnag|minidump-writer) '
status=0
for target in aarch64-linux-android armv7-linux-androideabi; do
  tree="$(cargo tree -p facetilt-jni --target "$target" -e normal,build --prefix none --format '{p}' | sort -u)"
  echo "== Rust ($target): $(echo "$tree" | wc -l) crates"
  if echo "$tree" | grep -E "$rust_bad"; then echo "ERROR: network/telemetry crate in Rust tree" >&2; status=1; fi
done
if [[ -x ./gradlew ]]; then
  kt="$(./gradlew -q :app:dependencies --configuration releaseRuntimeClasspath 2>/dev/null || true)"
  echo "== Kotlin release runtime classpath: $(echo "$kt" | grep -c -- '--- ' || true) entries"
  if echo "$kt" | grep -iE "okhttp|retrofit|ktor|volley|cronet|firebase|crashlytics|play-services|gms|sentry|bugsnag|appcenter|amplitude|mixpanel|segment|admob|ads-"; then
    echo "ERROR: network/analytics/ads library in Kotlin tree" >&2; status=1
  fi
fi
exit $status
