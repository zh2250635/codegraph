//! Tree-sitter AST extraction in UTF-16 input recovery mode for Go/cgo consumers.
//!
//! Conforms to the wire contract:
//! {"version":1,"types":[...],"fields":[...],"nodes":[{"t":...,"p":...,"f":...,"sb":...,"eb":...,"si":...,"ei":...,"sr":...,"sc":...,"er":...,"ec":...,"flags":...}],"root":0,"hasError":bool}

use std::collections::HashMap;
use std::time::{Duration, Instant};
use serde::Serialize;
use tree_sitter::{Parser, ParseOptions};

pub const MAX_NODES: usize = 1_000_000;
pub const DEFAULT_PARSE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize)]
pub struct ParseTreeDto {
    pub version: u32,
    pub types: Vec<String>,
    pub fields: Vec<String>,
    pub nodes: Vec<ParseTreeNodeDto>,
    pub root: i32,
    #[serde(rename = "hasError")]
    pub has_error: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseTreeNodeDto {
    pub t: u32,
    pub p: i32,
    pub f: i32,
    pub sb: u32,
    pub eb: u32,
    pub si: u32,
    pub ei: u32,
    pub sr: u32,
    pub sc: u32,
    pub er: u32,
    pub ec: u32,
    pub flags: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParserLanguagesDto {
    pub languages: Vec<String>,
}

/// Builds a lookup table from UTF-16 code-unit index to UTF-8 byte index.
pub fn build_utf16_to_utf8_map(source: &str, total_utf16_len: usize) -> Vec<usize> {
    let mut map = Vec::with_capacity(total_utf16_len + 1);
    let mut u8_idx = 0usize;
    for ch in source.chars() {
        let u8_len = ch.len_utf8();
        let u16_len = ch.len_utf16();
        map.push(u8_idx);
        if u16_len == 2 {
            map.push(u8_idx);
        }
        u8_idx += u8_len;
    }
    map.push(u8_idx);
    map
}

/// Parses UTF-8 source for the requested language using tree-sitter with UTF-16 input.
pub fn parse_tree(source: &str, language: &str) -> Result<ParseTreeDto, String> {
    let grammar = crate::langs::grammar_for(language)
        .ok_or_else(|| format!("unsupported language: {language}"))?;

    let utf16_units: Vec<u16> = source.encode_utf16().collect();
    let utf16_to_utf8 = build_utf16_to_utf8_map(source, utf16_units.len());

    let mut parser = Parser::new();
    parser
        .set_language(&grammar)
        .map_err(|e| format!("failed to set grammar for {language}: {e}"))?;

    let start_time = Instant::now();
    let mut progress = |_: &tree_sitter::ParseState| -> bool {
        start_time.elapsed() <= DEFAULT_PARSE_TIMEOUT
    };
    let mut parse_options = ParseOptions::new();
    parse_options = parse_options.progress_callback(&mut progress);

    let len = utf16_units.len();
    let tree = parser
        .parse_utf16_le_with_options(
            &mut |i, _| (i < len).then(|| &utf16_units[i..]).unwrap_or_default(),
            None,
            Some(parse_options),
        )
        .ok_or_else(|| format!("tree-sitter parse failure or timeout for {language}"))?;

    let root_node = tree.root_node();
    let has_error = root_node.has_error();

    let mut types: Vec<String> = Vec::new();
    let mut type_to_idx: HashMap<String, u32> = HashMap::new();
    let mut fields: Vec<String> = Vec::new();
    let mut field_to_idx: HashMap<String, u32> = HashMap::new();
    let mut nodes: Vec<ParseTreeNodeDto> = Vec::new();

    let mut cursor = tree.walk();
    let mut parent_stack: Vec<i32> = Vec::new();

    'walk: loop {
        let node = cursor.node();
        let kind = node.kind();
        let type_idx = *type_to_idx.entry(kind.to_string()).or_insert_with(|| {
            let idx = types.len() as u32;
            types.push(kind.to_string());
            idx
        });

        let field_idx = if let Some(field_name) = cursor.field_name() {
            let idx = *field_to_idx.entry(field_name.to_string()).or_insert_with(|| {
                let idx = fields.len() as u32;
                fields.push(field_name.to_string());
                idx
            });
            idx as i32
        } else {
            -1
        };

        let current_idx = nodes.len() as i32;
        if current_idx >= MAX_NODES as i32 {
            return Err(format!("node limit exceeded ({MAX_NODES})"));
        }

        let parent_idx = parent_stack.last().copied().unwrap_or(-1);

        let si = (node.start_byte() / 2) as u32;
        let ei = (node.end_byte() / 2) as u32;
        let sb = utf16_to_utf8.get(si as usize).copied().unwrap_or(0) as u32;
        let eb = utf16_to_utf8.get(ei as usize).copied().unwrap_or(source.len()) as u32;
        let sp = node.start_position();
        let ep = node.end_position();
        let sr = sp.row as u32;
        let sc = (sp.column / 2) as u32;
        let er = ep.row as u32;
        let ec = (ep.column / 2) as u32;

        let mut flags = 0u32;
        if node.is_named() {
            flags |= 1;
        }
        if node.is_missing() {
            flags |= 2;
        }
        if node.is_error() {
            flags |= 4;
        }
        if node.has_error() {
            flags |= 8;
        }

        nodes.push(ParseTreeNodeDto {
            t: type_idx,
            p: parent_idx,
            f: field_idx,
            sb,
            eb,
            si,
            ei,
            sr,
            sc,
            er,
            ec,
            flags,
        });

        if cursor.goto_first_child() {
            parent_stack.push(current_idx);
            continue 'walk;
        }

        if cursor.goto_next_sibling() {
            continue 'walk;
        }

        loop {
            if !cursor.goto_parent() {
                break 'walk;
            }
            parent_stack.pop();
            if cursor.goto_next_sibling() {
                continue 'walk;
            }
        }
    }

    Ok(ParseTreeDto {
        version: 1,
        types,
        fields,
        nodes,
        root: 0,
        has_error,
    })
}

/// Returns the currently supported parser languages for tree generation.
pub fn parser_languages() -> ParserLanguagesDto {
    ParserLanguagesDto {
        languages: crate::langs::PARSER_LANGUAGES.iter().map(|s| s.to_string()).collect(),
    }
}
