#!/usr/bin/env node
/**
 * Verify the optional NAPI binding against the checked-in release oracle.
 * This is a migration compatibility check only, not a native runtime dependency.
 * Usage: node go/scripts/check-napi.mjs <new-codegraph-kernel.node>
 */
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
if (!process.argv[2]) throw new Error('expected a freshly built .node module path');
const native = require(path.resolve(process.argv[2]));
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const oracle = JSON.parse(fs.readFileSync(path.join(root, 'go/kernel/testdata/napi-v1.6.0.json'), 'utf8'));
assert.deepEqual(Object.keys(native).sort(), [
  'cfnptrScanFiles', 'cfnptrStripC', 'contractInfo', 'extractFile', 'grammarInfo',
].sort());
assert.deepEqual(native.contractInfo(), oracle.contract);
for (const [language, grammar] of Object.entries(oracle.grammars)) {
  assert.deepEqual(native.grammarInfo(language), grammar, language + ' grammar differs');
}
assert.equal(native.grammarInfo('not-a-language'), null);
for (const entry of oracle.cases) {
  if (entry.error) {
    assert.throws(() => native.extractFile(entry.filePath, entry.content, entry.language),
      error => error.message === entry.error, entry.filePath);
    continue;
  }
  const raw = native.extractFile(entry.filePath, entry.content, entry.language);
  raw.meta.writeDoubleLE(0, 28);
  for (const [name, expected] of Object.entries(entry.buffers)) {
    assert.deepEqual(raw[name], Buffer.from(expected, 'base64'), entry.filePath + ' ' + name);
  }
}
for (const entry of oracle.cfnptr) {
  assert.deepEqual(native.cfnptrScanFiles(entry.input), entry.expected);
}
for (const entry of oracle.strips) {
  assert.equal(native.cfnptrStripC(entry.input), entry.expected);
}
console.log('Optional NAPI compatibility passed: 20 languages, 62 cases, grammar and cFnPtr');
