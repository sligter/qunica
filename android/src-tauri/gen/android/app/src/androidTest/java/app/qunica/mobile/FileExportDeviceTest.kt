package app.qunica.mobile

import android.net.Uri
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.view.accessibility.AccessibilityNodeInfo
import android.webkit.WebView
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Exercise the actual JS → Rust → Kotlin bridge and Android DocumentsUI. */
@RunWith(AndroidJUnit4::class)
class FileExportDeviceTest {
    private fun webView(view: View): WebView? {
        if (view is WebView) return view
        if (view is ViewGroup) for (i in 0 until view.childCount) webView(view.getChildAt(i))?.let { return it }
        return null
    }

    private fun evaluate(scenario: ActivityScenario<MainActivity>, script: String): String {
        val done = CountDownLatch(1)
        var value = "null"
        scenario.onActivity { activity ->
            val web = webView(activity.findViewById(android.R.id.content)) ?: error("WebView missing")
            web.evaluateJavascript(script) { value = it; done.countDown() }
        }
        check(done.await(10, TimeUnit.SECONDS)) { "WebView did not respond" }
        return value
    }

    private fun eventually(check: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 30_000
        while (SystemClock.uptimeMillis() < deadline) {
            if (check()) return
            SystemClock.sleep(200)
        }
        error("Timed out waiting for Android export")
    }

    private fun findSave(node: AccessibilityNodeInfo?): AccessibilityNodeInfo? {
        if (node == null) return null
        if (node.viewIdResourceName == "android:id/button1" && node.isEnabled && node.isClickable) return node
        if (node.isClickable && node.isEnabled && node.text?.toString()?.lowercase() in listOf("save", "保存")) return node
        for (i in 0 until node.childCount) findSave(node.getChild(i))?.let { return it }
        return null
    }

    @Test fun systemPickerSavesBinaryBytesAndCancellationLeavesNoStaging() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        // `let`, not `use`: destroying the Tauri activity takes the whole process
        // down, which races the runner's result flush and reports a passing test
        // as a crash. Instrumentation reclaims the activity when the run ends, so
        // this class runs in its own invocation. See android/README.md.
        ActivityScenario.launch(MainActivity::class.java).let { scenario ->
            eventually { evaluate(scenario, "typeof window.__TAURI_INTERNALS__?.invoke") == "\"function\"" }
            val name = "qunica-download-smoke-${System.currentTimeMillis()}.bin"
            val script = """
                window.__exportSmoke = null;
                (async () => {
                  const invoke = window.__TAURI_INTERNALS__.invoke;
                  const {id} = await invoke('mobile_file_export', {operation:'begin',payload:{name:'$name',mime:'application/octet-stream',size:70003}});
                  for(let offset=0; offset<70003; offset+=65536) {
                    let data=''; for(let i=offset; i<Math.min(offset+65536,70003); i++) data+=String.fromCharCode(i%256);
                    await invoke('mobile_file_export',{operation:'append',payload:{id,offset,data:btoa(data)}});
                  }
                  const result = await invoke('mobile_file_export',{operation:'save',payload:{id}});
                  window.__exportSmoke = result;
                })().catch(e => window.__exportSmoke = {error:String(e)});
            """.trimIndent()
            evaluate(scenario, script)
            eventually {
                val save = findSave(instrumentation.uiAutomation.rootInActiveWindow)
                save?.performAction(AccessibilityNodeInfo.ACTION_CLICK) == true
            }
            eventually { evaluate(scenario, "window.__exportSmoke") != "null" }
            val result = JSONObject(evaluate(scenario, "window.__exportSmoke"))
            assertFalse(result.toString(), result.has("error"))
            val uri = Uri.parse(result.getString("uri"))
            try {
                val bytes = instrumentation.targetContext.contentResolver.openInputStream(uri)!!.use { it.readBytes() }
                assertArrayEquals(ByteArray(70003) { (it % 256).toByte() }, bytes)
            } finally {
                android.provider.DocumentsContract.deleteDocument(instrumentation.targetContext.contentResolver, uri)
            }
            val cache = java.io.File(instrumentation.targetContext.cacheDir, "file-exports")
            eventually { cache.listFiles().orEmpty().isEmpty() }

            evaluate(scenario, """
                window.__exportSmoke = null;
                (async () => {
                  const invoke=window.__TAURI_INTERNALS__.invoke;
                  const {id}=await invoke('mobile_file_export',{operation:'begin',payload:{name:'cancelled.bin',size:0}});
                  window.__exportSmoke=await invoke('mobile_file_export',{operation:'save',payload:{id}});
                })().catch(e=>window.__exportSmoke={error:String(e)});
            """.trimIndent())
            eventually { findSave(instrumentation.uiAutomation.rootInActiveWindow) != null }
            // DocumentsUI reopens where the last save landed, and Back navigates
            // out of that folder before it dismisses the picker. Keep pressing
            // until the bridge reports the cancellation.
            eventually {
                if (evaluate(scenario, "window.__exportSmoke") != "null") return@eventually true
                instrumentation.uiAutomation.performGlobalAction(
                    android.accessibilityservice.AccessibilityService.GLOBAL_ACTION_BACK
                )
                SystemClock.sleep(400)
                evaluate(scenario, "window.__exportSmoke") != "null"
            }
            assertTrue(JSONObject(evaluate(scenario, "window.__exportSmoke")).isNull("uri"))
            eventually { cache.listFiles().orEmpty().isEmpty() }
        }
    }
}
