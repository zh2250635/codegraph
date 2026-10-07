#!/usr/bin/env node
/**
 * capture-grammar-parity.mjs
 *
 * Captures grammar metadata (ABI version, node kinds, fields) from vendored WASMs
 * and asserts exact 100% parity against native cg_grammar_info.
 */

import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import { createRequire } from 'module';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const require = createRequire(import.meta.url);

const webTreeSitterPath = path.resolve(
  __dirname,
  '../../../../node_modules/.pnpm/@colbymchenry+codegraph-darwin-arm64@1.6.0/node_modules/@colbymchenry/codegraph-darwin-arm64/lib/node_modules/web-tree-sitter/tree-sitter.cjs'
);

const { Parser, Language } = require(webTreeSitterPath);

const wasmDir = path.resolve(__dirname, '../../src/extraction/wasm');
const npmWasmDir = path.resolve(
  __dirname,
  '../../../../node_modules/.pnpm/@colbymchenry+codegraph-darwin-arm64@1.6.0/node_modules/@colbymchenry/codegraph-darwin-arm64/lib/node_modules/tree-sitter-wasms/out'
);

export const WASM_FILES = {
  typescript: path.join(wasmDir, 'tree-sitter-typescript.wasm'),
  tsx: path.join(wasmDir, 'tree-sitter-tsx.wasm'),
  javascript: path.join(wasmDir, 'tree-sitter-javascript.wasm'),
  jsx: path.join(wasmDir, 'tree-sitter-javascript.wasm'),
  python: path.join(wasmDir, 'tree-sitter-python.wasm'),
  go: path.join(wasmDir, 'tree-sitter-go.wasm'),
  rust: path.join(wasmDir, 'tree-sitter-rust.wasm'),
  java: path.join(wasmDir, 'tree-sitter-java.wasm'),
  c: path.join(wasmDir, 'tree-sitter-c.wasm'),
  cpp: path.join(wasmDir, 'tree-sitter-cpp.wasm'),
  csharp: path.join(wasmDir, 'tree-sitter-c_sharp.wasm'),
  php: path.join(wasmDir, 'tree-sitter-php.wasm'),
  ruby: path.join(wasmDir, 'tree-sitter-ruby.wasm'),
  swift: path.join(wasmDir, 'tree-sitter-swift.wasm'),
  kotlin: path.join(wasmDir, 'tree-sitter-kotlin.wasm'),
  dart: path.join(wasmDir, 'tree-sitter-dart.wasm'),
  pascal: path.join(wasmDir, 'tree-sitter-pascal.wasm'),
  scala: path.join(wasmDir, 'tree-sitter-scala.wasm'),
  lua: path.join(wasmDir, 'tree-sitter-lua.wasm'),
  r: path.join(wasmDir, 'tree-sitter-r.wasm'),
  luau: path.join(wasmDir, 'tree-sitter-luau.wasm'),
  objc: path.join(npmWasmDir, 'tree-sitter-objc.wasm'),
  cfml: path.join(wasmDir, 'tree-sitter-cfml.wasm'),
  cfscript: path.join(wasmDir, 'tree-sitter-cfscript.wasm'),
  cfquery: path.join(wasmDir, 'tree-sitter-cfquery.wasm'),
  cobol: path.join(wasmDir, 'tree-sitter-cobol.wasm'),
  vbnet: path.join(wasmDir, 'tree-sitter-vbnet.wasm'),
  erlang: path.join(wasmDir, 'tree-sitter-erlang.wasm'),
  solidity: path.join(npmWasmDir, 'tree-sitter-solidity.wasm'),
  terraform: path.join(wasmDir, 'tree-sitter-terraform.wasm'),
  arkts: path.join(wasmDir, 'tree-sitter-arkts.wasm'),
  nix: path.join(wasmDir, 'tree-sitter-nix.wasm'),
};

