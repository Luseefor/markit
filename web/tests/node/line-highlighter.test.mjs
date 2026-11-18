// Node + jsdom: the package entry, the inline Wasm engine, and the shared
// LineHighlighter wrapper. Run with `npm run test:node`.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { JSDOM } from 'jsdom';

const wasm = await readFile(new URL('../../pkg/markit_wasm_bg.wasm', import.meta.url));
const { LineHighlighter, MarkitRenderer, preloadMarkit } = await import('../../index.js');
preloadMarkit({ wasm });

function perLine(tokens) {
  const lines = [[]];
  for (const token of tokens) {
    const parts = token.text.split('\n');
    for (let index = 0; index < parts.length; index++) {
      if (index > 0) lines.push([]);
      const last = index + 1 < parts.length;
      const text = last && parts[index].endsWith('\r') ? parts[index].slice(0, -1) : parts[index];
      if (!text) continue;
      const line = lines.at(-1), previous = line.at(-1);
      if (previous && previous.light === token.light && previous.dark === token.dark) previous.text += text;
      else line.push({ text, light: token.light, dark: token.dark });
    }
  }
  return lines;
}

test('LineHighlighter exposes incremental grammar tokens and frees', async () => {
  const { window } = new JSDOM('<!doctype html><body></body>');
  const element = window.document.createElement('div');
  window.document.body.append(element);
  const renderer = new MarkitRenderer(element, { wasm });
  const source = 'fn main() {\n    let value = 1;\n    // note\n}\n';
  const changedLine = '    let changed = "x";';
  const changed = `fn main() {\n${changedLine}\n    // note\n}\n`;
  const expected = perLine((await renderer.highlight(changed, 'rust', { grammar: true })).tokens);

  const highlighter = new LineHighlighter('rust', { wasm });
  await highlighter.setText(source);
  await highlighter.edit(1, 1, [changedLine]);
  await highlighter.setBudget(1);
  const partial = await highlighter.tokens(0, expected.length);
  assert.ok(partial.some(line => line.length === 1 && line[0].light === '#24292f'));

  let more = true, guard = 0;
  while (more && guard++ < 100) more = await highlighter.advance(2);
  assert.equal(more, false);
  await highlighter.setBudget(2000);
  assert.deepEqual(await highlighter.tokens(0, expected.length), expected);

  await highlighter.free();
  await assert.rejects(highlighter.tokens(0, 1), /disposed/);
  renderer.destroy();
});
