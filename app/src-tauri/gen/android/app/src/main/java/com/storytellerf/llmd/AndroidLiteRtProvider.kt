package com.storytellerf.llmd

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
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

class AndroidLiteRtProvider(
    private val cacheDir: String,
    private val log: (String) -> Unit,
) {
    private val engineMutex = Mutex()
    private var loadedModelPath: String? = null
    private var engine: Engine? = null

    suspend fun close() = engineMutex.withLock {
        closeLocked()
    }

    private fun closeLocked() {
        engine?.close()
        engine = null
        loadedModelPath = null
    }

    private fun initializeLocked(modelPath: String, backend: String = "cpu") {
        val file = File(modelPath)
        require(file.exists()) { "Model file does not exist: $modelPath" }
        require(file.length() > 0L) { "Model file is empty: $modelPath" }
        if (loadedModelPath == file.absolutePath && engine?.isInitialized() == true) return

        closeLocked()
        val selectedBackend = when (backend.lowercase()) {
            "gpu" -> Backend.GPU()
            else -> Backend.CPU()
        }
        log("Initializing LiteRT-LM model ${file.absolutePath} with ${selectedBackend.name}")
        engine = Engine(
            EngineConfig(
                modelPath = file.absolutePath,
                backend = selectedBackend,
                visionBackend = null,
                audioBackend = null,
                maxNumTokens = null,
                maxNumImages = null,
                cacheDir = cacheDir,
            ),
        ).also { it.initialize() }
        loadedModelPath = file.absolutePath
        log("LiteRT-LM model initialized")
    }

    suspend fun generate(
        modelPath: String,
        systemPrompt: String,
        messages: List<LlmdChatMessage>,
        temperature: Double,
    ): String = engineMutex.withLock {
        initializeLocked(modelPath)
        val activeEngine = requireNotNull(engine) { "LiteRT-LM engine is not initialized" }
        val lastUserMessage = messages.lastOrNull { it.role == "user" }?.content
            ?: error("No user message to send")
        val initialMessages = messages.dropLast(1).mapNotNull { it.toLiteRtMessage() }
        val result = StringBuilder()

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

        result.toString().trim()
    }

    private fun LlmdChatMessage.toLiteRtMessage(): Message? = when (role) {
        "user" -> Message.user(content)
        "assistant" -> Message.model(Contents.of(content))
        "system" -> null
        else -> null
    }

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
}

fun String.stripChatTemplateMarkers(): String =
    replace(Regex("<\\|turn>\\w*\\n?(?:<turn\\|>\\n?)?"), "")
