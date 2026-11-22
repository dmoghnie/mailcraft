package io.github.zidan44.mailcraft.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AssistChip
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.zidan44.mailcraft.Avatar
import io.github.zidan44.mailcraft.Mail

/**
 * A message: the subject, then a header with the sender's avatar and name, the recipients
 * folded to one line (tap for all), the date, and Reply, Reply all and Forward beside the
 * sender; the text below. The top bar archives, deletes, marks unread and flags.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MessageScreen(mail: Mail, snack: SnackbarHostState) {
    val m = mail.view.message
    val row = mail.view.messages.firstOrNull { it.key == m?.key }
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = { mail.act("back") }) { Icon(Symbols.ArrowBack, "Back") } },
                title = {},
                actions = {
                    if (m != null) {
                        IconButton(onClick = { mail.act("row", "key" to m.key, "action" to "archive") }) { Icon(Symbols.Archive, "Archive") }
                        IconButton(onClick = { mail.act("row", "key" to m.key, "action" to "delete") }) { Icon(Symbols.Delete, "Delete") }
                        IconButton(onClick = { mail.act("row", "key" to m.key, "action" to "read"); mail.act("back") }) {
                            Icon(Symbols.MarkEmailUnread, "Mark as unread")
                        }
                        val flagged = row?.flagged == true
                        IconButton(onClick = { mail.act("row", "key" to m.key, "action" to "flag") }) {
                            Icon(if (flagged) Symbols.FlagFilled else Symbols.Flag, if (flagged) "Clear flag" else "Flag", tint = if (flagged) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                },
            )
        },
        snackbarHost = { Snacks(snack) },
    ) { pad ->
        if (m == null) {
            LinearProgressIndicator(Modifier.fillMaxWidth().padding(pad))
            return@Scaffold
        }
        Column(Modifier.fillMaxSize().padding(pad).verticalScroll(rememberScrollState()).padding(horizontal = 16.dp)) {
            Text(m.subject.ifEmpty { "(no subject)" }, style = MaterialTheme.typography.headlineSmall)
            Spacer(Modifier.height(16.dp))
            var all by remember(m.key) { mutableStateOf(false) }
            Row(verticalAlignment = Alignment.Top) {
                AvatarCircle(row?.avatar ?: Avatar(displayName(m.from).take(1).uppercase().ifEmpty { "?" }, 0), 40.dp)
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f).clickable { all = !all }) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(displayName(m.from), fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                        Spacer(Modifier.width(8.dp))
                        Text(shortDate(m.date), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    if (all) {
                        Text("From: " + m.from, style = MaterialTheme.typography.bodySmall)
                        if (m.to.isNotEmpty()) Text("To: " + m.to, style = MaterialTheme.typography.bodySmall)
                        if (m.cc.isNotEmpty()) Text("Cc: " + m.cc, style = MaterialTheme.typography.bodySmall)
                        Text(longDate(m.date), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    } else {
                        Text(
                            "to " + listOf(m.to, m.cc).filter { it.isNotEmpty() }.joinToString(", ") { displayName(it.substringBefore(',')) },
                            style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1, overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                IconButton(onClick = { mail.act("compose", "kind" to "reply") }) { Icon(Symbols.Reply, "Reply") }
            }
            if (m.attachments.isNotEmpty()) {
                Spacer(Modifier.height(8.dp))
                Row {
                    for (a in m.attachments.take(3)) {
                        AssistChip(onClick = {}, label = { Text(a, maxLines = 1, overflow = TextOverflow.Ellipsis) }, leadingIcon = { Icon(Symbols.AttachFile, null, Modifier.size(18.dp)) })
                        Spacer(Modifier.width(8.dp))
                    }
                }
            }
            HorizontalDivider(Modifier.padding(vertical = 12.dp))
            // HTML mail comes as the engine's text: nothing remote is loaded (CLAUDE.md).
            SelectionContainer { Text(m.text, style = MaterialTheme.typography.bodyLarge) }
            Spacer(Modifier.height(24.dp))
            Row(Modifier.fillMaxWidth()) {
                OutlinedButton(onClick = { mail.act("compose", "kind" to "reply") }, Modifier.weight(1f)) { Icon(Symbols.Reply, null); Spacer(Modifier.width(6.dp)); Text("Reply") }
                Spacer(Modifier.width(8.dp))
                OutlinedButton(onClick = { mail.act("compose", "kind" to "replyAll") }, Modifier.weight(1f)) { Icon(Symbols.ReplyAll, null); Spacer(Modifier.width(6.dp)); Text("All") }
                Spacer(Modifier.width(8.dp))
                OutlinedButton(onClick = { mail.act("compose", "kind" to "forward") }, Modifier.weight(1f)) { Icon(Symbols.Forward, null); Spacer(Modifier.width(6.dp)); Text("Forward") }
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}
