import { test, expect } from '@playwright/test';

function perLine(tokens) {
  const lines = [[]];
  for (const token of tokens) {
    const parts = token.text.split('\n');
    for (let index = 0; index < parts.length; index++) {
      if (index > 0) lines.push([]);
      const ends = index + 1 < parts.length;
      const text = ends && parts[index].endsWith('\r') ? parts[index].slice(0, -1) : parts[index];
      if (!text) continue;
      const line = lines.at(-1), previous = line.at(-1);
      if (previous && previous.light === token.light && previous.dark === token.dark) previous.text += text;
      else line.push({ text, light: token.light, dark: token.dark });
    }
  }
  return lines;
}

test('rendering without LineHighlighter never fetches the grammar Wasm', async ({ page }) => {
  test.setTimeout(120000);
  const requests = [];
  page.on('request', request => requests.push(new URL(request.url()).pathname));
  await page.goto('/');
  await expect(page.locator('#status')).toContainText('Ready', { timeout: 60000 });
  await page.evaluate(() => window.renderer.render('# no grammar'));
  await expect(page.locator('#preview h1')).toHaveText('no grammar');
  expect(requests.some(path => path.startsWith('/pkg-grammars/'))).toBe(false);
});

test('grammar highlighting loads the grammar Wasm only when requested', async ({ page }) => {
  test.setTimeout(120000);
  const requests = [];
  page.on('request', request => requests.push(new URL(request.url()).pathname));
  await page.goto('/');
  await expect(page.locator('#status')).toContainText('Ready', { timeout: 60000 });
  expect(requests.some(path => path.startsWith('/pkg-grammars/'))).toBe(false);
  const tokens = await page.evaluate(async () => (await window.renderer.highlight('const value = 1;\n', 'ts', { grammar: true })).tokens);
  expect(tokens.length).toBeGreaterThan(1);
  expect(requests.some(path => path.startsWith('/pkg-grammars/'))).toBe(true);
});

test('LineHighlighter loads the grammar Wasm lazily in the shared worker and matches per-line tokens', async ({ page }) => {
  test.setTimeout(180000);
  const requests = [];
  page.on('request', request => requests.push(new URL(request.url()).pathname));
  await page.goto('/');
  await expect(page.locator('#status')).toContainText('Ready', { timeout: 60000 });
  const changedLine = '    let changed = "x";';
  const changed = `fn main() {\n${changedLine}\n    // note\n}\n`;
  const result = await page.evaluate(async changedLine => {
    const { LineHighlighter } = await import('/index.js');
    const source = 'fn main() {\n    let value = 1;\n    // note\n}\n';
    const highlighter = new LineHighlighter('rust');
    await highlighter.setText(source);
    await highlighter.edit(1, 1, [changedLine]);
    await highlighter.setBudget(1);
    const partial = await highlighter.tokens(0, 5);
    let more = true, guard = 0;
    while (more && guard++ < 100) more = await highlighter.advance(2);
    await highlighter.setBudget(2000);
    const tokens = await highlighter.tokens(0, 5);
    await highlighter.free();
    let rejected = false;
    try { await highlighter.tokens(0, 1); } catch { rejected = true; }
    return { partial, tokens, rejected };
  }, changedLine);
  // The highlighter, not the renderer, fetched the grammar module, and in the same worker.
  expect(requests.some(path => path.startsWith('/pkg-grammars/'))).toBe(true);
  expect(page.workers()).toHaveLength(1);
  expect(result.partial.some(line => line.length === 1 && line[0].light === '#24292f')).toBe(true);
  expect(result.rejected).toBe(true);
  const expected = await page.evaluate(async changed => (await window.renderer.highlight(changed, 'rust', { grammar: true })).tokens, changed);
  expect(result.tokens).toEqual(perLine(expected));
});
