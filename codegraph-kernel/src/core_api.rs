//! Core extraction logic decoupled from Node / NAPI bindings.
//!
//! Shared across NAPI bindings and C ABI to guarantee identical behavior
//! and single-source-of-truth extraction semantics.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractInfoData {
    pub abi_version: u32,
    pub kernel_version: String,
    pub node_kinds: Vec<String>,
    pub edge_kinds: Vec<String>,
    pub languages: Vec<String>,
}

pub fn contract_info_core() -> ContractInfoData {
    ContractInfoData {
        abi_version: crate::buffers::KERNEL_ABI_VERSION as u32,
        kernel_version: env!("CARGO_PKG_VERSION").to_string(),
        node_kinds: crate::buffers::NODE_KINDS.iter().map(|s| s.to_string()).collect(),
        edge_kinds: crate::buffers::EDGE_KINDS.iter().map(|s| s.to_string()).collect(),
        languages: crate::langs::LANGUAGES.iter().map(|s| s.to_string()).collect(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrammarInfoData {
    pub abi_version: u32,
    pub node_kind_count: u32,
    pub field_count: u32,
    pub node_kinds: Vec<String>,
    pub field_names: Vec<String>,
}

pub fn grammar_info_core(language: &str) -> Option<GrammarInfoData> {
    let lang = crate::langs::grammar_for(language)?;
    let node_kind_count = lang.node_kind_count();
    let field_count = lang.field_count();
    let node_kinds = (0..node_kind_count)
        .map(|i| lang.node_kind_for_id(i as u16).unwrap_or("").to_string())
        .collect();
    let field_names = (1..=field_count)
        .map(|i| lang.field_name_for_id(i as u16).unwrap_or("").to_string())
        .collect();
    Some(GrammarInfoData {
        abi_version: lang.abi_version() as u32,
        node_kind_count: node_kind_count as u32,
        field_count: field_count as u32,
        node_kinds,
        field_names,
    })
}

#[derive(Debug)]
pub enum ExtractError {
    UnsupportedLanguage(String),
    ExtractionFailure(String),
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractError::UnsupportedLanguage(msg) => write!(f, "{msg}"),
            ExtractError::ExtractionFailure(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ExtractError {}

pub fn extract_file_core(
    file_path: &str,
    content: &str,
    language: &str,
) -> Result<crate::buffers::EmitOut, ExtractError> {
    if !crate::langs::LANGUAGES.contains(&language) {
        return Err(ExtractError::UnsupportedLanguage(format!(
            "unsupported language: {language}"
        )));
    }

    let out = crate::stack::run_guarded(|| match language {
        "java" => crate::java::extract(file_path, content),
        "python" => crate::python::extract(file_path, content),
        "go" => crate::go::extract(file_path, content),
        "c" | "cpp" => crate::ccpp::extract(file_path, content, language),
        "rust" => crate::rustlang::extract(file_path, content),
        "csharp" => crate::csharp::extract(file_path, content),
        "ruby" => crate::ruby::extract(file_path, content),
        "php" => crate::php::extract(file_path, content),
        "swift" => crate::swift::extract(file_path, content),
        "kotlin" => crate::kotlin::extract(file_path, content),
        "r" => crate::rlang::extract(file_path, content),
        "lua" | "luau" => crate::lua::extract(file_path, content, language),
        "scala" => crate::scala::extract(file_path, content),
        "dart" => crate::dart::extract(file_path, content),
        _ => crate::tsjs::extract(file_path, content, language),
    })
    .map_err(ExtractError::ExtractionFailure)?;

    Ok(out)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfnptrStructInDto {
    pub id: String,
    #[serde(alias = "start_line")]
    pub start_line: u32,
    #[serde(alias = "end_line")]
    pub end_line: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfnptrFileInDto {
    pub text: String,
    #[serde(default)]
    pub structs: Vec<CfnptrStructInDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CfnptrFieldDto {
    pub name: String,
    pub index: u32,
    pub ptr: bool,
    #[serde(rename = "type")]
    pub ty: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CfnptrStructOutDto {
    pub id: String,
    pub parsed: bool,
    pub fields: Vec<CfnptrFieldDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CfnptrFactsDto {
    pub fn_ptr_typedefs: Vec<String>,
    pub fn_type_typedefs: Vec<String>,
    pub structs: Vec<CfnptrStructOutDto>,
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

pub fn scan_single_file(text: &str, structs: &[CfnptrStructInDto]) -> CfnptrFactsDto {
    let s_extents: Vec<crate::cfnptr::StructExtent> = structs
        .iter()
        .map(|s| crate::cfnptr::StructExtent {
            id: s.id.clone(),
            start_line: s.start_line,
            end_line: s.end_line,
        })
        .collect();
    let facts = crate::cfnptr::scan_file(text, &s_extents);
    CfnptrFactsDto {
        fn_ptr_typedefs: facts.fn_ptr_typedefs,
        fn_type_typedefs: facts.fn_type_typedefs,
        structs: facts
            .structs
            .into_iter()
            .map(|s| CfnptrStructOutDto {
                id: s.id,
                parsed: s.parsed,
                fields: s
                    .fields
                    .into_iter()
                    .map(|fl| CfnptrFieldDto {
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
    }
}

pub fn cfnptr_scan_files_core(files: &[CfnptrFileInDto]) -> Vec<CfnptrFactsDto> {
    files
        .iter()
        .map(|f| scan_single_file(&f.text, &f.structs))
        .collect()
}

pub fn cfnptr_strip_c_core(text: &[u8]) -> Vec<u8> {
    crate::cfnptr::strip_c(text)
}
