#!/bin/sh
# Build and validate the extraction boundary. Node is not used by this script.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
if [ "$(go env GOOS)" != "$(go env GOHOSTOS)" ] || [ "$(go env GOARCH)" != "$(go env GOHOSTARCH)" ]; then
    echo "check-native.sh runs host binaries; cross-target builds require a target runner" >&2
    exit 2
fi
if [ -n "${CARGO_BUILD_TARGET:-}" ]; then
    echo "unset CARGO_BUILD_TARGET for the host-only extraction validation" >&2
    exit 2
fi

if [ "$(uname -s)" = Darwin ]; then
    : "${MACOSX_DEPLOYMENT_TARGET:=12.0}"
    export MACOSX_DEPLOYMENT_TARGET
    # Both Cargo's C grammars and Go's cgo objects must use the same target.
    # Explicit flags also invalidate cgo's cache for objects built on a newer SDK.
    CGO_CFLAGS="${CGO_CFLAGS:--O2 -g} -mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET"
    CGO_CXXFLAGS="${CGO_CXXFLAGS:--O2 -g} -mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET"
    CGO_LDFLAGS="${CGO_LDFLAGS:--O2 -g} -mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET"
    export CGO_CFLAGS CGO_CXXFLAGS CGO_LDFLAGS
fi
export CGO_ENABLED=1

manifest="$root/codegraph-kernel/Cargo.toml"
target="$root/codegraph-kernel/target"
cargo test --locked --no-default-features --features cabi --manifest-path "$manifest" --target-dir "$target"
cargo build --release --locked --no-default-features --features cabi --manifest-path "$manifest" --target-dir "$target"

# A successful standalone build must not even depend on the NAPI crates.
dependencies=$(cargo tree --locked --no-default-features --features cabi --manifest-path "$manifest" -e normal,build)
if printf '%s\n' "$dependencies" | grep -Eq '(^|[[:space:]])napi(-derive|-build)? v'; then
    echo "unexpected NAPI dependency in the standalone kernel" >&2
    exit 1
fi

cd "$root/go"
go test ./...
go test -race -tags codegraph_native ./...
go vet -tags codegraph_native ./...
mkdir -p out
go build -tags codegraph_native -trimpath -ldflags="-s -w" -o out/kernelprobe ./cmd/kernelprobe

# An empty PATH and environment prevent a hidden Node launcher or CLI fallback.
env -i PATH= "$root/go/out/kernelprobe" contract > out/contract.json
printf 'package main\nfunc hello() { println("您好") }\n' |
    env -i PATH= "$root/go/out/kernelprobe" extract --language go --path hello.go > out/extraction.json
echo "Node-free host extraction validation passed: $root/go/out/kernelprobe"
