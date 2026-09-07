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
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Deferred
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
    private val modelPath: String,
    private val cacheDir: String,
    private val log: (String) -> Unit,
) {
    enum class InitializationState {
        UNINITIALIZED,
        INITIALIZING,
        INITIALIZED,
        ERROR,
        CLOSED,
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val commands = Channel<Pair<CompletableDeferred<Unit>, Job>>(MAX_PENDING_REQUESTS)
    private val requestSlots = Channel<Unit>(MAX_PENDING_REQUESTS).also { slots ->
        repeat(MAX_PENDING_REQUESTS) { slots.trySend(Unit) }
    }

    @Volatile
    var initializationState = InitializationState.UNINITIALIZED
        private set

    @Volatile
    private var closed = false
    private var loadedModelPath: String? = null
    private var engine: Engine? = null
    private val initializationJob: Job

    init {
        initializationJob = scope.launch {
            if (!initializeEngine()) return@launch
            if (closed) return@launch

            initializationState = InitializationState.INITIALIZED
            for ((start, command) in commands) {
                start.complete(Unit)
                command.join()
            }
        }
    }

    fun isReady(): Boolean = initializationState == InitializationState.INITIALIZED && !closed

    /**
     * Reserves capacity before a caller reads a request's image payload. The reservation must be
     * passed to [generate] or released when request parsing fails.
     */
    fun tryReserveRequest(): RequestReservation? {
        if (!isReady() || requestSlots.tryReceive().isFailure) return null
        return RequestReservation(this)
    }

    private fun <T> enqueue(block: suspend () -> T): Deferred<T> {
        val start = CompletableDeferred<Unit>()
        return scope.async { start.await(); block() }.also { task ->
            if (closed || commands.trySend(start to task).isFailure) {
                task.cancel()
                error("LiteRT-LM provider is closed")
            }
        }
    }

    private fun initializeEngine(): Boolean {
        initializationState = InitializationState.INITIALIZING
        return try {
            initializeEngineLocked()
            true
        } catch (error: CancellationException) {
            throw error
        } catch (error: Exception) {
            if (!closed) {
                initializationState = InitializationState.ERROR
                log("LiteRT-LM initialization failed: ${error.message ?: error::class.java.simpleName}")
            }
            commands.close(error)
            false
        }
    }

    suspend fun close() {
        if (closed) return
        closed = true
        try {
            commands.close()
            scope.cancel()
            withContext(NonCancellable) { initializationJob.join() }
        } finally {
            withContext(NonCancellable) { engine?.close() }
            engine = null
            loadedModelPath = null
            initializationState = InitializationState.CLOSED
        }
    }

    private fun initializeEngineLocked() {
        val file = File(modelPath)
        require(file.isFile && file.length() > 0L) { "Model file is missing or empty: $modelPath" }
        if (loadedModelPath == file.absolutePath && isReady()) return
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
            if (closed) {
                candidate.close()
                return
            }
            engine = candidate
            loadedModelPath = file.absolutePath
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
    ): Deferred<String> {
        val reservation = requireNotNull(tryReserveRequest()) {
            "LiteRT-LM request queue is full or the engine is not ready"
        }
        return generate(systemPrompt, messages, temperature, reservation)
    }

    fun generate(
        systemPrompt: String,
        messages: List<LlmdChatMessage>,
        temperature: Double,
        reservation: RequestReservation,
    ): Deferred<String> {
        check(reservation.owner === this) { "LiteRT-LM request reservation belongs to another provider" }
        check(isReady()) { "LiteRT-LM engine is not ready" }
        return try {
            enqueue {
                check(isReady()) { "LiteRT-LM engine is not ready" }
                generateLocked(systemPrompt, messages, temperature)
            }.also { task -> task.invokeOnCompletion { reservation.release() } }
        } catch (error: Throwable) {
            reservation.release()
            throw error
        }
    }

    class RequestReservation internal constructor(
        internal val owner: AndroidLiteRtProvider,
    ) {
        private val released = AtomicBoolean(false)

        fun release() {
            if (released.compareAndSet(false, true) && !owner.closed) {
                owner.requestSlots.trySend(Unit)
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

    private companion object {
        // One request may run while one additional request waits for the serialized GPU engine.
        const val MAX_PENDING_REQUESTS = 2
    }

}

fun String.stripChatTemplateMarkers(): String =
    replace(Regex("<\\|turn>\\w*\\n?(?:<turn\\|>\\n?)?"), "")
