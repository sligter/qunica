package app.qunica.mobile

import java.io.File
import java.io.OutputStream
import java.util.Base64
import java.util.UUID

/** Private staging only; caller serializes access on its I/O worker. */
internal class DownloadStaging(private val directory: File) {
    data class Entry(val file: File, val name: String, val mime: String, val size: Long)
    private val entries = mutableMapOf<String, Entry>()

    init {
        check(directory.isDirectory || directory.mkdirs()) { "Unable to create download cache" }
        // Recover temporary files left by process death, only in our own cache directory.
        directory.listFiles()?.filter { it.isFile && it.name.endsWith(".part") }?.forEach { it.delete() }
    }

    fun begin(name: String, mime: String, size: Long): String {
        require(size >= 0) { "Invalid download size" }
        check(entries.size < 4) { "Too many downloads in progress" }
        val safeName = name.replace('\\', '/').substringAfterLast('/').filter { !it.isISOControl() }
            .take(180).ifBlank { "download" }
        val id = UUID.randomUUID().toString()
        val file = File(directory, "$id.part")
        check(file.createNewFile()) { "Unable to create download cache" }
        entries[id] = Entry(file, safeName, mime, size)
        return id
    }

    fun append(id: String, offset: Long, data: String) {
        val entry = entry(id)
        require(data.length <= 90_000) { "Download chunk too large" }
        val bytes = Base64.getDecoder().decode(data)
        require(bytes.size <= 65_536) { "Download chunk too large" }
        require(offset == entry.file.length()) { "Invalid download offset" }
        require(bytes.size.toLong() <= entry.size - offset) { "Invalid download length" }
        entry.file.appendBytes(bytes)
    }

    fun entry(id: String): Entry = entries[id] ?: error("Download no longer available")

    fun ready(id: String): Entry = entry(id).also {
        check(it.file.length() == it.size) { "Download is incomplete" }
    }

    fun copyTo(id: String, output: OutputStream) {
        ready(id).file.inputStream().use { it.copyTo(output) }
        output.flush()
    }

    fun discard(id: String) { entries.remove(id)?.file?.delete() }
    fun clear() { entries.keys.toList().forEach(::discard) }
}
