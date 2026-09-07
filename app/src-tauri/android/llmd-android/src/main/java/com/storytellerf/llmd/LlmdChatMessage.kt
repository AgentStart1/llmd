package com.storytellerf.llmd

data class LlmdChatMessage(
    val role: String,
    val content: List<LlmdChatContent>,
) {
    val text: String
        get() = content.filterIsInstance<LlmdChatContent.Text>().joinToString("\n") { it.value }
}

sealed interface LlmdChatContent {
    data class Text(val value: String) : LlmdChatContent

    data class Image(val bytes: ByteArray, val mimeType: String) : LlmdChatContent {
        override fun equals(other: Any?): Boolean =
            other is Image && mimeType == other.mimeType && bytes.contentEquals(other.bytes)

        override fun hashCode(): Int = 31 * bytes.contentHashCode() + mimeType.hashCode()
    }
}

internal const val MAX_IMAGES_PER_REQUEST = 1
