package dev.markit

import org.json.JSONObject

data class MarkitSpan(
    val start: Int,
    val end: Int,
    val kind: String,
    val level: Int?,
    val depth: Int?,
    val language: String?,
    val url: String?,
)

data class MarkitDocument(val version: Int, val text: String, val spans: List<MarkitSpan>)

object Markit {
    init { System.loadLibrary("markit_ffi") }

    @JvmStatic
    private external fun nativeRender(source: String): String

    /** Synchronous parsing. Dispatch large documents off the UI thread. */
    fun parse(source: String): MarkitDocument {
        val json = JSONObject(nativeRender(source))
        val spans = json.getJSONArray("spans")
        return MarkitDocument(
            version = json.getInt("version"),
            text = json.getString("text"),
            spans = List(spans.length()) { index ->
                val span = spans.getJSONObject(index)
                MarkitSpan(
                    start = span.getInt("start"), end = span.getInt("end"),
                    kind = span.getString("kind"),
                    level = if (span.has("level")) span.getInt("level") else null,
                    depth = if (span.has("depth")) span.getInt("depth") else null,
                    language = if (span.has("language")) span.getString("language") else null,
                    url = if (span.has("url")) span.getString("url") else null,
                )
            },
        )
    }
}
