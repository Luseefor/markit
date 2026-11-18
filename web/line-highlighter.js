import { engineFor } from './engine.js';

/**
 * Incremental grammar highlighting for editors. It uses the same shared
 * worker, Wasm instance, and preload path as MarkitRenderer; pass the same
 * `workerURL`, `worker`, and `wasm` options to share an instance.
 *
 * Lines split on `\n` only. Tokens cover a line without its ending, and
 * `tokens(0, n)` returns one list per line, including a final empty line after
 * a trailing newline.
 */
export class LineHighlighter {
  #engine; #ready; #id; #disposed = false;

  /** `language` names a grammar, for example `"rust"` or `"ts"`. */
  constructor(language, options = {}) {
    if (typeof language !== 'string') throw new TypeError('Language must be a string');
    this.#engine = engineFor(options);
    this.#ready = this.#engine.call(this, 'lineNew', { language }).then(id => { this.#id = id; });
    // A failed load is reported by the first method that awaits it.
    this.#ready.catch(() => {});
  }

  async #call(type, fields) {
    if (this.#disposed) throw new Error('Line highlighter disposed');
    await this.#ready;
    if (this.#disposed) throw new Error('Line highlighter disposed');
    return this.#engine.call(this, type, { highlighter: this.#id, ...fields });
  }

  /** Replaces the whole text. A `\r` before a `\n` is part of that line's ending. */
  setText(text) {
    return this.#call('lineSetText', { text }).then(() => undefined);
  }

  /**
   * Replaces `removedLines` lines at `startLine` with `inserted`. Inserted
   * entries carry no line endings; an entry containing `\n` becomes several
   * lines.
   */
  edit(startLine, removedLines, inserted) {
    if (!Array.isArray(inserted) || inserted.some(line => typeof line !== 'string')) {
      return Promise.reject(new TypeError('inserted must be an array of strings'));
    }
    return this.#call('lineEdit', { startLine, removedLines, inserted }).then(() => undefined);
  }

  /**
   * Tokens for the lines in `startLine..endLine`, one list per line:
   * `{ text, light, dark }`. Lines that a budget did not reach are plain.
   */
  tokens(startLine, endLine) {
    return this.#call('lineTokens', { startLine, endLine });
  }

  /** Sets how many lines one `tokens` call may parse. The default is 2,000. */
  setBudget(budget) {
    return this.#call('lineSetBudget', { budget }).then(() => undefined);
  }

  /** Parses up to `maxLines` more lines; resolves true while work remains. */
  advance(maxLines) {
    return this.#call('lineAdvance', { maxLines });
  }

  /** Releases the native highlighter. Safe to call more than once. */
  async free() {
    if (this.#disposed) return;
    this.#disposed = true;
    try {
      await this.#ready;
      await this.#engine.call(this, 'lineFree', { id: this.#id });
    } catch {
      // A failed load or a closed worker has nothing left to free.
    }
    if (!this.#engine.shared) this.#engine.terminate();
  }

  /** Frees the native highlighter. */
  dispose() {
    return this.free();
  }
}

if (typeof Symbol.dispose === 'symbol') {
  LineHighlighter.prototype[Symbol.dispose] = LineHighlighter.prototype.dispose;
}
