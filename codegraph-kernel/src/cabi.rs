//! Stable C ABI implementation for CodeGraph native extraction kernel.
//!
//! Exposes an ownership-explicit, panic-bounded C interface (v1) for Go cgo
//! and non-Node runtimes. Callers must uphold pointer and lifetime rules;
//! argument guards do not make arbitrary foreign memory accesses safe.

use std::panic::catch_unwind;

use crate::core_api::{
    cfnptr_scan_files_core, cfnptr_strip_c_core, contract_info_core, extract_file_core,
    grammar_info_core, parse_tree_core, parser_languages_core, CfnptrFileInDto, ExtractError,
};

pub const CG_C_ABI_VERSION: u32 = 1;

pub const CG_STATUS_SUCCESS: i32 = 0;
pub const CG_STATUS_INVALID_ARGUMENT: i32 = 1;
pub const CG_STATUS_UNSUPPORTED_LANGUAGE: i32 = 2;
pub const CG_STATUS_EXTRACTION_FAILURE: i32 = 3;
pub const CG_STATUS_PANIC: i32 = 4;

pub const CG_SLOT_META: u32 = 0;
pub const CG_SLOT_NODES: u32 = 1;
pub const CG_SLOT_EDGES: u32 = 2;
pub const CG_SLOT_REFS: u32 = 3;
pub const CG_SLOT_ARENA: u32 = 4;
pub const CG_SLOT_PAYLOAD: u32 = 5;
pub const CG_SLOT_ERROR: u32 = 6;
pub const CG_SLOT_COUNT: usize = 7;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct cg_buffer {
    pub data: *const u8,
    pub len: usize,
}

#[allow(non_camel_case_types)]
pub struct cg_result {
    pub status: i32,
    pub slots: [Vec<u8>; CG_SLOT_COUNT],
}

impl cg_result {
    pub fn success_extract(
        meta: Vec<u8>,
        nodes: Vec<u8>,
        edges: Vec<u8>,
        refs: Vec<u8>,
        arena: Vec<u8>,
    ) -> Self {
        let mut slots = [const { Vec::new() }; CG_SLOT_COUNT];
        slots[CG_SLOT_META as usize] = meta;
        slots[CG_SLOT_NODES as usize] = nodes;
        slots[CG_SLOT_EDGES as usize] = edges;
        slots[CG_SLOT_REFS as usize] = refs;
        slots[CG_SLOT_ARENA as usize] = arena;
        Self {
            status: CG_STATUS_SUCCESS,
            slots,
        }
    }

    pub fn success_payload(payload: Vec<u8>) -> Self {
        let mut slots = [const { Vec::new() }; CG_SLOT_COUNT];
        slots[CG_SLOT_PAYLOAD as usize] = payload;
        Self {
            status: CG_STATUS_SUCCESS,
            slots,
        }
    }

    pub fn error(status: i32, message: impl Into<String>) -> Self {
        let mut slots = [const { Vec::new() }; CG_SLOT_COUNT];
        slots[CG_SLOT_ERROR as usize] = message.into().into_bytes();
        Self { status, slots }
    }
}

/// Validates raw pointer and length boundaries into a byte slice.
///
/// Guard rules:
/// - len > isize::MAX: invalid argument
/// - ptr is null and len == 0: valid empty slice &[]
/// - ptr is null and len != 0: invalid argument
/// - otherwise borrow the range under the caller's valid-memory contract
unsafe fn validate_slice<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], (i32, &'static str)> {
    if len > isize::MAX as usize {
        return Err((CG_STATUS_INVALID_ARGUMENT, "len exceeds isize::MAX"));
    }
    if ptr.is_null() {
        if len == 0 {
            return Ok(&[]);
        }
        return Err((CG_STATUS_INVALID_ARGUMENT, "null pointer with non-zero len"));
    }
    Ok(std::slice::from_raw_parts(ptr, len))
}

/// Validates raw pointer and length boundaries into a UTF-8 string slice.
unsafe fn validate_utf8<'a>(ptr: *const u8, len: usize) -> Result<&'a str, (i32, &'static str)> {
    let bytes = validate_slice(ptr, len)?;
    std::str::from_utf8(bytes).map_err(|_| (CG_STATUS_INVALID_ARGUMENT, "invalid UTF-8"))
}

/// Executes a closure inside a panic boundary, returning an owned raw pointer.
fn catch_boundary<F>(f: F) -> *mut cg_result
where
    F: FnOnce() -> cg_result + std::panic::UnwindSafe,
{
    match catch_unwind(f) {
        Ok(res) => Box::into_raw(Box::new(res)),
        Err(err) => {
            let msg = if let Some(s) = err.downcast_ref::<&str>() {
                format!("panic: {s}")
            } else if let Some(s) = err.downcast_ref::<String>() {
                format!("panic: {s}")
            } else {
                "unknown panic".to_string()
            };
            Box::into_raw(Box::new(cg_result::error(CG_STATUS_PANIC, msg)))
        }
    }
}

#[no_mangle]
pub extern "C" fn cg_abi_version() -> u32 {
    CG_C_ABI_VERSION
}

#[no_mangle]
pub extern "C" fn cg_contract_info() -> *mut cg_result {
    catch_boundary(|| {
        let data = contract_info_core();
        match serde_json::to_vec(&data) {
            Ok(json_bytes) => cg_result::success_payload(json_bytes),
            Err(e) => cg_result::error(
                CG_STATUS_EXTRACTION_FAILURE,
                format!("failed to serialize contract info: {e}"),
            ),
        }
    })
}

/// Retrieves grammar metadata for the requested language.
///
/// # Safety
///
/// - `language` must point to `language_len` contiguous valid bytes of UTF-8 text,
///   or be null if `language_len` is 0.
/// - The memory range must not be mutated during execution of this call.
#[no_mangle]
pub unsafe extern "C" fn cg_grammar_info(
    language: *const u8,
    language_len: usize,
) -> *mut cg_result {
    catch_boundary(|| {
        let lang_str = match unsafe { validate_utf8(language, language_len) } {
            Ok(s) => s,
            Err((status, msg)) => return cg_result::error(status, msg),
        };
        let info = match grammar_info_core(lang_str) {
            Some(i) => i,
            None => {
                return cg_result::error(
                    CG_STATUS_UNSUPPORTED_LANGUAGE,
                    format!("unsupported language: {lang_str}"),
                )
            }
        };
        match serde_json::to_vec(&info) {
            Ok(json_bytes) => cg_result::success_payload(json_bytes),
            Err(e) => cg_result::error(
                CG_STATUS_EXTRACTION_FAILURE,
                format!("failed to serialize grammar info: {e}"),
            ),
        }
    })
}

