package app.qunica.mobile

import android.content.Context
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class SessionVaultTest {
    @Test
    fun credentialsAreEncryptedAndSignOutReplacesTheStoredToken() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val vault = SessionVault(context)
        val original = vault.read()
        try {
            val session = """{"server":"https://test.example","token":"instrumentation-secret"}"""
            vault.write(session)
            assertEquals(session, SessionVault(context).read())
            val record = context.getSharedPreferences("qunica-secure-session", Context.MODE_PRIVATE).getString("record", "")!!
            assertFalse(record.contains("instrumentation-secret"))
            assertFalse(record.contains("test.example"))
            vault.write("""{"server":"https://test.example","token":null}""")
            assertTrue(JSONObject(SessionVault(context).read()!!).isNull("token"))
        } finally {
            vault.write(original ?: """{"server":null,"token":null}""")
        }
    }

    @Test
    fun corruptedCiphertextCannotBeReadAsAValidSession() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val prefs = context.getSharedPreferences("qunica-secure-session", Context.MODE_PRIVATE)
        val original = prefs.getString("record", null)
        try {
            SessionVault(context).write("""{"server":"https://test.example","token":"test"}""")
            val record = JSONObject(prefs.getString("record", null)!!)
            record.put("ciphertext", "AAAA")
            assertTrue(prefs.edit().putString("record", record.toString()).commit())
            try {
                SessionVault(context).read()
                fail("Tampered ciphertext must be rejected")
            } catch (_: java.security.GeneralSecurityException) { /* expected */ }
        } finally {
            prefs.edit().putString("record", original).commit()
        }
    }
}
