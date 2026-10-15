package com.storytellerf.llmd

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob

object LlmdRuntime {
    val coordination = Dispatchers.Default.limitedParallelism(1)
    private val scope = CoroutineScope(SupervisorJob() + coordination)
    val host = LlmdServiceHost(scope, coordination, Dispatchers.IO,
        load = { model -> LlmdAndroidBridge.startModel(model) },
        unload = { LlmdAndroidBridge.close() })
}
