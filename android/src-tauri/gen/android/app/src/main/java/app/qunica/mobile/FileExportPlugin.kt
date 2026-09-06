package app.qunica.mobile

import android.app.Activity
import android.content.Intent
import android.provider.DocumentsContract
import android.webkit.MimeTypeMap
import androidx.activity.result.ActivityResult
import androidx.appcompat.app.AppCompatActivity
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject
import java.io.File
import java.util.concurrent.Executors

@InvokeArg
class BeginExportArgs { lateinit var name: String; var mime: String = "application/octet-stream"; var size: Long = 0 }
@InvokeArg
class ExportIdArgs { lateinit var id: String }
@InvokeArg
class AppendExportArgs { lateinit var id: String; var offset: Long = 0; lateinit var data: String }

@TauriPlugin
class FileExportPlugin(private val activity: Activity) : Plugin(activity) {
    private val worker = Executors.newSingleThreadExecutor()
    private val staging by lazy { DownloadStaging(File(activity.cacheDir, "file-exports")) }
    // Accessed on worker only; Android can show just one document picker at a time.
    private var picking: String? = null

    @Command
    fun beginExport(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(BeginExportArgs::class.java)
        invoke.resolve(JSObject().put("id", staging.begin(args.name, args.mime, args.size)))
    }

    @Command
    fun appendExport(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(AppendExportArgs::class.java)
        check(picking != args.id) { "Download is being saved" }
        staging.append(args.id, args.offset, args.data)
        invoke.resolve()
    }

    @Command
    fun discardExport(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(ExportIdArgs::class.java)
        if (picking != args.id) staging.discard(args.id)
        invoke.resolve()
    }

    @Command
    fun saveExport(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(ExportIdArgs::class.java)
        check(picking == null) { "Another file is being saved" }
        val entry = staging.ready(args.id)
        picking = args.id
        val mime = entry.mime.takeIf { it != "application/octet-stream" && it.contains('/') }
            ?: MimeTypeMap.getSingleton().getMimeTypeFromExtension(entry.name.substringAfterLast('.', "").lowercase())
            ?: "application/octet-stream"
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = mime
            putExtra(Intent.EXTRA_TITLE, entry.name)
            putExtra(DocumentsContract.EXTRA_INITIAL_URI,
                DocumentsContract.buildDocumentUri("com.android.externalstorage.documents", "primary:Download"))
        }
        activity.runOnUiThread {
            try { startActivityForResult(invoke, intent, "exportResult") }
            catch (error: Exception) {
                work(invoke) {
                    picking = null
                    staging.discard(args.id)
                    invoke.reject(error.message ?: "Unable to open Android file picker")
                }
            }
        }
    }

    @ActivityCallback
    fun exportResult(invoke: Invoke, result: ActivityResult) = work(invoke) {
        val args = invoke.parseArgs(ExportIdArgs::class.java)
        val uri = result.data?.data
        try {
            if (result.resultCode == Activity.RESULT_CANCELED) {
                invoke.resolve(JSObject().put("uri", JSONObject.NULL))
            } else {
                check(result.resultCode == Activity.RESULT_OK && uri != null) { "No destination selected" }
                try {
                    val output = activity.contentResolver.openOutputStream(uri, "w")
                        ?: error("Unable to open download destination")
                    output.use { staging.copyTo(args.id, it) }
                } catch (error: Exception) {
                    // ACTION_CREATE_DOCUMENT creates a new file; remove an incomplete copy.
                    runCatching { DocumentsContract.deleteDocument(activity.contentResolver, uri) }
                    throw error
                }
                invoke.resolve(JSObject().put("uri", uri.toString()))
            }
        } finally {
            picking = null
            staging.discard(args.id)
        }
    }

    private fun work(invoke: Invoke, action: () -> Unit) {
        worker.execute {
            try { action() }
            catch (error: Exception) { invoke.reject(error.message ?: "Unable to save download") }
        }
    }

    override fun onDestroy(activity: AppCompatActivity) {
        worker.execute { staging.clear() }
        worker.shutdown()
    }
}
