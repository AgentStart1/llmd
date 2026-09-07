package com.storytellerf.llmd.sample

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.graphics.Bitmap
import android.graphics.Color
import android.os.Bundle
import android.os.IBinder
import android.view.Gravity
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.FileProvider
import com.storytellerf.llmd.ipc.ILlmdChatCallback
import com.storytellerf.llmd.ipc.ILlmdService
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeout
import org.json.JSONArray
import org.json.JSONObject

class IpcSampleActivity : ComponentActivity() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private lateinit var status: TextView
    private lateinit var runButton: Button
    private var service: ILlmdService? = null
    private var bound = false
    private var targetPackage = BuildConfig.DEFAULT_LLMD_PACKAGE

    private val authorization = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        runIpcChecks()
    }

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName, binder: IBinder) {
            service = ILlmdService.Stub.asInterface(binder)
            runIpcChecks()
        }

        override fun onServiceDisconnected(name: ComponentName) {
            service = null
            bound = false
            updateStatus("FAIL: llmd IPC service disconnected")
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        status = TextView(this).apply { textSize = 16f }
        runButton = Button(this).apply {
            text = "Run IPC sample"
            setOnClickListener { startOrRetry() }
        }
        setContentView(
            LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER
                setPadding(48, 48, 48, 48)
                addView(status)
                addView(targetSelector())
                addView(runButton)
            },
        )
        targetPackage = intent.getStringExtra(EXTRA_LLMD_PACKAGE)
            ?.also { getPreferences(MODE_PRIVATE).edit().putString(EXTRA_LLMD_PACKAGE, it).apply() }
            ?: getPreferences(MODE_PRIVATE).getString(EXTRA_LLMD_PACKAGE, targetPackage).orEmpty()
        startOrRetry()
    }

    override fun onDestroy() {
        if (bound) unbindService(connection)
        scope.cancel()
        super.onDestroy()
    }

    private fun startOrRetry() {
        runButton.isEnabled = false
        if (bound) {
            unbindService(connection)
            bound = false
            service = null
        }
        updateStatus("Binding to $targetPackage…")
        bound = bindService(
            Intent(ACTION_BIND_IPC).setPackage(targetPackage),
            connection,
            Context.BIND_AUTO_CREATE,
        )
        if (!bound) finishRun("FAIL: llmd IPC service is unavailable")
    }

    private fun runIpcChecks() {
        val activeService = service ?: return
        scope.launch {
            try {
                updateStatus("Checking IPC authorization…")
                val firstHealth = call { callback -> activeService.healthAsync(callback) }
                if (firstHealth.errorType() == AUTHORIZATION_REQUIRED) {
                    authorization.launch(
                        Intent(ACTION_AUTHORIZE_CALLER)
                            .setPackage(targetPackage)
                            .putExtra(EXTRA_CALLER_PACKAGE, packageName),
                    )
                    return@launch
                }
                check(firstHealth.errorType() == null) { firstHealth.errorMessage() }
                runAuthorizedChecks(activeService)
            } catch (error: Exception) {
                finishRun("FAIL: ${error.message ?: error::class.java.simpleName}")
            }
        }
    }

    private suspend fun runAuthorizedChecks(activeService: ILlmdService) {
        updateStatus("Waiting for authorized engine startup…")
        withTimeout(ENGINE_START_TIMEOUT_MS) {
            while (true) {
                val health = call { callback -> activeService.healthAsync(callback) }
                check(health.errorType() == null) { health.errorMessage() }
                if (health.optBoolean("engineReady")) break
                check(health.optString("engineState") != "error") {
                    health.optString("engineError", "LiteRT-LM initialization failed")
                }
                delay(500)
            }
        }

        updateStatus("Checking model list…")
        val models = call { callback -> activeService.listModelsAsync(callback) }
        check(models.errorType() == null) { models.errorMessage() }
        check(models.getJSONArray("data").length() == 1) { "Expected exactly one imported model" }

        updateStatus("Checking text chat IPC…")
        val textReply = chat(activeService, textRequest("Reply with sample-ok."))
        check(textReply.isNotBlank()) { "Text chat response was empty" }

        updateStatus("Checking image chat IPC…")
        val imageReply = chat(activeService, imageRequest())
        check(imageReply.isNotBlank()) { "Image chat response was empty" }
        finishRun("PASS: text and image IPC completed")
    }

    private suspend fun chat(activeService: ILlmdService, request: JSONObject): String {
        val response = call { callback -> activeService.chatCompletionAsync(request.toString(), callback) }
        check(response.errorType() == null) { response.errorMessage() }
        return response.getJSONArray("choices")
            .getJSONObject(0)
            .getJSONObject("message")
            .getString("content")
    }

    private fun textRequest(prompt: String): JSONObject = JSONObject()
        .put("model", MODEL)
        .put("messages", JSONArray().put(JSONObject().put("role", "user").put("content", prompt)))

    private fun imageRequest(): JSONObject {
        val imageFile = File(File(cacheDir, "images"), "sample.png").apply { parentFile?.mkdirs() }
        val bitmap = Bitmap.createBitmap(1, 1, Bitmap.Config.ARGB_8888)
        try {
            bitmap.eraseColor(Color.RED)
            imageFile.outputStream().use { stream -> bitmap.compress(Bitmap.CompressFormat.PNG, 100, stream) }
        } finally {
            bitmap.recycle()
        }
        val imageUri = FileProvider.getUriForFile(this, "$packageName.fileprovider", imageFile)
        grantUriPermission(targetPackage, imageUri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        return JSONObject()
            .put("model", MODEL)
            .put(
                "messages",
                JSONArray().put(
                    JSONObject()
                        .put("role", "user")
                        .put(
                            "content",
                            JSONArray()
                                .put(JSONObject().put("type", "text").put("text", "What color is this image?"))
                                .put(
                                    JSONObject()
                                        .put("type", "image_url")
                                        .put("image_url", JSONObject().put("url", imageUri.toString())),
                                ),
                        ),
                ),
            )
    }

    private suspend fun call(invoke: (ILlmdChatCallback) -> Unit): JSONObject =
        suspendCancellableCoroutine { continuation ->
            invoke(
                object : ILlmdChatCallback.Stub() {
                    override fun onComplete(responseJson: String) {
                        continuation.resumeWith(runCatching { JSONObject(responseJson) })
                    }
                },
            )
        }

    private fun JSONObject.errorType(): String? = optJSONObject("error")?.optString("type")

    private fun JSONObject.errorMessage(): String =
        optJSONObject("error")?.optString("message").orEmpty().ifBlank { toString() }

    private fun finishRun(value: String) {
        updateStatus(value)
        runButton.isEnabled = true
    }

    private fun updateStatus(value: String) {
        status.text = value
        android.util.Log.i(TAG, value)
    }

    private fun targetSelector(): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.HORIZONTAL
        listOf(
            "Debug" to "com.storytellerf.llmd.debug",
            "Release" to "com.storytellerf.llmd",
            "Alpha" to "com.storytellerf.llmd.alpha",
            "E2E" to "com.storytellerf.llmd.e2e",
        ).forEach { (label, packageName) ->
            addView(Button(this@IpcSampleActivity).apply {
                text = label
                setOnClickListener {
                    targetPackage = packageName
                    getPreferences(MODE_PRIVATE).edit().putString(EXTRA_LLMD_PACKAGE, packageName).apply()
                    startOrRetry()
                }
            })
        }
    }

    private companion object {
        const val ACTION_AUTHORIZE_CALLER = "com.storytellerf.llmd.action.AUTHORIZE_CALLER"
        const val ACTION_BIND_IPC = "com.storytellerf.llmd.action.BIND_IPC"
        const val AUTHORIZATION_REQUIRED = "authorization_required"
        const val ENGINE_START_TIMEOUT_MS = 5 * 60 * 1000L
        const val EXTRA_CALLER_PACKAGE = "caller_package"
        const val EXTRA_LLMD_PACKAGE = "llmdPackage"
        const val MODEL = "gemma-4-E2B-it"
        const val TAG = "LlmdIpcSample"
    }
}
