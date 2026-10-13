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
    @Volatile private var loadedModelId: String? = null

    fun configure(context: Context) { appContext = context.applicationContext }

    private fun inventory() = ModelInventory(File(requireNotNull(appContext).filesDir, "models"))

    suspend fun startModel(model: String) {
        val engine = providerMutex.withLock {
            require(provider == null) { "Stop the current service before starting a model" }
            val file = inventory().resolve(model)
            AndroidLiteRtProvider(file.absolutePath, requireNotNull(appContext).cacheDir.absolutePath) {
                android.util.Log.i("llmd", it)
            }.also { provider = it; loadedModelId = model }
        }
        engine.awaitReady()
    }

    suspend fun close() = providerMutex.withLock {
        try { provider?.close() } finally { provider = null; loadedModelId = null }
    }

    suspend fun importModel(context: Context, source: Uri, name: String) = providerMutex.withLock {
        requireStopped()
        configure(context)
        withContext(Dispatchers.IO) {
            val destination = inventory().destination(name)
            val temp = File.createTempFile("import-", ".tmp", destination.parentFile)
            try {
                requireNotNull(context.contentResolver.openInputStream(source)).use { input ->
                    temp.outputStream().use { output -> input.copyTo(output) }
                }
                require(temp.length() > 0L) { "Selected model file is empty" }
                require(temp.renameTo(destination)) { "Unable to save imported model" }
            } finally { temp.delete() }
        }
    }

    suspend fun listModels(): List<String> = withContext(Dispatchers.IO) { inventory().files().map { it.nameWithoutExtension } }

    suspend fun deleteModel(model: String) = providerMutex.withLock {
        requireStopped()
        require(inventory().resolve(model).delete()) { "Unable to delete model" }
    }

    private fun requireStopped() {
        require(LlmdRuntime.host.state.value == LlmdServiceState.Off || LlmdRuntime.host.state.value is LlmdServiceState.Failed) {
            "Switch off the service before changing model files"
        }
        require(provider == null) { "Model is still unloading" }
    }

    fun modelStateJson(): String {
        val files = inventory().files()
        return JSONObject().put("models", JSONArray(files.map { it.nameWithoutExtension }))
            .put("modelPaths", JSONObject(files.associate { it.nameWithoutExtension to it.absolutePath }))
            .toString()
    }

    fun healthJson(): String {
        val state = LlmdRuntime.host.state.value
        val name = when (state) {
            LlmdServiceState.Off -> "off"
            is LlmdServiceState.Starting -> "starting"
            is LlmdServiceState.Ready -> "ready"
            is LlmdServiceState.Stopping -> "stopping"
            is LlmdServiceState.Failed -> "error"
        }
        return JSONObject().put("status", "ok").put("provider", "litert-lm-android").put("transport", "binder_ipc")
            .put("selectedModel", when (state) {
                LlmdServiceState.Off -> JSONObject.NULL
                is LlmdServiceState.Starting -> state.model
                is LlmdServiceState.Ready -> state.model
                is LlmdServiceState.Stopping -> state.model
                is LlmdServiceState.Failed -> state.model
            })
            .put("serviceState", name).put("serviceEnabled", state is LlmdServiceState.Starting || state is LlmdServiceState.Ready)
            .put("activeModel", if (state is LlmdServiceState.Ready) loadedModelId else JSONObject.NULL)
            .put("engineReady", state is LlmdServiceState.Ready && provider?.isReady() == true)
            .put("engineState", if (state is LlmdServiceState.Failed) "error" else provider?.initializationState?.name?.lowercase() ?: "uninitialized")
            .put("engineError", if (state is LlmdServiceState.Failed) "Model initialization failed; check the file and available memory" else JSONObject.NULL)
            .toString()
    }

    suspend fun chatCompletion(requestJson: String, callingUid: Int = android.os.Process.myUid()): String {
        val request = JSONObject(requestJson)
        val model = request.getString("model")
        require(model.isNotBlank()) { "Specify a model" }
        val (activeProvider, reservation) = providerMutex.withLock {
            inventory().resolve(model)
            require(LlmdRuntime.host.state.value is LlmdServiceState.Ready) { "Switch on the LLMD service and wait until Ready" }
            require(model == loadedModelId) { "Requested model is not the running model" }
            val currentProvider = requireNotNull(provider) { "Model is not loaded" }
            currentProvider.tryReserveRequest()?.let { currentProvider to it }
                ?: error("LiteRT-LM request queue is full or the engine is not ready")
        }
        return try {
            val messages = parseMessages(request.getJSONArray("messages"), callingUid)
            val systemPrompt = messages.filter { it.role == "system" }.joinToString("\n\n") { it.text }
            val generation = activeProvider.generate(systemPrompt, messages, request.optDouble("temperature", 0.0), reservation,
                parseResponseFormat(request))
            try { generation.await() }
            catch (cancelled: CancellationException) { generation.cancel(); throw cancelled }
        }
        finally { reservation.release() }
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

    private const val MAX_IMAGE_BYTES = 750_000
    private const val MAX_IMAGE_DIMENSION = 4_096
    private const val MAX_IMAGE_PIXELS = 4_000_000L
    private val SUPPORTED_IMAGE_MIME_TYPES = setOf("image/jpeg", "image/png", "image/webp")
}
