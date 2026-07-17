package com.storytellerf.llmd

import android.app.Service
import android.content.Intent
import android.os.Binder
import android.os.IBinder
import com.storytellerf.llmd.ipc.ILlmdChatCallback
import com.storytellerf.llmd.ipc.ILlmdService
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

class LlmdIpcService : Service() {
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    private val binder = object : ILlmdService.Stub() {
        override fun healthAsync(callback: ILlmdChatCallback) {
            respondAuthorized(callback) { buildHealthResponse() }
        }

        override fun listModelsAsync(callback: ILlmdChatCallback) {
            respondAuthorized(callback) { buildListModelsResponse() }
        }

        override fun chatCompletionAsync(requestJson: String, callback: ILlmdChatCallback) {
            respondAuthorized(callback) { buildChatCompletionResponse(requestJson) }
        }
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onDestroy() {
        serviceScope.cancel()
        super.onDestroy()
    }

    private fun respondAuthorized(callback: ILlmdChatCallback, buildResponse: suspend () -> String) {
        val callingUid = Binder.getCallingUid()
        serviceScope.launch {
            val response = if (LlmdIpcAuthorization.isAuthorized(this@LlmdIpcService, callingUid)) {
                buildResponse()
            } else {
                authorizationRequiredResponse()
            }
            runCatching { callback.onComplete(response) }
        }
    }

    private suspend fun buildHealthResponse(): String =
        withContext(Dispatchers.Default) {
            JSONObject()
                .put("status", "ok")
                .put("provider", PROVIDER)
                .toString()
        }

    private suspend fun buildListModelsResponse(): String =
        withContext(Dispatchers.Default) {
            LlmdAndroidBridge.configure(this@LlmdIpcService)
            val models = LlmdAndroidBridge.listModels()
            JSONObject()
                .put("object", "list")
                .put(
                    "data",
                    JSONArray(
                        models.map { model ->
                            JSONObject()
                                .put("id", model)
                                .put("object", "model")
                                .put("created", 0)
                                .put("owned_by", PROVIDER)
                        },
                    ),
                )
                .toString()
        }

    private suspend fun buildChatCompletionResponse(requestJson: String): String =
        withContext(Dispatchers.Default) {
            runCatching {
                LlmdAndroidBridge.initialize(this@LlmdIpcService)
                val request = JSONObject(requestJson)
                val model = request.optString("model", DEFAULT_MODEL)
                val content = LlmdAndroidBridge.chatCompletion(requestJson)
                JSONObject()
                    .put("id", "chatcmpl-android-ipc")
                    .put("object", "chat.completion")
                    .put("created", System.currentTimeMillis() / 1000L)
                    .put("model", model)
                    .put(
                        "choices",
                        JSONArray()
                            .put(
                                JSONObject()
                                    .put("index", 0)
                                    .put(
                                        "message",
                                        JSONObject()
                                            .put("role", "assistant")
                                            .put("content", content),
                                    )
                                    .put("finish_reason", "stop"),
                            ),
                    )
                    .put(
                        "usage",
                        JSONObject()
                            .put("prompt_tokens", 0)
                            .put("completion_tokens", 0)
                            .put("total_tokens", 0),
                    )
                    .toString()
            }.getOrElse { error ->
                errorResponse(error)
            }
        }

    private fun errorResponse(error: Throwable): String =
        JSONObject()
            .put(
                "error",
                JSONObject()
                    .put("message", error.message ?: error::class.java.simpleName)
                    .put("type", "llmd_error"),
            )
            .toString()

    private fun authorizationRequiredResponse(): String =
        JSONObject()
            .put(
                "error",
                JSONObject()
                    .put("message", "Caller is not authorized to use llmd IPC")
                    .put("type", "authorization_required"),
            )
            .toString()

    private companion object {
        const val DEFAULT_MODEL = "gemma-4-E2B-it"
        const val PROVIDER = "litert-lm-android"
    }
}
