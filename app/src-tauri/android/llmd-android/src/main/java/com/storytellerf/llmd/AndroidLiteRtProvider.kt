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
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class AndroidLiteRtProvider(
    private val cacheDir: String,
    private val log: (String) -> Unit,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val commands = Channel<Pair<CompletableDeferred<Unit>, Job>>(Channel.UNLIMITED)

    @Volatile
    private var ready = false

    @Volatile
    private var closed = false
    private var loadedModelPath: String? = null
    private var engine: Engine? = null

    init {
        scope.launch {
            for ((start, command) in commands) {
                start.complete(Unit)
                command.join()
            }
        }
    }

    fun isReady(): Boolean = ready && !closed

    private fun <T> enqueue(block: suspend () -> T): kotlinx.coroutines.Deferred<T> {
        val start = CompletableDeferred<Unit>()
        return scope.async { start.await(); block() }.also { task ->
            if (closed || commands.trySend(start to task).isFailure) {
                task.cancel()
                error("LiteRT-LM provider is closed")
            }
        }
    }

    suspend fun initialize(modelPath: String) {
        enqueue { initializeEngine(modelPath) }.await()
    }

    suspend fun close() {
        if (closed) return
        ready = false
        val closing = enqueue {
            try {
                engine?.close()
            } finally {
                engine = null
                loadedModelPath = null
                ready = false
            }
        }
        closed = true
        try {
            withContext(NonCancellable) { closing.await() }
        } finally {
            commands.close()
            scope.cancel()
        }
    }

    private fun initializeEngine(modelPath: String) {
        val file = File(modelPath)
        require(file.isFile && file.length() > 0L) { "Model file is missing or empty: $modelPath" }
        if (loadedModelPath == file.absolutePath && isReady()) return
        ready = false
        engine?.close()
        engine = null
        loadedModelPath = null
        val startedAt = SystemClock.elapsedRealtime()
        val candidate = Engine(
            EngineConfig(
                modelPath = file.absolutePath,
                backend = Backend.GPU(),
                visionBackend = Backend.GPU(),
                audioBackend = null,
                maxNumTokens = null,
                maxNumImages = MAX_IMAGES_PER_REQUEST,
                cacheDir = cacheDir,
            ),
        )
        try {
            candidate.initialize()
            engine = candidate
            loadedModelPath = file.absolutePath
            ready = !closed
            log("LiteRT-LM GPU initialized in ${SystemClock.elapsedRealtime() - startedAt} ms")
        } catch (error: Exception) {
            runCatching { candidate.close() }
            throw error
        }
    }

    fun generate(
        systemPrompt: String,
        messages: List<LlmdChatMessage>,
        temperature: Double,
        onComplete: (Result<String>) -> Unit,
    ): Job {
        check(isReady()) { "LiteRT-LM engine is not ready" }
        return enqueue {
            check(isReady()) { "LiteRT-LM engine is not ready" }
            try {
                onComplete(Result.success(generateLocked(systemPrompt, messages, temperature)))
            } catch (error: CancellationException) {
                throw error
            } catch (error: Exception) {
                onComplete(Result.failure(error))
            }
        }
    }

    private suspend fun generateLocked(
        systemPrompt: String,
        messages: List<LlmdChatMessage>,
        temperature: Double,
    ): String {
        val activeEngine = requireNotNull(engine) { "LiteRT-LM engine is not initialized" }
        val lastUserIndex = messages.indexOfLast { it.role == "user" }
        require(lastUserIndex >= 0) { "No user message to send" }
        val lastUserMessage = messages[lastUserIndex].toLiteRtContents()
        val initialMessages = messages.take(lastUserIndex).mapNotNull { it.toLiteRtMessage() }
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
                "LiteRT-LM generation completed with GPU in " +
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

}

fun String.stripChatTemplateMarkers(): String =
    replace(Regex("<\\|turn>\\w*\\n?(?:<turn\\|>\\n?)?"), "")
