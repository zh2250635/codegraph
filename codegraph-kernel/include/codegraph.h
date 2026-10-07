/*
 * codegraph.h — Stable C ABI v1 for CodeGraph native extraction kernel.
 *
 * Provides a C ABI boundary for non-Node runtimes (such as Go cgo) while
 * sharing the same underlying extraction engines and stack guards with NAPI.
 *
 * ============================================================================
 * MEMORY, POINTER & THREAD SAFETY CONTRACT
 * ============================================================================
 * 1. UTF-8 Input Strings (Borrow Lifetime, Valid Pointer, Range):
 *    - All functions taking (const uint8_t *ptr, size_t len) expect a borrowed,
 *      contiguous byte slice in the range [ptr, ptr + len).
 *    - The memory is only read during the function call; the kernel never retains
 *      borrowed input pointers after returning.
 *    - (ptr == NULL && len == 0) is valid and treated as an empty string ("").
 *    - (ptr == NULL && len > 0) or (len > PTRDIFF_MAX) is strictly
 *      prohibited and returns CG_STATUS_INVALID_ARGUMENT (1).
 *    - The range [ptr, ptr + len) must belong to a single valid memory allocation.
 *      Pointer guards do not prove that a non-NULL address is accessible.
 *
 * 2. Return Handle & Buffer Views (Ownership & Lifetimes):
 *    - Functions returning cg_result* allocate Rust heap memory.
 *    - The caller acquires ownership of the returned cg_result handle.
 *    - cg_result_buffer() returns a read-only cg_buffer view whose data pointer
 *      borrows directly into the internal allocation of the cg_result handle.
 *    - All cg_buffer views derived from a cg_result handle are strictly valid ONLY
 *      until cg_result_free() is called on that handle.
 *    - After cg_result_free(), all previously returned cg_buffer views become dangling
 *      and must never be dereferenced.
 *
 * 3. Free Lifecycle:
 *    - cg_result_free(NULL) is a safe, idempotent no-op and may be called repeatedly.
 *    - For a non-NULL, valid cg_result handle, cg_result_free() MUST BE CALLED
 *      EXACTLY ONCE. Double-freeing a valid handle or passing an invalid/stale pointer
 *      is undefined behavior.
 *
 * 4. Threading & Concurrency Rules:
 *    - Extraction and parsing routines (cg_contract_info, cg_grammar_info,
 *      cg_extract_file, cg_cfnptr_scan_files, cg_cfnptr_strip_c) keep independent
 *      results and support concurrent calls on multiple OS threads.
 *    - Individual cg_result handles and their derived buffer views are NOT thread-safe:
 *      concurrent reads or racing free operations on the same handle without external
 *      synchronization are forbidden.
 *
 * 5. Panic Catch Boundaries vs OOM Abort:
 *    - Allocating/extraction entry points catch unwinding Rust panics. Version,
 *      result-view, status and free operations cannot panic under the valid-input
 *      contract; invalid pointers are not recoverable errors.
 *    - Walker panics or internal invariant assertions do NOT unwind across FFI into C/Go;
 *      they are caught and returned as CG_STATUS_PANIC (4) with the error text in slot 6.
 *    - Out-of-memory (OOM) during memory allocation triggers Rust's standard allocator
 *      abort policy and cannot be caught via panic boundary.
 * ============================================================================
 */

#ifndef CODEGRAPH_H
#define CODEGRAPH_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define CG_C_ABI_VERSION 1

/* Status codes for cg_result_status */
#define CG_STATUS_SUCCESS 0
#define CG_STATUS_INVALID_ARGUMENT 1
#define CG_STATUS_UNSUPPORTED_LANGUAGE 2
#define CG_STATUS_EXTRACTION_FAILURE 3
#define CG_STATUS_PANIC 4

