package dev.markit

import org.json.JSONArray
import org.json.JSONObject
import java.io.Closeable

/** Span payload fields use the JSON export's names (`url`, `level`, `png`, ...). */
data class MarkitSpan(val start: Int, val end: Int, val kind: String, val fields: Map<String, Any> = emptyMap()) {
    fun string(key: String): String = fields[key]?.toString() ?: ""
    fun number(key: String): Int = when (val value = fields[key]) { is Number -> value.toInt(); is String -> value.toIntOrNull() ?: 0; else -> 0 }
    fun bool(key: String): Boolean = fields[key] == true
}
data class MarkitBlock(val start: Int, val end: Int, val kind: String)
data class MarkitDocument(val version: Int, val text: String, val spans: List<MarkitSpan>, val blocks: List<MarkitBlock>) {
    fun source(start: Int = 0, end: Int = text.length): String {
        require(start >= 0 && end >= start && end <= text.length)
        val output = StringBuilder(); var at = start
        for (span in spans.filter { it.kind == "math" && it.start >= start && it.end <= end }) {
            output.append(text,at,span.start); output.append(span.string("raw")); at = span.end
        }
        output.append(text,at,end); return output.toString()
    }
    companion object {
        internal fun decode(json: JSONObject): MarkitDocument {
            require(json.getInt("version") == 2) { "Unsupported document schema" }
            val text = json.getString("text")
            val spans = json.getJSONArray("spans")
            val blocks = json.getJSONArray("blocks")
            return MarkitDocument(2, text,
                List(spans.length()) { i -> val span = spans.getJSONObject(i)
                    val start = span.getInt("start"); val end = span.getInt("end")
                    require(start >= 0 && end >= start && end <= text.length)
                    val fields = HashMap<String, Any>()
                    for (key in span.keys()) if (key != "start" && key != "end" && key != "kind" && !span.isNull(key)) fields[key] = span.get(key)
                    MarkitSpan(start, end, span.getString("kind"), fields)
                }, List(blocks.length()) { i -> val block = blocks.getJSONObject(i)
                    val start = block.getInt("start"); val end = block.getInt("end")
                    require(start >= 0 && end >= start && end <= text.length)
                    MarkitBlock(start, end, block.getString("kind"))
                })
        }
    }
}
data class MarkitReference(val kind: String, val target: String)
/**
 * Which dialect a render uses. [DEFAULT] is the plainest (raw HTML shows as text);
 * [GITHUB] is for PR, issue and comment bodies: footnotes, alerts, an allowlisted
 * HTML subset, task spans with source offsets, forge references, emoji and diagrams.
 * [AGENT] is for coding agents' answers: everything [GITHUB] draws except forge
 * references (`@user`, `#123`, commit SHAs), which stay text.
 */
enum class MarkitFlavor(val code: Int) { DEFAULT(0), GITHUB(1), AGENT(2) }
data class MarkitPatch(val replaceFrom: Int, val documents: List<MarkitDocument>, val finalized: Boolean)

