//! Stable C ABI implementation for CodeGraph native extraction kernel.
//!
//! Exposes an ownership-explicit, panic-bounded C interface (v1) for Go cgo
//! and non-Node runtimes. Callers must uphold pointer and lifetime rules;
//! argument guards do not make arbitrary foreign memory accesses safe.

use std::panic::catch_unwind;

use crate::core_api::{
    cfnptr_scan_files_core, cfnptr_strip_c_core, contract_info_core, extract_file_core,
    grammar_info_core, CfnptrFileInDto, ExtractError,
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
}
