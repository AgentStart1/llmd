package com.storytellerf.llmd

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

sealed interface LlmdServiceState {
    data object Off : LlmdServiceState
    data class Starting(val model: String) : LlmdServiceState
    data class Ready(val model: String) : LlmdServiceState
    data class Stopping(val model: String) : LlmdServiceState
    data class Failed(val model: String) : LlmdServiceState
}

/** Application-owned serial coordination; engine work runs on the injected worker. */
class LlmdServiceHost(
    private val scope: CoroutineScope,
    private val coordination: CoroutineDispatcher,
    private val worker: CoroutineDispatcher,
    private val load: suspend (String) -> Unit,
    private val unload: suspend () -> Unit,
) {
    private val mutableState = MutableStateFlow<LlmdServiceState>(LlmdServiceState.Off)
    val state = mutableState.asStateFlow()
    private var loading: Job? = null

    fun enable(model: String) = scope.launch(coordination) {
        require(model.isNotBlank()) { "Choose a model" }
        if (state.value != LlmdServiceState.Off && state.value !is LlmdServiceState.Failed) return@launch
        mutableState.value = LlmdServiceState.Starting(model)
        loading = scope.launch(coordination) {
            try {
                withContext(worker) { load(model) }
                mutableState.value = LlmdServiceState.Ready(model)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Throwable) {
                // JNI initialization can report LinkageError or OutOfMemoryError.
                withContext(worker) { unload() }
                mutableState.value = LlmdServiceState.Failed(model)
            }
        }
    }

    fun disable() = scope.launch(coordination) {
        val model = when (val current = state.value) {
            is LlmdServiceState.Starting -> current.model
            is LlmdServiceState.Ready -> current.model
            is LlmdServiceState.Failed -> current.model
            LlmdServiceState.Off, is LlmdServiceState.Stopping -> return@launch
        }
        mutableState.value = LlmdServiceState.Stopping(model)
        loading?.cancel()
        withContext(worker) { unload() }
        loading?.join()
        mutableState.value = LlmdServiceState.Off
    }
}
