package app.qunica.mobile

import android.app.Activity
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.webkit.WebView
import androidx.appcompat.app.AppCompatActivity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject
import java.security.KeyStore
import java.util.concurrent.Executors
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** One encrypted record binds the credential to its server; no plaintext fallback. */
internal class SessionVault(context: Context) {
    private val preferences = context.getSharedPreferences("qunica-secure-session", Context.MODE_PRIVATE)
    private val alias = "qunica.session.v1"

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(alias, null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setKeySize(256).setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build())
        }.generateKey()
    }

    fun read(): String? {
        val record = preferences.getString("record", null) ?: return null
        val parts = JSONObject(record)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, Base64.decode(parts.getString("iv"), Base64.NO_WRAP)))
        return String(cipher.doFinal(Base64.decode(parts.getString("ciphertext"), Base64.NO_WRAP)), Charsets.UTF_8)
    }

    fun write(value: String) {
        require(value.length <= 65_536) { "Session is too large" }
        val data = JSONObject(value)
        require(data.has("server") && data.has("token")) { "Invalid session" }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val encrypted = cipher.doFinal(value.toByteArray(Charsets.UTF_8))
        val record = JSONObject().put("iv", Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
            .put("ciphertext", Base64.encodeToString(encrypted, Base64.NO_WRAP))
        check(preferences.edit().putString("record", record.toString()).commit()) { "Unable to persist session" }
    }
}

@InvokeArg
class WriteSessionArgs { lateinit var value: String }

@TauriPlugin
class SecureSessionPlugin(private val activity: Activity) : Plugin(activity) {
    private val vault = SessionVault(activity)
    private val worker = Executors.newSingleThreadExecutor()
    private var webView: WebView? = null

    override fun load(webView: WebView) { this.webView = webView }

    override fun onResume() {
        webView?.post {
            webView?.evaluateJavascript("document.dispatchEvent(new Event('visibilitychange'))", null)
        }
    }

    override fun onDestroy(activity: AppCompatActivity) { worker.shutdown() }

    @Command
    fun readSession(invoke: Invoke) {
        worker.execute {
            try { invoke.resolve(JSObject().put("value", vault.read() ?: JSONObject.NULL)) }
            catch (_: Exception) { invoke.reject("Unable to read Android secure storage") }
        }
    }

    @Command
    fun writeSession(invoke: Invoke) {
        val args = invoke.parseArgs(WriteSessionArgs::class.java)
        worker.execute {
            try { vault.write(args.value); invoke.resolve(JSObject()) }
            catch (_: Exception) { invoke.reject("Unable to write Android secure storage") }
        }
    }
}
