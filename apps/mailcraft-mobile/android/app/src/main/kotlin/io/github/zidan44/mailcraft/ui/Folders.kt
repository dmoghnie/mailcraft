package io.github.zidan44.mailcraft.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.zidan44.mailcraft.Account
import io.github.zidan44.mailcraft.Folder
import io.github.zidan44.mailcraft.Mail

/** The roles shown first in each account, in this order; the other folders follow by name. */
private val ORDER = listOf("inbox", "drafts", "sent", "archive", "junk", "trash")

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FoldersScreen(mail: Mail, snack: SnackbarHostState) {
    val v = mail.view
    var removing by remember { mutableStateOf<Account?>(null) }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("MailCraft") },
                actions = {
                    TextButton(onClick = { mail.act("refresh") }) { Text("Refresh") }
                    TextButton(onClick = { mail.act("startAdding") }) { Text("Add") }
                },
            )
        },
        snackbarHost = { Snacks(snack) },
    ) { pad ->
        if (v.accounts.isEmpty()) {
            Column(Modifier.fillMaxSize().padding(pad).padding(24.dp), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
                Text("No account yet", style = MaterialTheme.typography.titleLarge)
                Spacer(Modifier.padding(8.dp))
                TextButton(onClick = { mail.act("startAdding") }) { Text("Add an account") }
            }
            return@Scaffold
        }
        LazyColumn(Modifier.fillMaxSize().padding(pad)) {
            if (v.accounts.size > 1) {
                item {
                    val unread = v.accounts.sumOf { a -> a.folders.filter { it.role == "inbox" }.sumOf { it.unread } }
                    FolderRow("All inboxes", unread, bold = true) { mail.act("unifiedInbox") }
                    HorizontalDivider()
                }
            }
            for (a in v.accounts) {
                item(key = "account:" + a.id) { AccountHeader(a) { removing = a } }
                items(sorted(a.folders), key = { a.id + "/" + it.path }) { f ->
                    FolderRow(f.name, f.unread, bold = f.role == "inbox") { mail.act("openFolder", "account" to a.id, "folder" to f.path) }
                }
                item { HorizontalDivider() }
            }
        }
    }
    removing?.let { a ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text("Remove ${a.email}?") },
            text = { Text("Its mail on this phone goes; the mail on the server stays.") },
            confirmButton = { TextButton(onClick = { mail.act("removeAccount", "account" to a.id); removing = null }) { Text("Remove") } },
            dismissButton = { TextButton(onClick = { removing = null }) { Text("Cancel") } },
        )
    }
}

private fun sorted(folders: List<Folder>): List<Folder> =
    folders.sortedWith(compareBy<Folder>({ ORDER.indexOf(it.role).let { i -> if (i < 0) ORDER.size else i } }, { it.path.lowercase() }))

@Composable
private fun AccountHeader(a: Account, onRemove: () -> Unit) {
    var menu by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, top = 12.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(a.email, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
            val state = a.error ?: a.state.takeIf { it != "online" && it != "idle" }
            if (state != null) Text(state, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Row {
            TextButton(onClick = { menu = true }) { Text("⋮") }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                DropdownMenuItem(text = { Text("Remove account") }, onClick = { menu = false; onRemove() })
            }
        }
    }
}

@Composable
private fun FolderRow(name: String, unread: Long, bold: Boolean, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(name, Modifier.weight(1f), fontWeight = if (bold || unread > 0) FontWeight.SemiBold else FontWeight.Normal, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (unread > 0) {
            Spacer(Modifier.width(8.dp))
            Text(unread.toString(), color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.SemiBold)
        }
    }
}
