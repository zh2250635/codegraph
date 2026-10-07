# Go native extraction boundary

This module implements CodeGraph's native extraction boundary in Go. It is not
yet a replacement for the TypeScript graph service, indexer, resolver or MCP
server. The compatibility baseline is CodeGraph **v1.6.0**, revision
`dfccdf62547fcd76d343344d823a0e1998d3a89f`.

## Implemented

- A Node-independent Rust C ABI, sharing the original extractors, grammar
  registry, C function-pointer helpers and stack guard with the optional NAPI
  binding.
- A Go port of `src/extraction/kernel/layout.ts` and `decode.ts`, including
  optional fields, tri-state booleans, empty vs absent lists, extra node
  properties, edge metadata and unresolved references.
- Static cgo linkage and explicit native errors. Every Rust result is released
  exactly once; Go copies the buffers before release.
- A `kernelprobe` executable for contract, grammar, extraction, C function-pointer
  scan and comment-strip checks.

The C ABI version is **1**. The five-table extraction buffer ABI is independently
versioned at **2**. Go verifies both versions and the ordered kind tables before
using the native kernel.

The Rust registry supports these 20 language identifiers:

```text
typescript tsx javascript jsx java python go c cpp rust
csharp ruby php swift kotlin r lua luau scala dart
```

The broader `Languages` list and `SupportedLanguage` helper mirror the graph
domain's language vocabulary, **not** the native kernel's capabilities.
Use `NativeContract().Languages` for actual native capabilities.
Unsupported languages and extractor `defer:` conditions are returned as errors;
there is no silent empty result or automatic TS/WASM fallback.

This boundary accepts the source supplied by the caller. It does **not** perform
the TypeScript pipeline's preParse/post passes, extension detection or special
template extraction. Those and the remaining languages must be migrated before
the native graph service can replace the existing implementation.

## Build and validate

Prerequisites: Rust/Cargo, Go 1.25+, and a host C/C++ toolchain. Node is not needed
for the Go decoder, native build, tests or probe. Cargo and Go may need network
access on the first build to obtain their locked dependencies.

From the fork root:

```sh
sh go/scripts/check-native.sh
```

The script runs Rust C ABI tests, builds the standalone release library, verifies
that its dependency graph excludes NAPI, runs Go tests with the race detector and
vet, and builds `go/out/kernelprobe`. It also exercises the binary with an empty
PATH and environment to rule out a hidden Node launcher.

The script is host-only. The validated platform is **macOS arm64**, with an
explicit macOS 12.0 deployment target for both Rust and cgo. A deployment-target
header is not evidence of execution on older macOS versions. Linux and Windows
still require native-runner validation; Windows must align the Rust and Go C
toolchain ABI (GNU vs MSVC) and supply the correctly named static archive. Do not
treat the platform-specific linker flags as proof of cross-platform support.

Pure decoder tests require neither Rust nor a native archive:

```sh
cd go
go test ./...
go test ./kernel -run '^$' -fuzz '^FuzzDecode$' -fuzztime=30s -parallel=4
```

The `codegraph_native` build tag is explicit: ordinary decoder tests do not need
a compiled library. Native APIs and the probe require this tag and
`CGO_ENABLED=1`. There is no disabled-native stub pretending to provide extraction.

The native build uses `codegraph-kernel/target/release/libcodegraph_kernel.a`.
Do not overwrite this archive with a NAPI-enabled build. Build compatibility
artifacts into a separate `--target-dir`.

## Probe examples

From the fork root after the build:

```sh
go/out/kernelprobe contract
go/out/kernelprobe grammar go
printf 'package main\nfunc hello() {}\n' |
  go/out/kernelprobe extract --language go --path hello.go
printf '[{"text":"typedef void (*callback)(int);","structs":[]}]' |
  go/out/kernelprobe scan-cfnptr
printf 'int x; // comment\n' | go/out/kernelprobe strip-c
```

Source is read from stdin, with a 64 MiB limit. `--path` is the logical path used
in graph identifiers, not a request to read the filesystem.

## Release-oracle regression

`kernel/testdata/napi-v1.6.0.json` records the original release's NAPI boundary:
20 languages, 62 source cases, complete raw buffers and decoded results, grammar
tables, C function-pointer facts and comment-strip results. There are 58 successful
cases and four intentional extraction deferrals. Only durations and update
timestamps are normalized; graph fields and array ordering are retained.

- `TestDecoderNAPIReplay` compares the Go decoder to release objects without Node.
- `TestNativeNAPIParity` compares all five C ABI buffers and the Go decoded
  objects against the release, including exact deferral diagnostics.
- Native tests also cover all grammar tables, C helpers, invalid UTF-8,
  concurrent extraction and result ownership.
- Fuzz seeds include real release tables, rather than only an empty metadata row.

Oracle regeneration is a development-only operation needing the **unmodified**
v1.6.0 platform bundle:

```sh
node go/scripts/capture-napi-oracle.mjs /absolute/path/to/platform-package/lib
```

Review corpus or oracle changes rather than replacing expected output to hide a
difference. Capture validates the release version and rejects missing successful
language coverage.

The optional NAPI binding retains the existing JS export names. To verify a
freshly built compatibility module:

```sh
MACOSX_DEPLOYMENT_TARGET=12.0 cargo build --release --locked --features napi \
  --manifest-path codegraph-kernel/Cargo.toml --target-dir /absolute/scratch/napi
cp /absolute/scratch/napi/release/libcodegraph_kernel.dylib /absolute/scratch/napi/kernel.node
node go/scripts/check-napi.mjs /absolute/scratch/napi/kernel.node
```

The filename example is macOS-specific. It deliberately uses a separate output
directory so the standalone static library remains NAPI-free.

## FFI safety

See [codegraph.h](../codegraph-kernel/include/codegraph.h) for the public C ABI.
Input bytes are immutable UTF-8 borrowed only during the call. A result owns its
views until exactly one `cg_result_free`; null free is a safe no-op, but double
free is invalid. Valid-pointer and lifetime rules remain the caller's obligation.
Rust catches unwinding panics, not invalid memory access or allocator OOM aborts.

The Go decoder rejects mismatched table lengths, unsupported buffer versions,
invalid kind or endpoint indices, out-of-bounds or non-UTF-8 strings, malformed
JSON, non-finite/negative timings and counts over five million per table.
These checks do not make unbounded input or native allocation safe from OOM.

## Remaining migration

No TypeScript graph-service layer has been removed yet. The next work is:
database/schema and transactional incremental indexing; language routing and
fallback parity; cross-file resolution and synthesizers; graph/search/context
and explore formatting; MCP/lifecycle behavior; finally daemon integration and
six-platform VSIX validation. Node and WASM can leave the release package only
after those functionality gates pass.
