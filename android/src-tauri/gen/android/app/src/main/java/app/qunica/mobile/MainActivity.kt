package app.qunica.mobile

import android.os.Bundle
import android.graphics.Color
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    // Resize the native content area for bars/cutouts and IME. This also gives
    // the shared 100dvh shell the actual space left above the Android keyboard.
    val content = findViewById<android.view.View>(android.R.id.content)
    content.setBackgroundColor(Color.rgb(36, 33, 30))
    WindowCompat.getInsetsController(window, content).apply {
      isAppearanceLightStatusBars = false
      isAppearanceLightNavigationBars = false
    }
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val safe = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime())
      view.setPadding(safe.left, safe.top, safe.right, safe.bottom)
      WindowInsetsCompat.CONSUMED
    }
    ViewCompat.requestApplyInsets(content)
  }
}
