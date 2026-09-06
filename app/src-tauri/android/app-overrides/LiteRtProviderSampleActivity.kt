package com.storytellerf.llmd

import android.os.Bundle
import android.view.Gravity
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import androidx.activity.ComponentActivity
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeout

/** Debug-only end-to-end check for LiteRT-LM initialization, queueing, cancellation, and shutdown. */
class LiteRtProviderSampleActivity : ComponentActivity() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private lateinit var status: TextView
    private lateinit var runButton: Button
    private var provider: AndroidLiteRtProvider? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        status = TextView(this).apply {
            text = "Ready to run LiteRT-LM sample"
            textSize = 16f
        }
        runButton = Button(this).apply {
            text = "Run sample"
            setOnClickListener { runSample() }
        }
        setContentView(
            LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER
                setPadding(48, 48, 48, 48)
                addView(status)
                addView(runButton)
            },
        )
        runSample()
    }

    override fun onDestroy() {
        provider?.let { activeProvider ->
            CoroutineScope(SupervisorJob() + Dispatchers.IO).launch { activeProvider.close() }
        }
        scope.cancel()
        super.onDestroy()
    }

    private fun runSample() {
        runButton.isEnabled = false
        scope.launch {
            val activeProvider = AndroidLiteRtProvider(
                modelPath = LlmdAndroidBridge.defaultModelFile(this@LiteRtProviderSampleActivity).absolutePath,
                cacheDir = cacheDir.absolutePath,
                log = { android.util.Log.i(TAG, it) },
            )
            provider = activeProvider
            try {
                updateStatus("Initializing LiteRT-LM…")
                awaitInitialized(activeProvider)

                updateStatus("Checking serialized generation and cancellation…")
                val first = activeProvider.generate(
                    systemPrompt = "",
                    messages = listOf(LlmdChatMessage("user", listOf(LlmdChatContent.Text("Reply with sample-ok.")))),
                    temperature = 0.0,
                )
                val cancelled = activeProvider.generate(
                    systemPrompt = "",
                    messages = listOf(LlmdChatMessage("user", listOf(LlmdChatContent.Text("Reply with cancelled.")))),
                    temperature = 0.0,
                )
                cancelled.cancel()

                val response = first.await()
                check(response.isNotBlank()) { "The completed generation was empty" }
                try {
                    cancelled.await()
                    error("The queued generation was not cancelled")
                } catch (_: CancellationException) {
                    // Expected: cancellation must remove or short-circuit queued work.
                }

                activeProvider.close()
                check(activeProvider.initializationState == AndroidLiteRtProvider.InitializationState.CLOSED)
                provider = null
                updateStatus("PASS: ${response.take(120)}")
            } catch (error: Exception) {
                runCatching { activeProvider.close() }
                provider = null
                updateStatus("FAIL: ${error.message ?: error::class.java.simpleName}")
            } finally {
                runButton.isEnabled = true
            }
        }
    }

    private suspend fun awaitInitialized(activeProvider: AndroidLiteRtProvider) {
        withTimeout(INITIALIZATION_TIMEOUT_MS) {
            while (activeProvider.initializationState == AndroidLiteRtProvider.InitializationState.UNINITIALIZED ||
                activeProvider.initializationState == AndroidLiteRtProvider.InitializationState.INITIALIZING
            ) {
                delay(100)
            }
        }
        check(activeProvider.initializationState == AndroidLiteRtProvider.InitializationState.INITIALIZED) {
            "LiteRT-LM initialization ended as ${activeProvider.initializationState}"
        }
    }

    private fun updateStatus(value: String) {
        status.text = value
        android.util.Log.i(TAG, value)
    }

    private companion object {
        const val TAG = "LiteRtProviderSample"
        const val INITIALIZATION_TIMEOUT_MS = 5 * 60 * 1000L
    }
}