/// Extracts AST nodes, edges, refs, and string arena for a source file.
///
/// # Safety
///
/// - `file_path`, `content`, and `language` must each point to contiguous valid
///   bytes of their declared lengths, or be null if their corresponding length is 0.
/// - None of the input memory ranges may be mutated during execution of this call.
#[no_mangle]
pub unsafe extern "C" fn cg_extract_file(
    file_path: *const u8,
    file_path_len: usize,
    content: *const u8,
    content_len: usize,
    language: *const u8,
    language_len: usize,
) -> *mut cg_result {
    catch_boundary(|| {
        let fp = match unsafe { validate_utf8(file_path, file_path_len) } {
            Ok(s) => s,
            Err((status, msg)) => return cg_result::error(status, format!("file_path: {msg}")),
        };
        let c = match unsafe { validate_utf8(content, content_len) } {
            Ok(s) => s,
            Err((status, msg)) => return cg_result::error(status, format!("content: {msg}")),
        };
        let l = match unsafe { validate_utf8(language, language_len) } {
            Ok(s) => s,
            Err((status, msg)) => return cg_result::error(status, format!("language: {msg}")),
        };

        match extract_file_core(fp, c, l) {
            Ok(out) => {
                cg_result::success_extract(out.meta, out.nodes, out.edges, out.refs, out.arena)
            }
            Err(ExtractError::UnsupportedLanguage(msg)) => {
                cg_result::error(CG_STATUS_UNSUPPORTED_LANGUAGE, msg)
            }
            Err(ExtractError::ExtractionFailure(msg)) => {
                cg_result::error(CG_STATUS_EXTRACTION_FAILURE, msg)
            }
        }
    })
}

/// Performs C function pointer extraction sweep across a batch of files.
///
/// # Safety
///
/// - `json` must point to `len` contiguous valid bytes of UTF-8 JSON text,
///   or be null if `len` is 0.
/// - The memory range must not be mutated during execution of this call.
#[no_mangle]
pub unsafe extern "C" fn cg_cfnptr_scan_files(json: *const u8, len: usize) -> *mut cg_result {
    catch_boundary(|| {
        let json_str = match unsafe { validate_utf8(json, len) } {
            Ok(s) => s,
            Err((status, msg)) => return cg_result::error(status, msg),
        };
        let files: Vec<CfnptrFileInDto> = match serde_json::from_str(json_str) {
            Ok(f) => f,
            Err(e) => {
                return cg_result::error(
                    CG_STATUS_INVALID_ARGUMENT,
                    format!("invalid JSON: {e}"),
                )
            }
        };
        let facts = cfnptr_scan_files_core(&files);
        match serde_json::to_vec(&facts) {
            Ok(out_json) => cg_result::success_payload(out_json),
            Err(e) => cg_result::error(
                CG_STATUS_EXTRACTION_FAILURE,
                format!("failed to serialize facts: {e}"),
            ),
        }
    })
}

/// Strips comments from C/C++ source code for regex scanning.
///
/// # Safety
///
/// - `text` must point to `len` contiguous valid bytes of UTF-8 text,
///   or be null if `len` is 0.
/// - The memory range must not be mutated during execution of this call.
/// Parses source code with tree-sitter UTF-16 input recovery and serializes AST to JSON.
///
/// # Safety
///
/// - `source` and `language` must point to contiguous valid bytes of their declared lengths,
///   or be null if length is 0.
/// - The memory range must not be mutated during execution of this call.
#[no_mangle]
pub unsafe extern "C" fn cg_parse_tree(
    source: *const u8,
    source_len: usize,
    language: *const u8,
    language_len: usize,
) -> *mut cg_result {
    catch_boundary(|| {
        let src = match unsafe { validate_utf8(source, source_len) } {
            Ok(s) => s,
            Err((status, msg)) => return cg_result::error(status, format!("source: {msg}")),
        };
        let lang = match unsafe { validate_utf8(language, language_len) } {
            Ok(s) => s,
            Err((status, msg)) => return cg_result::error(status, format!("language: {msg}")),
        };

        match parse_tree_core(src, lang) {
            Ok(json_bytes) => cg_result::success_payload(json_bytes),
            Err(ExtractError::UnsupportedLanguage(msg)) => {
                cg_result::error(CG_STATUS_UNSUPPORTED_LANGUAGE, msg)
            }
            Err(ExtractError::ExtractionFailure(msg)) => {
                cg_result::error(CG_STATUS_EXTRACTION_FAILURE, msg)
            }
        }
    })
}

