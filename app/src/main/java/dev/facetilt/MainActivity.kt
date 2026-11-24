package dev.facetilt

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.mutableStateOf
import dev.facetilt.ui.FaceTiltRoot
import dev.facetilt.ui.FaceTiltTheme

class MainActivity : ComponentActivity() {
    /** Set when opened from the Quick Settings tile / boot notification to turn FaceTilt on. */
    private val enableRequest = mutableStateOf(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        enableRequest.value = intent?.getBooleanExtra(EXTRA_ENABLE, false) == true
        setContent {
            FaceTiltTheme {
                FaceTiltRoot(
                    repo = (application as FaceTiltApp).settings,
                    enableRequested = enableRequest.value,
                    onEnableRequestHandled = { finishAfterEnable ->
                        enableRequest.value = false
                        // Opened only to start the service from the tile: get out of the way.
                        if (finishAfterEnable) finish()
                    },
                )
            }
        }
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        if (intent.getBooleanExtra(EXTRA_ENABLE, false)) enableRequest.value = true
    }

    companion object {
        const val EXTRA_ENABLE = "dev.facetilt.extra.ENABLE"
    }
}
