package io.github.zidan44.mailcraft

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import io.github.zidan44.mailcraft.ui.Screens

class MainActivity : ComponentActivity() {
    private val zoneChanged = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) = mail.timeZoneChanged()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        registerReceiver(zoneChanged, IntentFilter(Intent.ACTION_TIMEZONE_CHANGED))
        setContent {
            Theme {
                // Back goes back a screen in the core; on the folders it leaves the app.
                BackHandler(enabled = mail.view.screen != "folders") { mail.act("back") }
                Screens(mail)
            }
        }
    }

    override fun onDestroy() {
        unregisterReceiver(zoneChanged)
        super.onDestroy()
    }

    private val mail: Mail get() = App.mail(this)
}

/** One core per process: the activity is made again on rotation, the core is not. */
object App {
    private var instance: Mail? = null
    fun mail(context: Context): Mail = instance ?: Mail(context.applicationContext).also { instance = it }
}

/** MailCraft's own colour, as the desktop's theme has it (`ui-egui/src/look/theme.rs`). */
private val AppColor = Color(0xFF0E7C86)
private val AppColorDark = Color(0xFF3FB8C4)

@Composable
private fun Theme(content: @Composable () -> Unit) {
    val colors = if (isSystemInDarkTheme()) darkColorScheme(primary = AppColorDark, onPrimary = Color(0xFF0B1E21)) else lightColorScheme(primary = AppColor)
    MaterialTheme(colorScheme = colors, content = content)
}
