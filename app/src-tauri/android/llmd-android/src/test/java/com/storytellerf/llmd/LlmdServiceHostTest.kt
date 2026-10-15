package com.storytellerf.llmd

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.*
import org.junit.Test
import java.nio.file.Files

@OptIn(ExperimentalCoroutinesApi::class)
class LlmdServiceHostTest {
    @Test fun readingStateDoesNotLoad_andExplicitStartWaitsForReadiness() = runTest {
        val dispatcher = StandardTestDispatcher(testScheduler)
        val ready = CompletableDeferred<Unit>()
        val calls = mutableListOf<String>()
        val host = LlmdServiceHost(backgroundScope, dispatcher, dispatcher, { calls.add(it); ready.await() }, {})
        assertEquals(LlmdServiceState.Off, host.state.value)
        assertTrue(calls.isEmpty())
        host.enable("chosen-model"); runCurrent()
        assertEquals(LlmdServiceState.Starting("chosen-model"), host.state.value)
        host.enable("other-model"); runCurrent()
        assertEquals(listOf("chosen-model"), calls)
        ready.complete(Unit); runCurrent()
        assertEquals(LlmdServiceState.Ready("chosen-model"), host.state.value)
    }

    @Test fun disablingDuringLoadCannotPublishLateReadiness_andCanRestartExplicitly() = runTest {
        val dispatcher = StandardTestDispatcher(testScheduler)
        val ready = CompletableDeferred<Unit>()
        var unloaded = 0
        val host = LlmdServiceHost(backgroundScope, dispatcher, dispatcher, { ready.await() }, { unloaded++ })
        host.enable("model-a"); runCurrent()
        host.disable(); runCurrent()
        ready.complete(Unit); runCurrent()
        assertEquals(LlmdServiceState.Off, host.state.value)
        assertEquals(1, unloaded)
        host.enable("model-b"); runCurrent()
        assertEquals(LlmdServiceState.Ready("model-b"), host.state.value)
    }

    @Test fun stoppingWaitsForResourceRelease_andFailureDoesNotFallback() = runTest {
        val dispatcher = StandardTestDispatcher(testScheduler)
        val released = CompletableDeferred<Unit>()
        val calls = mutableListOf<String>()
        val host = LlmdServiceHost(backgroundScope, dispatcher, dispatcher, { calls.add(it) }, { released.await() })
        host.enable("model-a"); runCurrent()
        host.disable(); runCurrent()
        assertEquals(LlmdServiceState.Stopping("model-a"), host.state.value)
        host.enable("model-b"); runCurrent()
        assertEquals(listOf("model-a"), calls)
        released.complete(Unit); runCurrent()
        assertEquals(LlmdServiceState.Off, host.state.value)
        for (failure in listOf(IllegalStateException("load failure"), OutOfMemoryError("native memory failure"))) {
            val broken = LlmdServiceHost(backgroundScope, dispatcher, dispatcher, { throw failure }, {})
            broken.enable("missing-model"); runCurrent()
            assertEquals(LlmdServiceState.Failed("missing-model"), broken.state.value)
        }
    }

    @Test fun inventoryReflectsFiles_andNeverResolvesMissingModelsOrPaths() {
        val directory = Files.createTempDirectory("model-inventory").toFile()
        try {
            directory.resolve("model-a.litertlm").writeText("model")
            directory.resolve("model-b.litertlm").writeText("model")
            directory.resolve("incomplete.tmp").writeText("partial")
            directory.resolve("empty.litertlm").createNewFile()
            val inventory = ModelInventory(directory)
            assertEquals(listOf("model-a", "model-b"), inventory.files().map { it.nameWithoutExtension })
            assertThrows(IllegalArgumentException::class.java) { inventory.resolve("missing") }
            assertThrows(IllegalArgumentException::class.java) { inventory.destination("../outside.litertlm") }
            directory.resolve("model-a.litertlm").delete()
            assertEquals(listOf("model-b"), inventory.files().map { it.nameWithoutExtension })
        } finally { directory.listFiles().orEmpty().forEach { it.delete() }; directory.delete() }
    }
}