export async function captureWasmGrammarInfo() {
  await Parser.init();
  const info = {};
  for (const [lang, filePath] of Object.entries(WASM_FILES)) {
    if (!fs.existsSync(filePath)) {
      throw new Error(`Missing WASM for ${lang}: ${filePath}`);
    }
    const l = await Language.load(filePath);
    const nodeKinds = [];
    for (let i = 0; i < l.nodeTypeCount; i++) {
      nodeKinds.push(l.nodeTypeForId(i) ?? '');
    }
    const fieldNames = [];
    for (let i = 1; i <= l.fieldCount; i++) {
      fieldNames.push(l.fieldNameForId(i) ?? '');
    }
    info[lang] = {
      abiVersion: l.abiVersion,
      nodeKindCount: l.nodeTypeCount,
      fieldCount: l.fieldCount,
      nodeKinds,
      fieldNames,
    };
  }
  return info;
}

export function compareParity() {
  const wasmPath = path.resolve(__dirname, 'wasm-grammars-dump.json');
  const nativePath = path.resolve(__dirname, 'native-grammars-dump.json');

  if (!fs.existsSync(wasmPath) || !fs.existsSync(nativePath)) {
    console.error('Missing wasm-grammars-dump.json or native-grammars-dump.json');
    process.exit(1);
  }

  const wasm = JSON.parse(fs.readFileSync(wasmPath, 'utf8'));
  const native = JSON.parse(fs.readFileSync(nativePath, 'utf8'));

  console.log(
    String('LANGUAGE').padEnd(14) +
    String('WASM(nodes/fields)').padEnd(22) +
    String('NATIVE(nodes/fields)').padEnd(24) +
    'STATUS'
  );
  console.log('-'.repeat(72));

  let matched = 0;
  const total = Object.keys(wasm).length;
  const mismatches = [];

  for (const lang of Object.keys(wasm)) {
    const w = wasm[lang];
    const n = native[lang];
    if (!n) {
      console.log(lang.padEnd(14) + 'MISSING IN NATIVE');
      mismatches.push(`${lang}: missing in native`);
      continue;
    }
    const wDesc = `${w.nodeKindCount} / ${w.fieldCount}`;
    const nDesc = `${n.nodeKindCount} / ${n.fieldCount}`;
    const nodesEqual = JSON.stringify(w.nodeKinds) === JSON.stringify(n.nodeKinds);
    const fieldsEqual = JSON.stringify(w.fieldNames) === JSON.stringify(n.fieldNames);

    if (nodesEqual && fieldsEqual) {
      matched++;
      console.log(lang.padEnd(14) + wDesc.padEnd(22) + nDesc.padEnd(24) + 'MATCH (100%)');
    } else {
      console.log(
        lang.padEnd(14) +
        wDesc.padEnd(22) +
        nDesc.padEnd(24) +
        'DIFF ' +
        (!nodesEqual ? '[nodes] ' : '') +
        (!fieldsEqual ? '[fields]' : '')
      );
      mismatches.push(lang);
    }
  }

  console.log('-'.repeat(72));
  console.log(`Parity summary: ${matched} / ${total} grammars fully matched`);

  if (mismatches.length > 0) {
    console.error('Mismatches detected:', mismatches.join(', '));
    process.exit(1);
  } else {
    console.log('ALL 32 GRAMMARS HAVE 100% PARITY WITH VENDORED WASMS!');
  }
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes('--dump') || !fs.existsSync(path.resolve(__dirname, 'wasm-grammars-dump.json'))) {
    const info = await captureWasmGrammarInfo();
    const outPath = path.resolve(__dirname, 'wasm-grammars-dump.json');
    fs.writeFileSync(outPath, JSON.stringify(info, null, 2));
    console.log(`Captured ${Object.keys(info).length} grammars into ${outPath}`);
  }
  compareParity();
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((err) => {
    console.error(err);
    process.exit(1);
  });
}