object Markit {
    init { System.loadLibrary("markit_ffi") }
    @JvmStatic private external fun nativeRender(source: String, format: Int): String
    @JvmStatic private external fun nativeRenderFlavor(source: String, format: Int, flavor: Int): String
    @JvmStatic external fun nativeCreateStream(): Long
    @JvmStatic external fun nativeCreateStreamFlavor(flavor: Int): Long
    @JvmStatic external fun nativeAppend(handle: Long, delta: String, finalized: Boolean): String
    /** MKNP v2 patch bytes. See docs/binary-transport.md. */
    @JvmStatic external fun nativeAppendPatch(handle: Long, delta: String, finalized: Boolean): ByteArray
    @JvmStatic external fun nativeFreeStream(handle: Long)
    @JvmStatic external fun nativeCreateLineHighlighter(language: String): Long
    @JvmStatic external fun nativeLineSetText(handle: Long, text: String)
    @JvmStatic external fun nativeLineEdit(handle: Long, startLine: Int, removedLines: Int, inserted: String)
    @JvmStatic external fun nativeLineTokens(handle: Long, startLine: Int, endLine: Int): String
    @JvmStatic external fun nativeFreeLineHighlighter(handle: Long)
    fun parse(source: String, flavor: MarkitFlavor = MarkitFlavor.DEFAULT): MarkitDocument =
        MarkitDocument.decode(JSONObject(if (flavor == MarkitFlavor.DEFAULT) nativeRender(source, 0) else nativeRenderFlavor(source, 0, flavor.code)))
    /** Colours the whole source (up to 8 MiB of UTF-8; larger sources stay plain). Unknown non-empty languages use a generic lexer; "" and "text" stay plain. */
    fun highlight(source: String, language: String, fullGrammar: Boolean = false): MarkitDocument = MarkitDocument.decode(JSONObject(nativeRender(JSONObject().put("source", source).put("language", language).toString(), if (fullGrammar) 3 else 2)))
}
/**
 * Patches cross JNI as MKNP v2 bytes instead of JSON. An open code fence sends
 * only its appended text and changed spans, applied to the block this stream
 * already holds, so a long live fence stays proportional to each append.
 */
class MarkitStream(val flavor: MarkitFlavor = MarkitFlavor.DEFAULT) : Closeable {
    private var handle = if (flavor == MarkitFlavor.DEFAULT) Markit.nativeCreateStream() else Markit.nativeCreateStreamFlavor(flavor.code)
    private val documents = ArrayList<MarkitDocument>()
    private var failed = false
    @Synchronized fun append(delta: String, finalized: Boolean = false): MarkitPatch {
        check(handle != 0L) { "Stream closed" }
        check(!failed) { "Stream failed; create a new stream" }
        val bytes = Markit.nativeAppendPatch(handle, delta, finalized)
        try {
            return NativePatchReader.apply(bytes, documents)
        } catch (error: RuntimeException) {
            failed = true
            throw error
        }
    }
    @Synchronized override fun close() { if (handle != 0L) { Markit.nativeFreeStream(handle); handle = 0 } }
}

data class MarkitToken(val text: String, val light: String, val dark: String)
/**
 * Incremental grammar highlighting for editors: set a whole file, report line
 * edits, then ask for the visible lines. Tokens cover a line without its line
 * ending. Requires a native build with the `grammars` feature, and a handle is
 * used from the thread that created it.
 */
class LineHighlighter(language: String) : Closeable {
    private var handle = Markit.nativeCreateLineHighlighter(language)
    init { check(handle != 0L) { "Line highlighting needs a grammars build" } }
    fun setText(text: String) {
        check(handle != 0L) { "Line highlighter closed" }
        Markit.nativeLineSetText(handle, text)
    }
    /** Replaces [removedLines] lines at [startLine] with [inserted], without line endings. */
    fun edit(startLine: Int, removedLines: Int, inserted: List<String>) {
        check(handle != 0L) { "Line highlighter closed" }
        val array = JSONArray()
        inserted.forEach(array::put)
        Markit.nativeLineEdit(handle, startLine, removedLines, array.toString())
    }
    /** One token list per line in `startLine until endLine`. */
    fun tokens(startLine: Int, endLine: Int): List<List<MarkitToken>> {
        check(handle != 0L) { "Line highlighter closed" }
        val lines = JSONArray(Markit.nativeLineTokens(handle, startLine, endLine))
        return List(lines.length()) { line ->
            val tokens = lines.getJSONArray(line)
            List(tokens.length()) { index ->
                val token = tokens.getJSONObject(index)
                MarkitToken(token.getString("text"), token.getString("light"), token.getString("dark"))
            }
        }
    }
    @Synchronized override fun close() { if (handle != 0L) { Markit.nativeFreeLineHighlighter(handle); handle = 0 } }
}
