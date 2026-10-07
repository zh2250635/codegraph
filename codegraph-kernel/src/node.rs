//! Node.js / NAPI bindings for codegraph-kernel.
//!
//! Preserves existing Node interface for differential testing and oracle comparison.

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::core_api::{
    cfnptr_scan_files_core, cfnptr_strip_c_core, contract_info_core, extract_file_core,
    grammar_info_core, CfnptrFileInDto, CfnptrStructInDto,
};

/// The five flat tables for one file. See buffers.rs for the byte layout;
/// `src/extraction/kernel/layout.ts` is the TS mirror.
#[napi(object)]
pub struct ExtractBuffers {
    pub meta: Buffer,
    pub nodes: Buffer,
    pub edges: Buffer,
    pub refs: Buffer,
    pub arena: Buffer,
}

/// Wire-contract description — the TS loader verifies this against
/// src/types.ts before routing anything to the kernel, so an out-of-date
/// `.node` degrades to the wasm path instead of mis-decoding.
#[napi(object)]
pub struct ContractInfo {
    pub abi_version: u32,
    pub kernel_version: String,
    pub node_kinds: Vec<String>,
    pub edge_kinds: Vec<String>,
    /// Languages this binary can extract (routing is still TS-side policy).
    pub languages: Vec<String>,
}

/// Grammar identity for the grammar-source-parity gate: the wasm grammar and
/// the native grammar must expose identical node-kind/field tables, or
/// kernel-vs-fallback routing would be non-deterministic.
#[napi(object)]
pub struct GrammarInfo {
    pub abi_version: u32,
    pub node_kind_count: u32,
    pub field_count: u32,
    pub node_kinds: Vec<String>,
    pub field_names: Vec<String>,
}

#[napi]
pub fn contract_info() -> ContractInfo {
    let data = contract_info_core();
    ContractInfo {
        abi_version: data.abi_version,
        kernel_version: data.kernel_version,
        node_kinds: data.node_kinds,
        edge_kinds: data.edge_kinds,
        languages: data.languages,
    }
}

#[napi]
pub fn grammar_info(language: String) -> Option<GrammarInfo> {
    let info = grammar_info_core(&language)?;
    Some(GrammarInfo {
        abi_version: info.abi_version,
        node_kind_count: info.node_kind_count,
        field_count: info.field_count,
        node_kinds: info.node_kinds,
        field_names: info.field_names,
    })
}

/// One struct node's extent for the cFnPtr sweep (mirror of the TS caller's
/// `{ id, startLine, endLine }`, with `endLine ?? startLine` applied TS-side).
#[napi(object)]
pub struct CfnptrStructIn {
    pub id: String,
    pub start_line: u32,
    pub end_line: u32,
}

#[napi(object)]
pub struct CfnptrFileIn {
    /// RAW file text, exactly as the resolver's readFile returned it.
    pub text: String,
    pub structs: Vec<CfnptrStructIn>,
}

#[napi(object)]
pub struct CfnptrField {
    pub name: String,
    pub index: u32,
    pub ptr: bool,
    #[napi(js_name = "type")]
    pub ty: String,
}

#[napi(object)]
pub struct CfnptrStructOut {
    pub id: String,
    pub parsed: bool,
    pub fields: Vec<CfnptrField>,
}

/// The cFnPtr extraction-sweep facts for one file — see cfnptr.rs (and the
/// TS synthesizer's `FileFacts`) for field semantics.
#[napi(object)]
pub struct CfnptrFacts {
    pub fn_ptr_typedefs: Vec<String>,
    pub fn_type_typedefs: Vec<String>,
    pub structs: Vec<CfnptrStructOut>,
    pub inline_ptr: bool,
    pub inline_types: Vec<String>,
    pub inline_tags: Vec<String>,
    pub init_tokens: Vec<String>,
    pub array_elems: Vec<String>,
    pub alias_names: Vec<String>,
    pub d_pairs: Vec<String>,
    pub dispatch_fields: Vec<String>,
    pub array_dispatch_names: Vec<String>,
    pub includes: Vec<String>,
}

/// Batched cFnPtr extraction sweep (task #5 step 2): one call scans a batch
/// of files and returns their collected facts, amortizing the NAPI boundary.
/// Feature-detected by the TS loader — absent on older binaries, where the
/// synthesizer keeps its JS sweep.
#[napi]
pub fn cfnptr_scan_files(files: Vec<CfnptrFileIn>) -> Vec<CfnptrFacts> {
    let dtos: Vec<CfnptrFileInDto> = files
        .into_iter()
        .map(|f| CfnptrFileInDto {
            text: f.text,
            structs: f
                .structs
                .into_iter()
                .map(|s| CfnptrStructInDto {
                    id: s.id,
                    start_line: s.start_line,
                    end_line: s.end_line,
                })
                .collect(),
        })
        .collect();

    let facts_dtos = cfnptr_scan_files_core(&dtos);
    facts_dtos
        .into_iter()
        .map(|facts| CfnptrFacts {
            fn_ptr_typedefs: facts.fn_ptr_typedefs,
            fn_type_typedefs: facts.fn_type_typedefs,
            structs: facts
                .structs
                .into_iter()
                .map(|s| CfnptrStructOut {
                    id: s.id,
                    parsed: s.parsed,
                    fields: s
                        .fields
                        .into_iter()
                        .map(|fl| CfnptrField {
                            name: fl.name,
                            index: fl.index,
                            ptr: fl.ptr,
                            ty: fl.ty,
                        })
                        .collect(),
                })
                .collect(),
            inline_ptr: facts.inline_ptr,
            inline_types: facts.inline_types,
            inline_tags: facts.inline_tags,
            init_tokens: facts.init_tokens,
            array_elems: facts.array_elems,
            alias_names: facts.alias_names,
            d_pairs: facts.d_pairs,
            dispatch_fields: facts.dispatch_fields,
            array_dispatch_names: facts.array_dispatch_names,
            includes: facts.includes,
        })
        .collect()
}

/// Debug/differential hook: the native `stripCommentsForRegex(text, 'c')`.
/// Exists so the strip differential oracle can pin the Rust stripper against
/// the TS reference directly.
#[napi]
pub fn cfnptr_strip_c(text: String) -> String {
    let stripped = cfnptr_strip_c_core(text.as_bytes());
    String::from_utf8_lossy(&stripped).into_owned()
}

#[napi]
pub fn extract_file(
    file_path: String,
    content: String,
    language: String,
) -> napi::Result<ExtractBuffers> {
    let out = extract_file_core(&file_path, &content, &language)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;

    Ok(ExtractBuffers {
        meta: out.meta.into(),
        nodes: out.nodes.into(),
        edges: out.edges.into(),
        refs: out.refs.into(),
        arena: out.arena.into(),
    })
}
