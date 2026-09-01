package com.storytellerf.llmd

import android.content.Context
import android.util.Base64
import java.io.File
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject

object LlmdAndroidBridge {
    private val providerMutex = Mutex()
    private var appContext: Context? = null
    private var provider: AndroidLiteRtProvider? = null
    private var selectedModelPath = ""

    fun configure(context: Context) {
        val applicationContext = context.applicationContext
        appContext = applicationContext
        selectedModelPath = defaultModelFile(applicationContext).absolutePath
    }

    suspend fun initialize(context: Context) = providerMutex.withLock {
        configure(context)
        if (provider == null) {
            provider = AndroidLiteRtProvider(context.applicationContext.cacheDir.absolutePath) {
                android.util.Log.i("llmd", it)
            }
        }
    }

    suspend fun close() = providerMutex.withLock {
        provider?.close()
        provider = null
    }

    suspend fun listModels(): List<String> = providerMutex.withLock { listModelsSync() }

    suspend fun deleteModel(model: String) = providerMutex.withLock {
        require(model == DEFAULT_MODEL) { "Unsupported model: $model" }
        val modelFile = File(selectedModelPath)
        require(modelFile.isUsableModelFile()) { "Model file does not exist: $selectedModelPath" }

        provider?.close()
        provider = null
        require(modelFile.delete()) { "Unable to delete model file: $selectedModelPath" }
    }

    fun listModelsJson(): String = JSONArray(listModelsSync()).toString()

    fun modelStateJson(): String {
        val path = selectedModelPath.ifBlank {
            appContext?.let { defaultModelFile(it).absolutePath }.orEmpty()
        }
        return JSONObject()
            .put("defaultModel", DEFAULT_MODEL)
            .put("modelPath", path)
            .put("models", JSONArray(listModelsSync()))
            .toString()
    }

    fun healthJson(): String = JSONObject()
        .put("status", "ok")
        .put("provider", "litert-lm-android")
        .put("transport", "binder_ipc")
        .toString()

    private fun listModelsSync(): List<String> =
        when {
            File(selectedModelPath).isUsableModelFile() -> listOf(DEFAULT_MODEL)
            else -> emptyList()
        }

    suspend fun chatCompletion(requestJson: String): String = providerMutex.withLock {
        val request = JSONObject(requestJson)
        val model = request.optString("model", DEFAULT_MODEL)
        require(model == DEFAULT_MODEL) { "Unsupported model: $model" }
        require(File(selectedModelPath).isUsableModelFile()) {
            "Model file does not exist: $selectedModelPath"
        }

        val messages = parseMessages(request.getJSONArray("messages"))
        val imageCount = messages.sumOf { message ->
            message.content.count { it is LlmdChatContent.Image }
        }
        require(imageCount <= MAX_IMAGES_PER_REQUEST) {
            "A request may contain at most $MAX_IMAGES_PER_REQUEST image"
        }
        val systemPrompt = messages.firstOrNull { it.role == "system" }?.text ?: ""
        val temperature = when {
            request.isNull("temperature") -> 0.0
            else -> request.optDouble("temperature", 0.0)
        }
        val activeProvider = requireNotNull(provider) { "Android LiteRT bridge is not initialized" }

        activeProvider.generate(
            modelPath = selectedModelPath,
            systemPrompt = systemPrompt,
            messages = messages,
            temperature = temperature,
        )
    }

    internal fun parseMessages(array: JSONArray): List<LlmdChatMessage> =
        (0 until array.length()).map { index ->
            val item = array.getJSONObject(index)
            val role = item.getString("role")
            val content = parseContent(item.get("content"))
            require(role != "system" || content.none { it is LlmdChatContent.Image }) {
                "System messages must not contain images"
            }
            LlmdChatMessage(
                role = role,
                content = content,
            )
        }

    private fun parseContent(value: Any): List<LlmdChatContent> = when (value) {
        is String -> listOf(LlmdChatContent.Text(value))
        is JSONArray -> (0 until value.length()).map { index ->
            val part = value.getJSONObject(index)
            when (val type = part.getString("type")) {
                "text" -> LlmdChatContent.Text(part.getString("text"))
                "image_url" -> parseImageUrl(part.get("image_url"))
                else -> throw IllegalArgumentException("Unsupported message content type: $type")
            }
        }
        else -> throw IllegalArgumentException("Message content must be a string or content array")
    }

    private fun parseImageUrl(value: Any): LlmdChatContent.Image {
        val url = when (value) {
            is String -> value
            is JSONObject -> value.getString("url")
            else -> throw IllegalArgumentException("image_url must be a string or object")
        }
        val match = DATA_IMAGE_PATTERN.matchEntire(url)
            ?: throw IllegalArgumentException("Only Base64 data image URLs are supported on Android")
        val mimeType = match.groupValues[1].lowercase()
        require(mimeType in SUPPORTED_IMAGE_MIME_TYPES) { "Unsupported image type: $mimeType" }
        val bytes = runCatching { Base64.decode(match.groupValues[2], Base64.DEFAULT) }
            .getOrElse { throw IllegalArgumentException("Image data is not valid Base64", it) }
        require(bytes.isNotEmpty()) { "Image data is empty" }
        require(bytes.size <= MAX_IMAGE_BYTES) { "Image exceeds the $MAX_IMAGE_BYTES byte limit" }
        return LlmdChatContent.Image(bytes, mimeType)
    }

    private fun File.isUsableModelFile(): Boolean = exists() && isFile && length() > 0L

    fun defaultModelFile(context: Context): File =
        File(File(context.applicationContext.filesDir, MODEL_DIR), DEFAULT_MODEL_FILE_NAME)

    private const val DEFAULT_MODEL = "gemma-4-E2B-it"
    private const val DEFAULT_MODEL_FILE_NAME = "$DEFAULT_MODEL.litertlm"
    private const val MODEL_DIR = "models"
    private const val MAX_IMAGE_BYTES = 750_000
    private val DATA_IMAGE_PATTERN = Regex("""data:(image/[a-zA-Z0-9.+-]+);base64,([a-zA-Z0-9+/=\r\n]+)""")
    private val SUPPORTED_IMAGE_MIME_TYPES = setOf("image/jpeg", "image/png", "image/webp")
}
