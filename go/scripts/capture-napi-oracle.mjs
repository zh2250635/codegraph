#!/usr/bin/env node
/**
 * Capture the v1.6.0 NAPI boundary, not the higher-level TS/WASM pipeline.
 * The checked-in oracle lets Go replay the decoder without Node or Rust.
 *
 * Usage: node go/scripts/capture-napi-oracle.mjs <v1.6.0-platform-package>/lib
 * Use an unmodified release bundle; the source revision is pinned below.
 */
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const lib = process.argv[2] && path.resolve(process.argv[2]);
if (!lib) throw new Error('expected a v1.6.0 platform package lib directory');
const require = createRequire(import.meta.url);
const manifest = JSON.parse(fs.readFileSync(path.join(lib, 'package.json'), 'utf8'));
if (manifest.version !== '1.6.0') throw new Error('oracle must be CodeGraph 1.6.0');
const native = require(path.join(lib, 'kernel/codegraph-kernel.node'));
const { decodeExtractBuffers } = require(path.join(lib, 'dist/extraction/kernel/decode.js'));
const contract = native.contractInfo();

const extensions = new Map([
  ['.ts', 'typescript'], ['.tsx', 'tsx'], ['.js', 'javascript'], ['.jsx', 'jsx'],
  ['.java', 'java'], ['.py', 'python'], ['.go', 'go'], ['.c', 'c'],
  ['.cpp', 'cpp'], ['.hpp', 'cpp'], ['.rs', 'rust'], ['.cs', 'csharp'],
  ['.rb', 'ruby'], ['.php', 'php'], ['.module', 'php'], ['.swift', 'swift'],
  ['.kt', 'kotlin'], ['.kts', 'kotlin'], ['.r', 'r'], ['.lua', 'lua'],
  ['.luau', 'luau'], ['.scala', 'scala'], ['.sc', 'scala'], ['.dart', 'dart'],
]);
const corpus = path.join(root, '__tests__/fixtures/kernel-parity');
const inputs = fs.readdirSync(corpus).sort().map(name => ({
  filePath: '__tests__/fixtures/kernel-parity/' + name,
  language: extensions.get(path.extname(name).toLowerCase()),
  content: fs.readFileSync(path.join(corpus, name), 'utf8'),
}));
inputs.push(
  {filePath: 'oracle/probe.ts', language: 'typescript',
    content: 'export interface 服务 { run(): void }\nexport class Worker implements 服务 {\nasync run() { return await task(0); }\n}\n'},
  {filePath: 'oracle/probe.jsx', language: 'jsx',
    content: 'export function Page() { return <div>Hello</div>; }\n'},
  {filePath: 'oracle/unicode.go', language: 'go',
    content: 'package main\n// 问候 keeps UTF-8 columns.\nfunc 问候() { println("您好🙂") }\n'},
  {filePath: 'oracle/broken.py', language: 'python', content: 'def broken(:\n    pass\n'},
);
for (const language of contract.languages) {
  inputs.push({filePath: 'oracle/empty-' + language, language, content: ''});
}

function capture(input) {
  if (!input.language) throw new Error('no language for ' + input.filePath);
  try {
    const raw = native.extractFile(input.filePath, input.content, input.language);
    const expected = decodeExtractBuffers(raw, input.filePath, input.language);
    // Timings are non-semantic. All graph fields and array order are retained.
    expected.durationMs = 0;
    for (const node of expected.nodes) node.updatedAt = 0;
    const meta = Buffer.from(raw.meta);
    meta.writeDoubleLE(0, 28);
    const buffers = {...raw, meta};
    return {...input, expected, buffers: Object.fromEntries(
      Object.entries(buffers).map(([key, data]) => [key, data.toString('base64')])
    )};
  } catch (error) {
    return {...input, error: error.message};
  }
}

const cfnptrInputs = [
  [],
  [{text: '', structs: []}],
  [{
    text: '#include "ops.h"\ntypedef int (*handler_t)(int);\nstruct Ops {\n handler_t run;\n int (*stop)(void);\n int value;\n};\nstruct Ops ops = { .run = start, .stop = finish };\n',
    structs: [{id: 'Ops', startLine: 3, endLine: 7}],
  }],
  [{
    text: 'typedef void callback_t(void);\nstatic callback_t *callback;\n// comment\nchar *literal = "/* not a comment */";\n',
    structs: [],
  }],
];
const stripInputs = [
  '', '// 汉字🙂\nint x; /* trailing */\n',
  'char *s = "/* literal */"; // gone\nchar c = \'/\';\n',
  '/* multiline\n comment */\n#define TEXT "// keeps literal"\n',
];
const oracle = {
  release: '1.6.0',
  sourceRevision: 'dfccdf62547fcd76d343344d823a0e1998d3a89f',
  scope: 'NAPI extraction boundary only; excludes TS preParse/post/WASM routing and all graph services',
  contract,
  grammars: Object.fromEntries(contract.languages.map(lang => [lang, native.grammarInfo(lang)])),
  cases: inputs.map(capture),
  cfnptr: cfnptrInputs.map(input => ({input, expected: native.cfnptrScanFiles(input)})),
  strips: stripInputs.map(input => ({input, expected: native.cfnptrStripC(input)})),
};
const supported = new Set(oracle.cases.filter(c => c.expected).map(c => c.language));
for (const lang of contract.languages) {
  if (!supported.has(lang)) throw new Error('oracle has no successful case for ' + lang);
}
const destination = path.join(root, 'go/kernel/testdata/napi-v1.6.0.json');
fs.mkdirSync(path.dirname(destination), {recursive: true});
fs.writeFileSync(destination, JSON.stringify(oracle, null, 2) + '\n');
console.log(JSON.stringify({
  output: destination, bytes: fs.statSync(destination).size,
  languages: supported.size,
  success: oracle.cases.filter(c => c.expected).length,
  deferred: oracle.cases.filter(c => c.error).length,
}));
