package com.storytellerf.llmd

import android.os.SystemClock
import com.google.ai.edge.litertlm.Backend
import com.google.ai.edge.litertlm.Content
import com.google.ai.edge.litertlm.Contents
import com.google.ai.edge.litertlm.Conversation
import com.google.ai.edge.litertlm.ConversationConfig
import com.google.ai.edge.litertlm.Engine
import com.google.ai.edge.litertlm.EngineConfig
import com.google.ai.edge.litertlm.ExperimentalApi
import com.google.ai.edge.litertlm.Message
import com.google.ai.edge.litertlm.SamplerConfig
import java.io.File
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

class AndroidLiteRtProvider(
    private val cacheDir: String,
    private val log: (String) -> Unit,
) {
    private val engineMutex = Mutex()
    private var loadedModelPath: String? = null
    private var loadedBackend: BackendKind? = null
    private var engine: Engine? = null

    suspend fun close() = engineMutex.withLock {
        closeLocked()
    }

    private fun closeLocked() {
        engine?.close()
        engine = null
        loadedModelPath = null
        loadedBackend = null
    }

    private fun initializeLocked(
        modelPath: String,
        backendCandidates: List<BackendKind> = listOf(BackendKind.GPU, BackendKind.CPU),
    ) {
        val file = File(modelPath)
        require(file.exists()) { "Model file does not exist: $modelPath" }
        require(file.length() > 0L) { "Model file is empty: $modelPath" }
        if (loadedModelPath == file.absolutePath && engine?.isInitialized() == true) return

        closeLocked()
        var lastError: Exception? = null
        backendCandidates.distinct().forEach { backendKind ->
            val selectedBackend = backendKind.create()
            val startedAt = SystemClock.elapsedRealtime()
            log("Initializing LiteRT-LM model ${file.absolutePath} with ${selectedBackend.name}")
            val candidateEngine = Engine(
                EngineConfig(
                    modelPath = file.absolutePath,
                    backend = selectedBackend,
                    visionBackend = selectedBackend,
                    audioBackend = null,
                    maxNumTokens = null,
                    maxNumImages = MAX_IMAGES_PER_REQUEST,
                    cacheDir = cacheDir,
                ),
            )
            try {
                candidateEngine.initialize()
                engine = candidateEngine
                loadedModelPath = file.absolutePath
                loadedBackend = backendKind
                log(
                    "LiteRT-LM model initialized with ${selectedBackend.name} in " +
                        "${SystemClock.elapsedRealtime() - startedAt} ms",
                )
                return
            } catch (error: Exception) {
                runCatching { candidateEngine.close() }
                lastError = error
                log(
                    "LiteRT-LM ${selectedBackend.name} initialization failed after " +
                        "${SystemClock.elapsedRealtime() - startedAt} ms: ${error.message}",
                )
            }
        }
        throw IllegalStateException("Unable to initialize LiteRT-LM with GPU or CPU", lastError)
    }

    suspend fun generate(
        modelPath: String,
        systemPrompt: String,
        messages: List<LlmdChatMessage>,
        temperature: Double,
    ): String = engineMutex.withLock {
        initializeLocked(modelPath)
        try {
            generateLocked(systemPrompt, messages, temperature)
        } catch (error: CancellationException) {
            throw error
        } catch (error: Exception) {
            if (loadedBackend != BackendKind.GPU) throw error
            log("LiteRT-LM GPU inference failed: ${error.message}; retrying with CPU")
            closeLocked()
            initializeLocked(modelPath, listOf(BackendKind.CPU))
            generateLocked(systemPrompt, messages, temperature)
        }
    }

    private suspend fun generateLocked(
        systemPrompt: String,
        messages: List<LlmdChatMessage>,
        temperature: Double,
    ): String {
        val activeEngine = requireNotNull(engine) { "LiteRT-LM engine is not initialized" }
        val lastUserMessage = messages.lastOrNull { it.role == "user" }?.toLiteRtContents()
            ?: error("No user message to send")
        val initialMessages = messages.dropLast(1).mapNotNull { it.toLiteRtMessage() }
        val result = StringBuilder()
        val startedAt = SystemClock.elapsedRealtime()

        activeEngine.createConversation(
            ConversationConfig(
                systemInstruction = Contents.of(systemPrompt),
                initialMessages = initialMessages,
                tools = emptyList(),
                samplerConfig = SamplerConfig(
                    topK = 40,
                    topP = 0.95,
                    temperature = temperature,
                    seed = 0,
                ),
            ),
        ).use { conversation ->
            var previous = ""
            conversation.sendMessageAsync(lastUserMessage).collect { message ->
                val rendered = message.textContent().ifBlank { conversation.safeRender(message) }
                val delta = if (rendered.startsWith(previous)) rendered.removePrefix(previous) else rendered
                previous = rendered
                if (delta.isNotEmpty()) result.append(delta)
            }
        }

        return result.toString().trim().also {
            log(
                "LiteRT-LM generation completed with ${loadedBackend?.label} in " +
                    "${SystemClock.elapsedRealtime() - startedAt} ms",
            )
        }
    }

    private fun LlmdChatMessage.toLiteRtMessage(): Message? = when (role) {
        "user" -> Message.user(toLiteRtContents())
        "assistant" -> Message.model(toLiteRtContents())
        "system" -> null
        else -> null
    }

    private fun LlmdChatMessage.toLiteRtContents(): Contents = Contents.of(
        content.map { part ->
            when (part) {
                is LlmdChatContent.Text -> Content.Text(part.value)
                is LlmdChatContent.Image -> Content.ImageBytes(part.bytes)
            }
        },
    )

    private fun Message.textContent(): String =
        contents.contents.joinToString(separator = "") { content ->
            when (content) {
                is Content.Text -> content.text
                else -> content.toString()
            }
        }.stripChatTemplateMarkers()

    @OptIn(ExperimentalApi::class)
    private fun Conversation.safeRender(message: Message): String =
        runCatching { renderMessageIntoString(message) }
            .getOrDefault(message.toString())
            .stripChatTemplateMarkers()

    private enum class BackendKind(val label: String) {
        GPU("GPU"),
        CPU("CPU"),
        ;

        fun create(): Backend = when (this) {
            GPU -> Backend.GPU()
            CPU -> Backend.CPU()
        }
    }
}

fun String.stripChatTemplateMarkers(): String =
    replace(Regex("<\\|turn>\\w*\\n?(?:<turn\\|>\\n?)?"), "")
