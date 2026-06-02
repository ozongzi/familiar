package app.familiar.client

import android.os.Bundle
import android.view.View
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)

    // Edge-to-edge draws the WebView behind the system bars and the soft
    // keyboard, and on Android 15+ `adjustResize` no longer shrinks the window
    // for the IME — so the keyboard just overlaps the focused input and the
    // WebView never reports a smaller viewport (the JS VisualViewport handler
    // gets no resize signal).
    //
    // Apply the IME inset as bottom padding on the content view so the WebView
    // physically shrinks to sit above the keyboard. We subtract the navigation
    // bar inset (already handled via CSS safe-area in edge-to-edge), leaving
    // only the extra keyboard height: padding is 0 when the keyboard is hidden,
    // preserving the edge-to-edge layout.
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { v, insets ->
      val ime = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
      val navBar = insets.getInsets(WindowInsetsCompat.Type.navigationBars()).bottom
      v.setPadding(0, 0, 0, maxOf(ime - navBar, 0))
      insets
    }
  }
}