/// Retrieves list of supported parser language identifiers.
#[no_mangle]
pub extern "C" fn cg_parser_languages() -> *mut cg_result {
    catch_boundary(|| {
        match parser_languages_core() {
            Ok(json_bytes) => cg_result::success_payload(json_bytes),
            Err(e) => cg_result::error(
                CG_STATUS_EXTRACTION_FAILURE,
                format!("failed to serialize parser languages: {e}"),
            ),
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn cg_cfnptr_strip_c(text: *const u8, len: usize) -> *mut cg_result {
    catch_boundary(|| {
        let text_bytes = match unsafe { validate_slice(text, len) } {
            Ok(b) => b,
            Err((status, msg)) => return cg_result::error(status, msg),
        };
        if std::str::from_utf8(text_bytes).is_err() {
            return cg_result::error(CG_STATUS_INVALID_ARGUMENT, "invalid UTF-8");
        }
        let stripped = cfnptr_strip_c_core(text_bytes);
        cg_result::success_payload(stripped)
    })
}

/// Reads the execution status from a result handle.
///
/// # Safety
///
/// - `result` must be either null or a valid, non-dangling pointer to a `cg_result`
///   handle previously returned by this library and not yet freed.
#[no_mangle]
pub unsafe extern "C" fn cg_result_status(result: *const cg_result) -> i32 {
    if result.is_null() {
        return CG_STATUS_INVALID_ARGUMENT;
    }
    unsafe { (*result).status }
}

/// Reads a buffer slice view for the requested slot.
///
/// # Safety
///
/// - `result` must be either null or a valid, non-dangling pointer to a `cg_result`
///   handle previously returned by this library and not yet freed.
/// - The returned `cg_buffer` must not be used after `cg_result_free` has been called
///   on `result`.
#[no_mangle]
pub unsafe extern "C" fn cg_result_buffer(result: *const cg_result, slot: u32) -> cg_buffer {
    if result.is_null() || slot as usize >= CG_SLOT_COUNT {
        return cg_buffer {
            data: std::ptr::null(),
            len: 0,
        };
    }
    let buf = unsafe { &(*result).slots[slot as usize] };
    if buf.is_empty() {
        cg_buffer {
            data: std::ptr::null(),
            len: 0,
        }
    } else {
        cg_buffer {
            data: buf.as_ptr(),
            len: buf.len(),
        }
    }
}

/// Frees an allocated result handle and its internal buffers.
///
/// # Safety
///
/// - `result` must be either null or a valid, non-dangling pointer to a `cg_result`
///   handle previously returned by this library.
/// - If `result` is non-null, it must NOT be freed more than once (no double-free).
#[no_mangle]
pub unsafe extern "C" fn cg_result_free(result: *mut cg_result) {
    if result.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(result));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abi_version() {
        assert_eq!(cg_abi_version(), 1);
    }

    #[test]
    fn test_contract_info() {
        let res = cg_contract_info();
        assert!(!res.is_null());
        assert_eq!(unsafe { cg_result_status(res) }, CG_STATUS_SUCCESS);

        let buf = unsafe { cg_result_buffer(res, CG_SLOT_PAYLOAD) };
        assert!(!buf.data.is_null());
        assert!(buf.len > 0);

        let slice = unsafe { std::slice::from_raw_parts(buf.data, buf.len) };
        let json_val: serde_json::Value = serde_json::from_slice(slice).unwrap();
        assert_eq!(json_val["abiVersion"], 2);
        assert!(json_val["kernelVersion"].is_string());
        assert!(json_val["nodeKinds"].is_array());
        assert!(json_val["edgeKinds"].is_array());
        assert!(json_val["languages"].is_array());
        assert_eq!(json_val["languages"].as_array().unwrap().len(), 20);

        unsafe { cg_result_free(res) };
    }

    #[test]
    fn test_grammar_info_supported_and_unsupported() {
        // Supported: typescript
        let lang = "typescript";
        let res = unsafe { cg_grammar_info(lang.as_ptr(), lang.len()) };
        assert_eq!(unsafe { cg_result_status(res) }, CG_STATUS_SUCCESS);
        let buf = unsafe { cg_result_buffer(res, CG_SLOT_PAYLOAD) };
        let slice = unsafe { std::slice::from_raw_parts(buf.data, buf.len) };
        let val: serde_json::Value = serde_json::from_slice(slice).unwrap();
        assert!(val["abiVersion"].is_number());
        assert!(val["nodeKindCount"].as_u64().unwrap() > 0);
        assert!(val["fieldCount"].as_u64().unwrap() > 0);
        assert!(val["nodeKinds"].is_array());
        assert!(val["fieldNames"].is_array());
        unsafe { cg_result_free(res) };

        // Unsupported: non_existent
        let unsupp = "unsupported_lang";
        let res_unsupp = unsafe { cg_grammar_info(unsupp.as_ptr(), unsupp.len()) };
        assert_eq!(
            unsafe { cg_result_status(res_unsupp) },
            CG_STATUS_UNSUPPORTED_LANGUAGE
        );
        let err_buf = unsafe { cg_result_buffer(res_unsupp, CG_SLOT_ERROR) };
        assert!(!err_buf.data.is_null());
        let err_str = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(err_buf.data, err_buf.len)).unwrap()
        };
        assert!(err_str.contains("unsupported language"));
        unsafe { cg_result_free(res_unsupp) };
    }

    #[test]
    fn test_extract_file_smoke_and_errors() {
        // 1. Success smoke test (typescript)
        let path = "test.ts";
        let content = "export function add(a: number, b: number): number { return a + b; }";
        let lang = "typescript";
        let res = unsafe {
            cg_extract_file(
                path.as_ptr(),
                path.len(),
                content.as_ptr(),
                content.len(),
                lang.as_ptr(),
                lang.len(),
            )
        };
        assert_eq!(unsafe { cg_result_status(res) }, CG_STATUS_SUCCESS);
        let meta = unsafe { cg_result_buffer(res, CG_SLOT_META) };
        let nodes = unsafe { cg_result_buffer(res, CG_SLOT_NODES) };
        let edges = unsafe { cg_result_buffer(res, CG_SLOT_EDGES) };
        let refs = unsafe { cg_result_buffer(res, CG_SLOT_REFS) };
        let arena = unsafe { cg_result_buffer(res, CG_SLOT_ARENA) };
        assert_eq!(meta.len, 36);
        assert!(nodes.len > 0);
        assert!(arena.len > 0);
        let _ = (edges, refs);
        unsafe { cg_result_free(res) };

        // 2. Unsupported language
        let bad_lang = "unknown_lang";
        let res_bad = unsafe {
            cg_extract_file(
                path.as_ptr(),
                path.len(),
                content.as_ptr(),
                content.len(),
                bad_lang.as_ptr(),
                bad_lang.len(),
            )
        };
        assert_eq!(
            unsafe { cg_result_status(res_bad) },
            CG_STATUS_UNSUPPORTED_LANGUAGE
        );
        let err_buf = unsafe { cg_result_buffer(res_bad, CG_SLOT_ERROR) };
        let err_str = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(err_buf.data, err_buf.len)).unwrap()
        };
        assert!(err_str.contains("unsupported language"));
        unsafe { cg_result_free(res_bad) };

        // 3. Invalid UTF-8
        let bad_utf8 = [0xFF, 0xFE, 0xFD];
        let res_utf8 = unsafe {
            cg_extract_file(
                path.as_ptr(),
                path.len(),
                bad_utf8.as_ptr(),
                bad_utf8.len(),
                lang.as_ptr(),
                lang.len(),
            )
        };
        assert_eq!(
            unsafe { cg_result_status(res_utf8) },
            CG_STATUS_INVALID_ARGUMENT
        );
        unsafe { cg_result_free(res_utf8) };

        // 4. Null pointer with non-zero len
        let res_null = unsafe {
            cg_extract_file(
                std::ptr::null(),
                10,
                content.as_ptr(),
                content.len(),
                lang.as_ptr(),
                lang.len(),
            )
        };
        assert_eq!(
            unsafe { cg_result_status(res_null) },
            CG_STATUS_INVALID_ARGUMENT
        );
        unsafe { cg_result_free(res_null) };

        // 5. Null pointer with len 0 (valid empty string)
        let res_empty = unsafe {
            cg_extract_file(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                lang.as_ptr(),
                lang.len(),
            )
        };
        assert_eq!(unsafe { cg_result_status(res_empty) }, CG_STATUS_SUCCESS);
        unsafe { cg_result_free(res_empty) };

        // 6. Len exceeds isize::MAX
        let res_huge = unsafe {
            cg_extract_file(
                path.as_ptr(),
                (isize::MAX as usize) + 1,
                content.as_ptr(),
                content.len(),
                lang.as_ptr(),
                lang.len(),
            )
        };
        assert_eq!(
            unsafe { cg_result_status(res_huge) },
            CG_STATUS_INVALID_ARGUMENT
        );
        unsafe { cg_result_free(res_huge) };
    }

    #[test]
    fn test_cfnptr_scan_files_and_strip() {
        // Strip C test
        let c_code = "/* comment */ int main() { // inline\n return 0; }";
        let res_strip = unsafe { cg_cfnptr_strip_c(c_code.as_ptr(), c_code.len()) };
        assert_eq!(unsafe { cg_result_status(res_strip) }, CG_STATUS_SUCCESS);
        let buf = unsafe { cg_result_buffer(res_strip, CG_SLOT_PAYLOAD) };
        let stripped = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(buf.data, buf.len)).unwrap()
        };
        assert!(!stripped.contains("/* comment */"));
        assert!(!stripped.contains("// inline"));
        unsafe { cg_result_free(res_strip) };

        // Scan files JSON test
        let scan_json = r#"[{"text":"typedef void (*callback)(int);","structs":[]}]"#;
        let res_scan = unsafe { cg_cfnptr_scan_files(scan_json.as_ptr(), scan_json.len()) };
        assert_eq!(unsafe { cg_result_status(res_scan) }, CG_STATUS_SUCCESS);
        let buf_scan = unsafe { cg_result_buffer(res_scan, CG_SLOT_PAYLOAD) };
        let facts_json = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(buf_scan.data, buf_scan.len)).unwrap()
        };
        let val: serde_json::Value = serde_json::from_str(facts_json).unwrap();
        assert!(val.is_array());
        let facts = &val.as_array().unwrap()[0];
        assert_eq!(facts["fnPtrTypedefs"][0], "callback");
        unsafe { cg_result_free(res_scan) };

        // Invalid JSON scan
        let bad_json = "invalid-json";
        let res_bad_scan = unsafe { cg_cfnptr_scan_files(bad_json.as_ptr(), bad_json.len()) };
        assert_eq!(
            unsafe { cg_result_status(res_bad_scan) },
            CG_STATUS_INVALID_ARGUMENT
        );
        unsafe { cg_result_free(res_bad_scan) };
    }

    #[test]
    fn test_null_handle_safety_and_ownership() {
        assert_eq!(
            unsafe { cg_result_status(std::ptr::null()) },
            CG_STATUS_INVALID_ARGUMENT
        );
        let empty_buf = unsafe { cg_result_buffer(std::ptr::null(), CG_SLOT_META) };
        assert!(empty_buf.data.is_null());
        assert_eq!(empty_buf.len, 0);

        // Free null is safe and idempotent
        unsafe {
            cg_result_free(std::ptr::null_mut());
            cg_result_free(std::ptr::null_mut());
        }
    }

    #[test]
    fn test_panic_boundary() {
        let panic_res = catch_boundary(|| {
            panic!("test panic caught");
        });
        assert!(!panic_res.is_null());
        assert_eq!(unsafe { cg_result_status(panic_res) }, CG_STATUS_PANIC);
        let err_buf = unsafe { cg_result_buffer(panic_res, CG_SLOT_ERROR) };
        let err_str = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(err_buf.data, err_buf.len)).unwrap()
        };
        assert!(err_str.contains("panic: test panic caught"));
        unsafe { cg_result_free(panic_res) };
    }

    #[test]
    fn test_all_20_languages_smoke() {
        let snippets: &[(&str, &str, &str)] = &[
            ("test.ts", "typescript", "export const x = 1;"),
            ("test.tsx", "tsx", "export const App = () => <div/>;"),
            ("test.js", "javascript", "const x = 1;"),
            ("test.jsx", "jsx", "const App = () => <div/>;"),
            ("Test.java", "java", "class Test {}"),
            ("test.py", "python", "x = 1"),
            ("main.go", "go", "package main"),
            ("test.c", "c", "int main() { return 0; }"),
            ("test.cpp", "cpp", "int main() { return 0; }"),
            ("main.rs", "rust", "fn main() {}"),
            ("Program.cs", "csharp", "class Program {}"),
            ("test.rb", "ruby", "def hello; end"),
            ("test.php", "php", "<?php echo 1;"),
            ("test.swift", "swift", "let x = 1"),
            ("Test.kt", "kotlin", "fun main() {}"),
            ("test.R", "r", "x <- 1"),
            ("test.lua", "lua", "local x = 1"),
            ("test.luau", "luau", "local x: number = 1"),
            ("Test.scala", "scala", "object Test {}"),
            ("test.dart", "dart", "void main() {}"),
        ];

        for &(path, lang, code) in snippets {
            let res = unsafe {
                cg_extract_file(
                    path.as_ptr(),
                    path.len(),
                    code.as_ptr(),
                    code.len(),
                    lang.as_ptr(),
                    lang.len(),
                )
            };
            let status = unsafe { cg_result_status(res) };
            assert_eq!(
                status, CG_STATUS_SUCCESS,
                "Language {lang} extraction failed with status {status}"
            );
            let meta = unsafe { cg_result_buffer(res, CG_SLOT_META) };
            assert_eq!(meta.len, 36, "Language {lang} meta size mismatch");
            unsafe { cg_result_free(res) };
        }
    }

    #[test]
    fn test_parser_languages() {
        let res = cg_parser_languages();
        assert!(!res.is_null());
        assert_eq!(unsafe { cg_result_status(res) }, CG_STATUS_SUCCESS);
        let buf = unsafe { cg_result_buffer(res, CG_SLOT_PAYLOAD) };
        assert!(!buf.data.is_null());
        let slice = unsafe { std::slice::from_raw_parts(buf.data, buf.len) };
        let json_val: serde_json::Value = serde_json::from_slice(slice).unwrap();
        let langs = json_val["languages"].as_array().expect("languages array");
        // All 32 supported languages in parser_languages (20 old + 12 new)
        assert_eq!(langs.len(), 32);
        assert!(langs.iter().any(|v| v.as_str() == Some("typescript")));
        assert!(langs.iter().any(|v| v.as_str() == Some("rust")));
        assert!(langs.iter().any(|v| v.as_str() == Some("arkts")));
        assert!(langs.iter().any(|v| v.as_str() == Some("terraform")));
        assert!(langs.iter().any(|v| v.as_str() == Some("objc")));
        assert!(langs.iter().any(|v| v.as_str() == Some("solidity")));
        assert!(langs.iter().any(|v| v.as_str() == Some("erlang")));
        assert!(langs.iter().any(|v| v.as_str() == Some("pascal")));
        assert!(langs.iter().any(|v| v.as_str() == Some("cfml")));
        assert!(langs.iter().any(|v| v.as_str() == Some("cfscript")));
        assert!(langs.iter().any(|v| v.as_str() == Some("cfquery")));
        assert!(langs.iter().any(|v| v.as_str() == Some("nix")));
        assert!(langs.iter().any(|v| v.as_str() == Some("cobol")));
        assert!(langs.iter().any(|v| v.as_str() == Some("vbnet")));

        // Verify cg_contract_info is UNTOUCHED (oracle preserved at 20)
        let contract_res = cg_contract_info();
        assert_eq!(unsafe { cg_result_status(contract_res) }, CG_STATUS_SUCCESS);
        let c_buf = unsafe { cg_result_buffer(contract_res, CG_SLOT_PAYLOAD) };
        let c_slice = unsafe { std::slice::from_raw_parts(c_buf.data, c_buf.len) };
        let c_val: serde_json::Value = serde_json::from_slice(c_slice).unwrap();
        assert_eq!(c_val["languages"].as_array().unwrap().len(), 20);
        unsafe { cg_result_free(contract_res) };

        unsafe { cg_result_free(res) };
    }

    #[test]
    fn test_parse_tree_smoke_and_structure() {
        let lang = "typescript";
        let code = "function add(a: number, b: number): number { return a + b; }";
        let res = unsafe {
            cg_parse_tree(code.as_ptr(), code.len(), lang.as_ptr(), lang.len())
        };
        assert_eq!(unsafe { cg_result_status(res) }, CG_STATUS_SUCCESS);
        let buf = unsafe { cg_result_buffer(res, CG_SLOT_PAYLOAD) };
        assert!(!buf.data.is_null());
        let slice = unsafe { std::slice::from_raw_parts(buf.data, buf.len) };
        let val: serde_json::Value = serde_json::from_slice(slice).unwrap();

        assert_eq!(val["version"].as_u64().unwrap(), 1);
        assert_eq!(val["root"].as_i64().unwrap(), 0);
        assert_eq!(val["hasError"].as_bool().unwrap(), false);

        let types = val["types"].as_array().unwrap();
        let fields = val["fields"].as_array().unwrap();
        let nodes = val["nodes"].as_array().unwrap();

        assert!(!types.is_empty());
        assert!(!fields.is_empty());
        assert!(!nodes.is_empty());

        // Check root node
        let root_node = &nodes[0];
        assert_eq!(root_node["p"].as_i64().unwrap(), -1);
        assert_eq!(root_node["f"].as_i64().unwrap(), -1);
        assert_eq!(root_node["sb"].as_u64().unwrap(), 0);
        assert_eq!(root_node["eb"].as_u64().unwrap(), code.len() as u64);

        // Verify preorder parenting invariants: parent < child index, and p is valid
        for (i, node) in nodes.iter().enumerate() {
            let p = node["p"].as_i64().unwrap();
            if i == 0 {
                assert_eq!(p, -1);
            } else {
                assert!(p >= 0 && (p as usize) < i, "child {i} parent {p} must be < child index");
            }
            let t = node["t"].as_u64().unwrap() as usize;
            assert!(t < types.len(), "type index out of bounds");
            let f = node["f"].as_i64().unwrap();
            if f >= 0 {
                assert!((f as usize) < fields.len(), "field index out of bounds");
            }
            let sb = node["sb"].as_u64().unwrap() as usize;
            let eb = node["eb"].as_u64().unwrap() as usize;
            assert!(sb <= eb);
            assert!(eb <= code.len());
        }

        unsafe { cg_result_free(res) };
    }

    #[test]
    fn test_parse_tree_unicode_and_utf16() {
        let lang = "typescript";
        // Code with multi-byte Chinese characters (3 bytes UTF-8, 1 code unit UTF-16)
        // and 4-byte emoji 😀 (4 bytes UTF-8, 2 code units UTF-16).
        let code = "const 变量 = '😀测试';";
        let res = unsafe {
            cg_parse_tree(code.as_ptr(), code.len(), lang.as_ptr(), lang.len())
        };
        assert_eq!(unsafe { cg_result_status(res) }, CG_STATUS_SUCCESS);
        let buf = unsafe { cg_result_buffer(res, CG_SLOT_PAYLOAD) };
        let slice = unsafe { std::slice::from_raw_parts(buf.data, buf.len) };
        let val: serde_json::Value = serde_json::from_slice(slice).unwrap();
        let types = val["types"].as_array().unwrap();
        let nodes = val["nodes"].as_array().unwrap();

        // Find the identifier node for "变量"
        let ident_node = nodes.iter().find(|n| {
            let t = n["t"].as_u64().unwrap() as usize;
            types[t].as_str() == Some("identifier")
        }).expect("should find identifier node");

        let sb = ident_node["sb"].as_u64().unwrap() as usize;
        let eb = ident_node["eb"].as_u64().unwrap() as usize;
        assert_eq!(&code[sb..eb], "变量");

        // Verify string node contains the emoji and Chinese
        let str_node = nodes.iter().find(|n| {
            let t = n["t"].as_u64().unwrap() as usize;
            types[t].as_str() == Some("string")
        }).expect("should find string node");
        let s_sb = str_node["sb"].as_u64().unwrap() as usize;
        let s_eb = str_node["eb"].as_u64().unwrap() as usize;
        assert_eq!(&code[s_sb..s_eb], "'😀测试'");

        unsafe { cg_result_free(res) };
    }

    #[test]
    fn test_parse_tree_syntax_error_recovery() {
        let lang = "typescript";
        // Broken syntax: function without name or body
        let code = "function ( {";
        let res = unsafe {
            cg_parse_tree(code.as_ptr(), code.len(), lang.as_ptr(), lang.len())
        };
        assert_eq!(unsafe { cg_result_status(res) }, CG_STATUS_SUCCESS);
        let buf = unsafe { cg_result_buffer(res, CG_SLOT_PAYLOAD) };
        let slice = unsafe { std::slice::from_raw_parts(buf.data, buf.len) };
        let val: serde_json::Value = serde_json::from_slice(slice).unwrap();

        assert_eq!(val["hasError"].as_bool().unwrap(), true);
        let nodes = val["nodes"].as_array().unwrap();
        assert!(!nodes.is_empty());
        // Verify at least one node has isError(4) or hasError(8) flag
        let has_err_flag = nodes.iter().any(|n| {
            let flags = n["flags"].as_u64().unwrap();
            (flags & 4) != 0 || (flags & 8) != 0
        });
        assert!(has_err_flag, "AST recovery should mark error nodes in flags");

        unsafe { cg_result_free(res) };
    }

    #[test]
    fn test_parse_tree_error_guards() {
        let lang = "typescript";
        let code = "const x = 1;";

        // 1. Invalid UTF-8 in source
        let bad_utf8 = [0xFF, 0xFE, 0xFD];
        let res_bad = unsafe {
            cg_parse_tree(bad_utf8.as_ptr(), bad_utf8.len(), lang.as_ptr(), lang.len())
        };
        assert_eq!(unsafe { cg_result_status(res_bad) }, CG_STATUS_INVALID_ARGUMENT);
        unsafe { cg_result_free(res_bad) };

        // 2. Unsupported language
        let unsupp = "brainfuck";
        let res_unsupp = unsafe {
            cg_parse_tree(code.as_ptr(), code.len(), unsupp.as_ptr(), unsupp.len())
        };
        assert_eq!(unsafe { cg_result_status(res_unsupp) }, CG_STATUS_UNSUPPORTED_LANGUAGE);
        unsafe { cg_result_free(res_unsupp) };

        // 3. Null pointer with non-zero len
        let res_null = unsafe {
            cg_parse_tree(std::ptr::null(), 10, lang.as_ptr(), lang.len())
        };
        assert_eq!(unsafe { cg_result_status(res_null) }, CG_STATUS_INVALID_ARGUMENT);
        unsafe { cg_result_free(res_null) };

        // 4. Empty source (null with len 0)
        let res_empty = unsafe {
            cg_parse_tree(std::ptr::null(), 0, lang.as_ptr(), lang.len())
        };
        assert_eq!(unsafe { cg_result_status(res_empty) }, CG_STATUS_SUCCESS);
        unsafe { cg_result_free(res_empty) };
    }

    #[test]
    fn test_parse_tree_new_12_languages() {
        let new_snippets: &[(&str, &str)] = &[
            ("arkts", r#"@Component struct MyComponent { build() { Column() {} } }"#),
            ("terraform", r#"bucket = "my-bucket""#),
            ("objc", r#"@interface MyClass : NSObject
- (void)doSomething;
@end"#),
            ("solidity", r#"contract MyContract { function hello() public pure {} }"#),
            ("erlang", r#"-module(hello).
-export([start/0]).
start() -> ok."#),
            ("pascal", r#"program Hello;
begin
  writeln('hello');
end."#),
            ("cfml", r#"<cfcomponent><cffunction name="test"></cffunction></cfcomponent>"#),
            ("cfscript", r#"component { function hello() { return 1; } }"#),
            ("cfquery", r#"SELECT id FROM users WHERE id = #userId#"#),
            ("nix", r#"{ pkgs ? import <nixpkgs> {} }: pkgs.stdenv.mkDerivation {}"#),
            ("cobol", "       IDENTIFICATION DIVISION.
       PROGRAM-ID. HELLO.
       PROCEDURE DIVISION.
           DISPLAY 'HELLO'.
           STOP RUN."),
            ("vbnet", r#"Class Program
    Sub Main()
        Console.WriteLine("Hi")
    End Sub
End Class"#),
        ];

        for &(lang, code) in new_snippets {
            let res = unsafe {
                cg_parse_tree(code.as_ptr(), code.len(), lang.as_ptr(), lang.len())
            };
            let status = unsafe { cg_result_status(res) };
            if status != CG_STATUS_SUCCESS {
                let err_buf = unsafe { cg_result_buffer(res, CG_SLOT_ERROR) };
                let err_msg = if !err_buf.data.is_null() && err_buf.len > 0 {
                    unsafe { std::str::from_utf8(std::slice::from_raw_parts(err_buf.data, err_buf.len)).unwrap_or("non-utf8 error") }
                } else {
                    "empty error"
                };
                panic!("Language {lang} cg_parse_tree failed with status {status}: {err_msg}");
            }
            let buf = unsafe { cg_result_buffer(res, CG_SLOT_PAYLOAD) };
            assert!(!buf.data.is_null());
            let slice = unsafe { std::slice::from_raw_parts(buf.data, buf.len) };
            let val: serde_json::Value = serde_json::from_slice(slice)
                .unwrap_or_else(|e| panic!("Failed to parse JSON for {lang}: {e}"));
            assert_eq!(val["version"].as_u64().unwrap(), 1);
            assert_eq!(val["root"].as_i64().unwrap(), 0);
            assert!(val["nodes"].as_array().unwrap().len() > 0, "No nodes for {lang}");
            unsafe { cg_result_free(res) };

            // Also test grammar_info works for all new languages
            let gres = unsafe { cg_grammar_info(lang.as_ptr(), lang.len()) };
            assert_eq!(unsafe { cg_result_status(gres) }, CG_STATUS_SUCCESS, "grammar_info for {lang}");
            let gbuf = unsafe { cg_result_buffer(gres, CG_SLOT_PAYLOAD) };
            let gslice = unsafe { std::slice::from_raw_parts(gbuf.data, gbuf.len) };
            let gval: serde_json::Value = serde_json::from_slice(gslice).unwrap();
            assert!(gval["nodeKindCount"].as_u64().unwrap() > 0);
            unsafe { cg_result_free(gres) };
        }
    }

    #[test]
    fn dump_native_grammars_info() {
        let mut map = serde_json::Map::new();
        for &lang in &crate::langs::PARSER_LANGUAGES {
            let res = unsafe { cg_grammar_info(lang.as_ptr(), lang.len()) };
            assert_eq!(unsafe { cg_result_status(res) }, CG_STATUS_SUCCESS, "grammar_info for {lang}");
            let buf = unsafe { cg_result_buffer(res, CG_SLOT_PAYLOAD) };
            let slice = unsafe { std::slice::from_raw_parts(buf.data, buf.len) };
            let val: serde_json::Value = serde_json::from_slice(slice).unwrap();
            map.insert(lang.to_string(), val);
            unsafe { cg_result_free(res) };
        }
        let out_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tools/native-grammars-dump.json");
        std::fs::write(out_path, serde_json::to_string_pretty(&map).unwrap()).unwrap();
    }

    #[test]
    fn test_exact_tree_parity_with_wasm_including_errors_missing_unicode() {
        let sample_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tools/wasm-tree-parity-sample.json");
        let content = std::fs::read_to_string(sample_path).expect("read wasm-tree-parity-sample.json");
        let sample_val: serde_json::Value = serde_json::from_str(&content).unwrap();

        let code = sample_val["code"].as_str().unwrap();
        let wasm_nodes = sample_val["wasmNodes"].as_array().unwrap();

        let lang = "typescript";
        let res = unsafe {
            cg_parse_tree(code.as_ptr(), code.len(), lang.as_ptr(), lang.len())
        };
        assert_eq!(unsafe { cg_result_status(res) }, CG_STATUS_SUCCESS);
        let buf = unsafe { cg_result_buffer(res, CG_SLOT_PAYLOAD) };
        let slice = unsafe { std::slice::from_raw_parts(buf.data, buf.len) };
        let native_val: serde_json::Value = serde_json::from_slice(slice).unwrap();

        assert_eq!(native_val["hasError"].as_bool().unwrap(), true, "Root should have hasError=true");

        let types = native_val["types"].as_array().unwrap();
        let fields = native_val["fields"].as_array().unwrap();
        let nodes = native_val["nodes"].as_array().unwrap();

        assert_eq!(nodes.len(), wasm_nodes.len(), "Total node count must match WASM tree exactly");

        for (i, (n, w)) in nodes.iter().zip(wasm_nodes.iter()).enumerate() {
            let t_idx = n["t"].as_u64().unwrap() as usize;
            let n_type = types[t_idx].as_str().unwrap();
            let w_type = w["type"].as_str().unwrap();
            assert_eq!(n_type, w_type, "node[{i}] type mismatch");

            let f_idx = n["f"].as_i64().unwrap();
            if f_idx >= 0 {
                let n_field = fields[f_idx as usize].as_str().unwrap();
                let w_field = w["field"].as_str().unwrap();
                assert_eq!(n_field, w_field, "node[{i}] field mismatch");
            } else {
                assert!(w["field"].is_null(), "node[{i}] expected no field but got {:?}", w["field"]);
            }

            let n_parent = n["p"].as_i64().unwrap();
            let w_parent = w["parent"].as_i64().unwrap();
            assert_eq!(n_parent, w_parent, "node[{i}] parent mismatch");

            let n_si = n["si"].as_u64().unwrap();
            let w_si = w["si"].as_u64().unwrap();
            assert_eq!(n_si, w_si, "node[{i}] startIndex mismatch");

            let n_ei = n["ei"].as_u64().unwrap();
            let w_ei = w["ei"].as_u64().unwrap();
            assert_eq!(n_ei, w_ei, "node[{i}] endIndex mismatch");

            let n_sr = n["sr"].as_u64().unwrap();
            let w_sr = w["sr"].as_u64().unwrap();
            assert_eq!(n_sr, w_sr, "node[{i}] startRow mismatch");

            let n_sc = n["sc"].as_u64().unwrap();
            let w_sc = w["sc"].as_u64().unwrap();
            assert_eq!(n_sc, w_sc, "node[{i}] startColumn mismatch");

            let n_er = n["er"].as_u64().unwrap();
            let w_er = w["er"].as_u64().unwrap();
            assert_eq!(n_er, w_er, "node[{i}] endRow mismatch");

            let n_ec = n["ec"].as_u64().unwrap();
            let w_ec = w["ec"].as_u64().unwrap();
            assert_eq!(n_ec, w_ec, "node[{i}] endColumn mismatch");

            let flags = n["flags"].as_u64().unwrap();
            let n_named = (flags & 1) != 0;
            let w_named = w["isNamed"].as_bool().unwrap();
            assert_eq!(n_named, w_named, "node[{i}] isNamed mismatch");

            let n_missing = (flags & 2) != 0;
            let w_missing = w["isMissing"].as_bool().unwrap();
            assert_eq!(n_missing, w_missing, "node[{i}] isMissing mismatch");

            let n_error = (flags & 4) != 0;
            let w_error = w["isError"].as_bool().unwrap();
            assert_eq!(n_error, w_error, "node[{i}] isError mismatch");

            let n_has_error = (flags & 8) != 0;
            let w_has_error = w["hasError"].as_bool().unwrap();
            assert_eq!(n_has_error, w_has_error, "node[{i}] hasError mismatch");
        }

        unsafe { cg_result_free(res) };
    }

    #[test]
    fn test_all_vendored_grammars_have_valid_licenses() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let grammars_dir = std::path::Path::new(manifest_dir).join("grammars");
        let vendored_dirs = [
            "kotlin",
            "lua",
            "scala",
            "dart",
            "arkts",
            "terraform",
            "objc",
            "solidity",
            "erlang",
            "pascal",
            "cfml_family",
            "nix",
            "cobol",
            "vbnet",
        ];

        for name in vendored_dirs {
            let dir = grammars_dir.join(name);
            assert!(dir.is_dir(), "Grammar dir does not exist: {:?}", dir);
            let license_path = dir.join("LICENSE");
            assert!(
                license_path.is_file(),
                "Grammar {:?} is missing LICENSE file at {:?}",
                name,
                license_path
            );

            let content = std::fs::read_to_string(&license_path)
                .unwrap_or_else(|e| panic!("Failed to read {:?}: {e}", license_path));

            assert!(
                content.len() >= 500,
                "LICENSE for {:?} is too short ({} bytes, expected >= 500 bytes of genuine license text)",
                name,
                content.len()
            );

            let lower = content.to_ascii_lowercase();
            assert!(
                lower.contains("copyright"),
                "LICENSE for {:?} must include Copyright notice",
                name
            );
            assert!(
                lower.contains("license") || lower.contains("permission is hereby granted"),
                "LICENSE for {:?} must contain legal grant/license terms",
                name
            );
        }
    }

    #[test]
    fn test_all_32_grammars_parity_with_wasm_dump() {
        let dump_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tools/wasm-grammars-dump.json");
        let dump_content = match std::fs::read_to_string(dump_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Warning: wasm-grammars-dump.json not found ({e}), skipping parity test");
                return;
            }
        };
        let wasm_dump: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&dump_content).unwrap();

        let mut mismatches = Vec::new();
        for (lang, wasm_val) in wasm_dump {
            let res = unsafe { cg_grammar_info(lang.as_ptr(), lang.len()) };
            if unsafe { cg_result_status(res) } != CG_STATUS_SUCCESS {
                mismatches.push(format!("{lang}: cg_grammar_info returned non-zero"));
                continue;
            }
            let buf = unsafe { cg_result_buffer(res, CG_SLOT_PAYLOAD) };
            let slice = unsafe { std::slice::from_raw_parts(buf.data, buf.len) };
            let native_val: serde_json::Value = serde_json::from_slice(slice).unwrap();

            let w_node_count = wasm_val["nodeKindCount"].as_u64().unwrap();
            let n_node_count = native_val["nodeKindCount"].as_u64().unwrap();
            let w_field_count = wasm_val["fieldCount"].as_u64().unwrap();
            let n_field_count = native_val["fieldCount"].as_u64().unwrap();

            let w_kinds = wasm_val["nodeKinds"].as_array().unwrap();
            let n_kinds = native_val["nodeKinds"].as_array().unwrap();
            let w_fields = wasm_val["fieldNames"].as_array().unwrap();
            let n_fields = native_val["fieldNames"].as_array().unwrap();

            if w_node_count != n_node_count || w_field_count != n_field_count || w_kinds != n_kinds || w_fields != n_fields {
                mismatches.push(format!(
                    "{lang}: wasm(nodes={w_node_count}, fields={w_field_count}) vs native(nodes={n_node_count}, fields={n_field_count})"
                ));
            }

            unsafe { cg_result_free(res) };
        }

        if !mismatches.is_empty() {
            eprintln!("Parity mismatches found ({} languages):", mismatches.len());
            for m in &mismatches {
                eprintln!("  - {m}");
            }
            panic!("Parity check failed for {} languages", mismatches.len());
        }
    }

    #[test]
    fn test_parse_tree_all_20_languages() {
        let snippets: &[(&str, &str)] = &[
            ("typescript", "export const x = 1;"),
            ("tsx", "export const App = () => <div/>;"),
            ("javascript", "const x = 1;"),
            ("jsx", "const App = () => <div/>;"),
            ("java", "class Test {}"),
            ("python", "x = 1"),
            ("go", "package main"),
            ("c", "int main() { return 0; }"),
            ("cpp", "int main() { return 0; }"),
            ("rust", "fn main() {}"),
            ("csharp", "class Program {}"),
            ("ruby", "def hello; end"),
            ("php", "<?php echo 1;"),
            ("swift", "let x = 1"),
            ("kotlin", "fun main() {}"),
            ("r", "x <- 1"),
            ("lua", "local x = 1"),
            ("luau", "local x: number = 1"),
            ("scala", "object Test {}"),
            ("dart", "void main() {}"),
        ];

        for &(lang, code) in snippets {
            let res = unsafe {
                cg_parse_tree(code.as_ptr(), code.len(), lang.as_ptr(), lang.len())
            };
            let status = unsafe { cg_result_status(res) };
            assert_eq!(
                status, CG_STATUS_SUCCESS,
                "Language {lang} cg_parse_tree failed with status {status}"
            );
            let buf = unsafe { cg_result_buffer(res, CG_SLOT_PAYLOAD) };
            assert!(!buf.data.is_null());
            let slice = unsafe { std::slice::from_raw_parts(buf.data, buf.len) };
            let val: serde_json::Value = serde_json::from_slice(slice)
                .unwrap_or_else(|e| panic!("Failed to parse JSON for {lang}: {e}"));
            assert_eq!(val["version"].as_u64().unwrap(), 1);
            assert_eq!(val["root"].as_i64().unwrap(), 0);
            assert!(val["nodes"].as_array().unwrap().len() > 0);
            unsafe { cg_result_free(res) };
        }
    }
}
