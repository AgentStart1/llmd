package com.storytellerf.llmd

import android.content.Context
import android.graphics.BitmapFactory
import android.net.Uri
import java.io.ByteArrayOutputStream
import java.io.File
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject

object LlmdAndroidBridge {
    private val providerMutex = Mutex()
    private var appContext: Context? = null
    @Volatile private var provider: AndroidLiteRtProvider? = null
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
        provider?.initialize(selectedModelPath)
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
        .put("engineReady", provider?.isReady() == true)
        .toString()

    private fun listModelsSync(): List<String> =
        when {
            File(selectedModelPath).isUsableModelFile() -> listOf(DEFAULT_MODEL)
            else -> emptyList()
        }

    suspend fun chatCompletion(requestJson: String, callingUid: Int = android.os.Process.myUid()): String {
        val request = JSONObject(requestJson)
        val model = request.optString("model", DEFAULT_MODEL)
        require(model == DEFAULT_MODEL) { "Unsupported model: $model" }
        require(File(selectedModelPath).isUsableModelFile()) {
            "Model file does not exist: $selectedModelPath"
        }

        val messages = parseMessages(request.getJSONArray("messages"), callingUid)
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

        return suspendCancellableCoroutine { continuation ->
            val job = activeProvider.generate(
                systemPrompt = systemPrompt,
                messages = messages,
                temperature = temperature,
                onComplete = { continuation.resumeWith(it) },
            )
            continuation.invokeOnCancellation { job.cancel() }
            job.invokeOnCompletion { error ->
                if (error != null) continuation.cancel(error)
            }
        }
    }

    internal fun parseMessages(array: JSONArray, callingUid: Int = android.os.Process.myUid()): List<LlmdChatMessage> =
        (0 until array.length()).map { index ->
            val item = array.getJSONObject(index)
            val role = item.getString("role")
            val content = parseContent(item.get("content"), callingUid)
            require(role != "system" || content.none { it is LlmdChatContent.Image }) {
                "System messages must not contain images"
            }
            LlmdChatMessage(
                role = role,
                content = content,
            )
        }

    private fun parseContent(value: Any, callingUid: Int): List<LlmdChatContent> = when (value) {
        is String -> listOf(LlmdChatContent.Text(value))
        is JSONArray -> (0 until value.length()).map { index ->
            val part = value.getJSONObject(index)
            when (val type = part.getString("type")) {
                "text" -> LlmdChatContent.Text(part.getString("text"))
                "image_url" -> parseImageUrl(part.get("image_url"), callingUid)
                else -> throw IllegalArgumentException("Unsupported message content type: $type")
            }
        }
        else -> throw IllegalArgumentException("Message content must be a string or content array")
    }

    private fun parseImageUrl(value: Any, callingUid: Int): LlmdChatContent.Image {
        val url = when (value) {
            is String -> value
            is JSONObject -> value.getString("url")
            else -> throw IllegalArgumentException("image_url must be a string or object")
        }
        val uri = Uri.parse(url)
        require(uri.scheme == "content" && !uri.authority.isNullOrBlank()) {
            "Android images require a FileProvider content:// URI with read permission"
        }
        val context = requireNotNull(appContext) { "Android bridge is not configured" }
        // Only accept content owned by the Binder caller. Opening the stream separately proves
        // that the caller also granted llmd temporary read access.
        val ownerUid = context.packageManager.resolveContentProvider(uri.authority!!, 0)?.applicationInfo?.uid
        require(callingUid == android.os.Process.myUid() || callingUid == ownerUid) {
            "Image URI is not owned by the Binder caller"
        }
        val mimeType = context.contentResolver.getType(uri)?.lowercase()
        require(mimeType in SUPPORTED_IMAGE_MIME_TYPES) { "Unsupported image type: $mimeType" }
        val bytes = requireNotNull(context.contentResolver.openInputStream(uri)) {
            "Unable to open image URI; grant llmd read permission first"
        }.use { input ->
            val output = ByteArrayOutputStream()
            val buffer = ByteArray(8192)
            while (true) {
                val count = input.read(buffer, 0, minOf(buffer.size, MAX_IMAGE_BYTES + 1 - output.size()))
                if (count < 0) break
                output.write(buffer, 0, count)
                require(output.size() <= MAX_IMAGE_BYTES) { "Image exceeds the $MAX_IMAGE_BYTES byte limit" }
            }
            output.toByteArray()
        }
        require(bytes.isNotEmpty()) { "Image data is empty" }
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        require(bounds.outWidth > 0 && bounds.outHeight > 0 && bounds.outMimeType == mimeType) {
            "Invalid image data or MIME type mismatch"
        }
        return LlmdChatContent.Image(bytes, requireNotNull(mimeType))
    }

    private fun File.isUsableModelFile(): Boolean = exists() && isFile && length() > 0L

    fun defaultModelFile(context: Context): File =
        File(File(context.applicationContext.filesDir, MODEL_DIR), DEFAULT_MODEL_FILE_NAME)

    private const val DEFAULT_MODEL = "gemma-4-E2B-it"
    private const val DEFAULT_MODEL_FILE_NAME = "$DEFAULT_MODEL.litertlm"
    private const val MODEL_DIR = "models"
    private const val MAX_IMAGE_BYTES = 750_000
    private val SUPPORTED_IMAGE_MIME_TYPES = setOf("image/jpeg", "image/png", "image/webp")
}
