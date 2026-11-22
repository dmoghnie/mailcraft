package io.github.zidan44.mailcraft.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.zidan44.mailcraft.Mail
import io.github.zidan44.mailcraft.Row as MailRow

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MessagesScreen(mail: Mail, snack: SnackbarHostState) {
    val v = mail.view
    var searching by remember { mutableStateOf(v.searching != null) }
    var query by remember { mutableStateOf(v.searching ?: "") }
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { TextButton(onClick = { mail.act("back") }) { Text("‹") } },
                title = { Text(v.title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                actions = {
                    TextButton(onClick = {
                        if (searching && v.searching != null) mail.act("clearSearch")
                        searching = !searching
                        query = ""
                    }) { Text(if (searching) "Close" else "Search") }
                    TextButton(onClick = { mail.act("refresh") }) { Text("Refresh") }
                },
            )
        },
        floatingActionButton = { ExtendedFloatingActionButton(onClick = { mail.act("compose", "kind" to "new") }) { Text("New mail") } },
        snackbarHost = { Snacks(snack) },
    ) { pad ->
        Column(Modifier.fillMaxSize().padding(pad)) {
            if (searching) {
                OutlinedTextField(
                    value = query,
                    onValueChange = { query = it },
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
                    placeholder = { Text("Search mail") },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    keyboardActions = KeyboardActions(onSearch = { if (query.isNotBlank()) mail.act("search", "query" to query.trim()) }),
                )
            }
            if (v.messages.isEmpty()) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Text("Nothing here", color = MaterialTheme.colorScheme.onSurfaceVariant) }
                return@Column
            }
            LazyColumn(Modifier.fillMaxSize()) {
                items(v.messages, key = { it.key }) { m ->
                    MessageRow(m, onOpen = { mail.act("openMessage", "key" to m.key) }, onRow = { a -> mail.act("row", "key" to m.key, "action" to a) })
                    HorizontalDivider()
                }
                if (v.messages.size < v.total) {
                    item {
                        TextButton(onClick = { mail.act("loadMore") }, modifier = Modifier.fillMaxWidth().padding(8.dp)) {
                            Text("Load more (${v.total - v.messages.size} older)")
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun MessageRow(m: MailRow, onOpen: () -> Unit, onRow: (String) -> Unit) {
    var menu by remember { mutableStateOf(false) }
    Box {
        Row(Modifier.fillMaxWidth().combinedClickable(onClick = onOpen, onLongClick = { menu = true }).padding(horizontal = 16.dp, vertical = 10.dp)) {
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        displayName(m.from), Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis,
                        fontWeight = if (m.unread) FontWeight.Bold else FontWeight.Normal,
                    )
                    Spacer(Modifier.width(8.dp))
                    Text(shortDate(m.date), style = MaterialTheme.typography.bodySmall, color = if (m.unread) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        m.subject.ifEmpty { "(no subject)" }, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis,
                        fontWeight = if (m.unread) FontWeight.SemiBold else FontWeight.Normal, color = if (m.unread) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                    )
                    if (m.attachments) Text(" 📎", style = MaterialTheme.typography.bodySmall)
                    if (m.flagged) Text(" ⚑", color = MaterialTheme.colorScheme.error)
                }
                Text(m.preview, maxLines = 2, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            DropdownMenuItem(text = { Text(if (m.unread) "Mark as read" else "Mark as unread") }, onClick = { menu = false; onRow("read") })
            DropdownMenuItem(text = { Text(if (m.flagged) "Clear flag" else "Flag") }, onClick = { menu = false; onRow("flag") })
            DropdownMenuItem(text = { Text("Delete") }, onClick = { menu = false; onRow("delete") })
        }
    }
}
