#!/usr/bin/env bash
# Runs every fuzz target for a while: fuzz/run.sh [seconds per target, default 60] [targets...]
# Starts each from real files (the examples and the frozen format-1 package), plus what earlier
# runs found. A crash stops the run, with the input in fuzz/artifacts/<target>/.
# On Windows without Visual Studio's AddressSanitizer files: FUZZ_SANITIZER=none fuzz/run.sh
set -euo pipefail
cd "$(dirname "$0")/.."

seconds="${1:-60}"
shift || true
targets=("$@")
[ "${#targets[@]}" -gt 0 ] || targets=(package signed_package manifest lockfile scan documents)

for target in package signed_package manifest lockfile scan documents; do
    mkdir -p "fuzz/corpus/$target"
done
cp crates/devstribution-core/tests/fixtures/format-1/hello.dsapp fuzz/corpus/package/
cp crates/devstribution-core/tests/fixtures/format-1/publisher.pub fuzz/corpus/documents/
for app in hello-gui excel-report; do
    cp "examples/$app/pyproject.toml" "fuzz/corpus/manifest/$app.toml"
    cp "examples/$app/uv.lock" "fuzz/corpus/lockfile/$app.lock"
    cp "examples/$app/main.py" "fuzz/corpus/scan/$app.py"
done

for target in "${targets[@]}"; do
    echo "== $target (${seconds}s)"
    # One input taking 10 seconds is a hang; 4 GB is far above what the small limits allow.
    cargo +nightly fuzz run -s "${FUZZ_SANITIZER:-address}" "$target" -- \
        -max_total_time="$seconds" -timeout=10 -rss_limit_mb=4096 -max_len=65536 -len_control=0 -print_final_stats=1
done
