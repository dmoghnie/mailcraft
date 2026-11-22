package io.github.zidan44.mailcraft.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.BottomAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.zidan44.mailcraft.Mail

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MessageScreen(mail: Mail, snack: SnackbarHostState) {
    val m = mail.view.message
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { TextButton(onClick = { mail.act("back") }) { Text("‹") } },
                title = { Text(m?.subject?.ifEmpty { "(no subject)" } ?: "", maxLines = 1, overflow = TextOverflow.Ellipsis) },
                actions = {
                    if (m != null) {
                        TextButton(onClick = { mail.act("row", "key" to m.key, "action" to "flag") }) { Text("Flag") }
                        TextButton(onClick = { mail.act("row", "key" to m.key, "action" to "delete") }) { Text("Delete") }
                    }
                },
            )
        },
        bottomBar = {
            BottomAppBar {
                TextButton(onClick = { mail.act("compose", "kind" to "reply") }, Modifier.weight(1f)) { Text("Reply") }
                TextButton(onClick = { mail.act("compose", "kind" to "replyAll") }, Modifier.weight(1f)) { Text("Reply all") }
                TextButton(onClick = { mail.act("compose", "kind" to "forward") }, Modifier.weight(1f)) { Text("Forward") }
            }
        },
        snackbarHost = { Snacks(snack) },
    ) { pad ->
        if (m == null) {
            LinearProgressIndicator(Modifier.fillMaxWidth().padding(pad))
            return@Scaffold
        }
        Column(Modifier.fillMaxSize().padding(pad).verticalScroll(rememberScrollState()).padding(16.dp)) {
            Text(m.subject.ifEmpty { "(no subject)" }, style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(8.dp))
            Text(m.from, style = MaterialTheme.typography.titleSmall)
            if (m.to.isNotEmpty()) Text("To: " + m.to, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (m.cc.isNotEmpty()) Text("Cc: " + m.cc, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(longDate(m.date), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (m.attachments.isNotEmpty()) {
                Spacer(Modifier.height(4.dp))
                Text("📎 " + m.attachments.joinToString(", "), style = MaterialTheme.typography.bodySmall)
            }
            HorizontalDivider(Modifier.padding(vertical = 12.dp))
            // HTML mail comes as the engine's text: nothing remote is loaded (CLAUDE.md).
            SelectionContainer { Text(m.text, style = MaterialTheme.typography.bodyMedium) }
        }
    }
}
