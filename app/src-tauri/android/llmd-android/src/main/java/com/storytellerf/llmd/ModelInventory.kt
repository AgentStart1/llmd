package com.storytellerf.llmd

import java.io.File

/** IDs are imported file stems; missing IDs never resolve to another file. */
class ModelInventory(private val directory: File) {
    fun files(): List<File> = directory.listFiles().orEmpty()
        .filter { it.isFile && it.length() > 0L && it.extension.equals("litertlm", ignoreCase = true) }
        .sortedBy { it.name }

    fun resolve(id: String): File = files().singleOrNull { it.nameWithoutExtension == id }
        ?: throw IllegalArgumentException("Model not found: $id")

    fun destination(name: String): File {
        require(name == File(name).name && !name.contains('\\') && name.endsWith(".litertlm", ignoreCase = true)) {
            "Select a named .litertlm file"
        }
        require(File(name).nameWithoutExtension.isNotBlank() && File(name).nameWithoutExtension !in setOf(".", "..")) { "Model file needs a name" }
        directory.mkdirs()
        return File(directory, name)
    }
}
