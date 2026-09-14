package com.storytellerf.llmd

import android.content.Context
import android.graphics.BitmapFactory
import android.net.Uri
import java.io.ByteArrayOutputStream
import java.io.File
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
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
            provider = AndroidLiteRtProvider(
                modelPath = selectedModelPath,
                cacheDir = context.applicationContext.cacheDir.absolutePath,
            ) { android.util.Log.i("llmd", it) }
        }
    }

    suspend fun close() = providerMutex.withLock {
        provider?.close()
        provider = null
    }

    /** Replaces the model without allowing a concurrent IPC request to start a stale provider. */
    suspend fun replaceDefaultModel(context: Context, source: Uri) = providerMutex.withLock {
        configure(context)
        provider?.close()
        provider = null
        try {
            copyDefaultModel(context, source)
            provider = createProvider(context)
        } catch (error: Throwable) {
            // The existing destination is preserved until the replacement is ready. Restore its
            // engine when importing the new file fails.
            if (File(selectedModelPath).isUsableModelFile()) {
                provider = createProvider(context)
            }
            throw error
        }
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
        .put(
            "engineState",
            provider?.initializationState?.name?.lowercase() ?: "uninitialized",
        )
        .put("engineError", provider?.initializationError)
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
        val (activeProvider, reservation) = providerMutex.withLock {
            require(File(selectedModelPath).isUsableModelFile()) {
                "Model file does not exist: $selectedModelPath"
            }
            val currentProvider = requireNotNull(provider) { "Android LiteRT bridge is not initialized" }
            currentProvider.tryReserveRequest()
                ?.let { currentProvider to it }
                ?: error("LiteRT-LM request queue is full or the engine is not ready")
        }

        return try {
            val messages = parseMessages(request.getJSONArray("messages"), callingUid)
            val systemPrompt = messages.filter { it.role == "system" }.map { it.text }
            val temperature = when {
                request.isNull("temperature") -> 0.0
                else -> request.optDouble("temperature", 0.0)
            }
            val task = activeProvider.generate(
                systemPrompt = systemPrompt.joinToString("\n\n"),
                messages = messages,
                temperature = temperature,
                reservation = reservation,
                responseFormat = parseResponseFormat(request),
            )
            task.await()
        } catch (error: CancellationException) {
            throw error
        } finally {
            reservation.release()
        }
    }

    private fun createProvider(context: Context): AndroidLiteRtProvider = AndroidLiteRtProvider(
        modelPath = selectedModelPath,
        cacheDir = context.applicationContext.cacheDir.absolutePath,
    ) { android.util.Log.i("llmd", it) }

    private suspend fun copyDefaultModel(context: Context, source: Uri): File = withContext(Dispatchers.IO) {
        val destination = defaultModelFile(context)
        val destinationDir = requireNotNull(destination.parentFile) { "Model directory is unavailable" }
        val temp = File(destinationDir, "${destination.name}.tmp")
        destinationDir.mkdirs()
        try {
            context.contentResolver.openInputStream(source).use { input ->
                requireNotNull(input) { "Unable to open selected model file" }
                temp.outputStream().use { output -> input.copyTo(output) }
            }
            require(temp.length() > 0L) { "Selected model file is empty" }
            if (destination.exists()) destination.delete()
            require(temp.renameTo(destination)) { "Unable to save imported model" }
            destination
        } finally {
            if (temp.exists()) temp.delete()
        }
    }

    internal fun parseMessages(array: JSONArray, callingUid: Int = android.os.Process.myUid()): List<LlmdChatMessage> {
        var imageCount = 0
        return (0 until array.length()).map { index ->
            val item = array.getJSONObject(index)
            val role = item.getString("role")
            val content = parseContent(item.get("content"), callingUid) {
                require(++imageCount <= MAX_IMAGES_PER_REQUEST) {
                    "A request may contain at most $MAX_IMAGES_PER_REQUEST image"
                }
            }
            require(role != "system" || content.none { it is LlmdChatContent.Image }) {
                "System messages must not contain images"
            }
            LlmdChatMessage(
                role = role,
                content = content,
            )
        }
    }

    private fun parseContent(
        value: Any,
        callingUid: Int,
        beforeImageRead: () -> Unit,
    ): List<LlmdChatContent> = when (value) {
        is String -> listOf(LlmdChatContent.Text(value))
        is JSONArray -> (0 until value.length()).map { index ->
            val part = value.getJSONObject(index)
            when (val type = part.getString("type")) {
                "text" -> LlmdChatContent.Text(part.getString("text"))
                "image_url" -> {
                    beforeImageRead()
                    parseImageUrl(part.get("image_url"), callingUid)
                }
                else -> throw IllegalArgumentException("Unsupported message content type: $type")
            }
        }
        else -> throw IllegalArgumentException("Message content must be a string or content array")
    }

    private fun parseResponseFormat(request: JSONObject): com.google.ai.edge.litertlm.ResponseFormat? {
        if (!request.has("response_format") || request.isNull("response_format")) return null
        val responseFormat = request.getJSONObject("response_format")
        return when (responseFormat.getString("type")) {
            "text" -> null
            "json_object" -> {
                com.google.ai.edge.litertlm.ResponseFormat.json("{\"type\":\"object\"}")
            }
            "json_schema" -> {
                val jsonSchema = responseFormat.getJSONObject("json_schema")
                jsonSchema.getString("name")
                val schema = jsonSchema.get("schema")
                com.google.ai.edge.litertlm.ResponseFormat.json(schema.toString())
            }
            else -> throw IllegalArgumentException("Unsupported response_format type")
        }
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
        val pixelCount = bounds.outWidth.toLong() * bounds.outHeight.toLong()
        require(
            bounds.outWidth <= MAX_IMAGE_DIMENSION &&
                bounds.outHeight <= MAX_IMAGE_DIMENSION &&
                pixelCount <= MAX_IMAGE_PIXELS,
        ) { "Image dimensions exceed the ${MAX_IMAGE_PIXELS}-pixel limit" }
        return LlmdChatContent.Image(bytes, requireNotNull(mimeType))
    }

    private fun File.isUsableModelFile(): Boolean = exists() && isFile && length() > 0L

    fun defaultModelFile(context: Context): File =
        File(File(context.applicationContext.filesDir, MODEL_DIR), DEFAULT_MODEL_FILE_NAME)

    private const val DEFAULT_MODEL = "gemma-4-E2B-it"
    private const val DEFAULT_MODEL_FILE_NAME = "$DEFAULT_MODEL.litertlm"
    private const val MODEL_DIR = "models"
    private const val MAX_IMAGE_BYTES = 750_000
    private const val MAX_IMAGE_DIMENSION = 4_096
    private const val MAX_IMAGE_PIXELS = 4_000_000L
    private val SUPPORTED_IMAGE_MIME_TYPES = setOf("image/jpeg", "image/png", "image/webp")
}