/* Buffer slot indices for cg_result_buffer */
#define CG_SLOT_META 0
#define CG_SLOT_NODES 1
#define CG_SLOT_EDGES 2
#define CG_SLOT_REFS 3
#define CG_SLOT_ARENA 4
#define CG_SLOT_PAYLOAD 5
#define CG_SLOT_ERROR 6
#define CG_SLOT_COUNT 7

/* Opaque result handle allocated and freed by the native kernel. */
typedef struct cg_result cg_result;

/* Read-only byte view into an internal buffer. Valid until cg_result_free. */
typedef struct cg_buffer {
    const uint8_t *data;
    size_t len;
} cg_buffer;

/**
 * Returns the version of this C ABI (currently 1).
 * Safe to call at any time from any thread.
 */
uint32_t cg_abi_version(void);

/**
 * Returns contract metadata JSON in slot 5:
 * {"abiVersion":2,"kernelVersion":"...","nodeKinds":[...],"edgeKinds":[...],"languages":[...]}
 * Returns an allocated cg_result* handle. Must be released with cg_result_free().
 */
cg_result *cg_contract_info(void);

/**
 * Returns tree-sitter grammar information JSON in slot 5 for the given language:
 * {"abiVersion":...,"nodeKindCount":...,"fieldCount":...,"nodeKinds":[...],"fieldNames":[...]}
 * If the language is unsupported, status is CG_STATUS_UNSUPPORTED_LANGUAGE (2).
 * Returns an allocated cg_result* handle. Must be released with cg_result_free().
 */
cg_result *cg_grammar_info(const uint8_t *language, size_t language_len);

/**
 * Extracts AST symbols, relationships, and references for a single file.
 *
 * Input parameters:
 * - file_path: UTF-8 path string bytes.
 * - content: UTF-8 source code content bytes.
 * - language: UTF-8 language identifier bytes.
 *
 * On success (status 0), returns 5 flat tables:
 * - slot 0: meta (36 bytes fixed layout)
 * - slot 1: nodes table
 * - slot 2: edges table
 * - slot 3: refs table
 * - slot 4: arena string storage
 *
 * On failure, status is non-zero and slot 6 contains the UTF-8 error string.
 * Returns an allocated cg_result* handle. Must be released with cg_result_free().
 */
cg_result *cg_extract_file(
    const uint8_t *file_path,
    size_t file_path_len,
    const uint8_t *content,
    size_t content_len,
    const uint8_t *language,
    size_t language_len
);

/**
 * Performs batched C function-pointer extraction sweep.
 * Accepts a UTF-8 JSON array of files ({ "text": "...", "structs": [...] })
 * and returns JSON array of facts in slot 5.
 * Returns an allocated cg_result* handle. Must be released with cg_result_free().
 */
cg_result *cg_cfnptr_scan_files(const uint8_t *json, size_t len);

/**
 * Strips comments from C/C++ source code for regex matching.
 * Returns stripped UTF-8 text in slot 5.
 * Returns an allocated cg_result* handle. Must be released with cg_result_free().
 */
cg_result *cg_cfnptr_strip_c(const uint8_t *text, size_t len);

/**
 * Returns the status code of a result handle.
 * Returns CG_STATUS_INVALID_ARGUMENT (1) if result is NULL.
 * Provably panic-free when called with NULL or a valid cg_result pointer.
 */
int32_t cg_result_status(const cg_result *result);

/**
 * Retrieves a buffer slice from the given slot.
 * Returns {NULL, 0} if result is NULL or slot is invalid/empty.
 * The returned pointer is read-only and only valid until cg_result_free is called.
 * Provably panic-free when called with NULL or a valid cg_result pointer.
 */
cg_buffer cg_result_buffer(const cg_result *result, uint32_t slot);

/**
 * Frees the result handle and all memory associated with its buffer slots.
 * Calling with NULL is a safe, idempotent no-op.
 * Calling with a valid pointer must occur EXACTLY ONCE; double-free is undefined.
 */
void cg_result_free(cg_result *result);

#ifdef __cplusplus
}
#endif

#endif /* CODEGRAPH_H */
