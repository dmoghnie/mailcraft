package io.github.zidan44.mailcraft.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.zidan44.mailcraft.Account
import io.github.zidan44.mailcraft.Folder
import io.github.zidan44.mailcraft.Mail

/** A special folder's symbol; the person's own folders get a folder. */
internal fun roleSymbol(role: String, filled: Boolean): ImageVector = when (role) {
    "inbox" -> if (filled) Symbols.InboxFilled else Symbols.Inbox
    "drafts" -> if (filled) Symbols.DraftsFilled else Symbols.Drafts
    "sent" -> if (filled) Symbols.SendFilled else Symbols.Send
    "archive" -> if (filled) Symbols.ArchiveFilled else Symbols.Archive
    "junk" -> if (filled) Symbols.ReportFilled else Symbols.Report
    "trash" -> if (filled) Symbols.DeleteFilled else Symbols.Delete
    else -> if (filled) Symbols.FolderFilled else Symbols.Folder
}

/**
 * The navigation drawer: All inboxes, then each account with its avatar and unread count, its
 * folders under it as a tree; the chevron folds an account or a folder away (kept by the core,
 * shared with the desktop's folder pane). `go` closes the drawer after a choice.
 */
@Composable
fun Drawer(mail: Mail, unified: Boolean, go: () -> Unit) {
    val v = mail.view
    var removing by remember { mutableStateOf<Account?>(null) }
    ModalDrawerSheet {
        LazyColumn(Modifier.padding(horizontal = 12.dp)) {
            item {
                Text("MailCraft", style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(start = 16.dp, top = 20.dp, bottom = 12.dp))
            }
            if (v.accounts.size > 1) {
                item {
                    NavigationDrawerItem(
                        icon = { Icon(if (unified) Symbols.AllInboxFilled else Symbols.AllInbox, null) },
                        label = { Text("All inboxes") },
                        badge = { Count(v.accounts.sumOf { it.unread }) },
                        selected = unified,
                        onClick = { mail.act("unifiedInbox"); go() },
                    )
                    HorizontalDivider(Modifier.padding(vertical = 8.dp))
                }
            }
            for (a in v.accounts) {
                item(key = "account:" + a.id) {
                    AccountHeader(a, onToggle = { mail.act("collapse", "account" to a.id) }, onRemove = { removing = a })
                }
                items(a.folders, key = { a.id + "/" + it.path }) { f ->
                    FolderItem(f, onOpen = { mail.act("openFolder", "account" to a.id, "folder" to f.path); go() }) {
                        mail.act("collapse", "account" to a.id, "folder" to f.path)
                    }
                }
                item { HorizontalDivider(Modifier.padding(vertical = 8.dp)) }
            }
            item {
                NavigationDrawerItem(
                    icon = { Icon(Symbols.PersonAdd, null) },
                    label = { Text("Add account") },
                    selected = false,
                    onClick = { mail.act("startAdding"); go() },
                )
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

@Composable
private fun Count(n: Long) {
    if (n > 0) Text(if (n > 9999) "9999+" else n.toString(), style = MaterialTheme.typography.labelLarge)
}

@Composable
private fun AccountHeader(a: Account, onToggle: () -> Unit, onRemove: () -> Unit) {
    var menu by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onToggle).padding(start = 12.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        AvatarCircle(a.avatar, 36.dp)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(a.name.ifEmpty { a.email }, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            val state = a.error ?: a.state.takeIf { it != "online" && it != "idle" && it != "offline" }
            Text(
                state ?: a.email, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis,
                color = if (a.error != null) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (a.collapsed && a.unread > 0) {
            Spacer(Modifier.width(8.dp))
            Count(a.unread)
        }
        IconButton(onClick = onToggle) {
            Icon(if (a.collapsed) Symbols.ExpandMore else Symbols.ExpandLess, if (a.collapsed) "Show ${a.email}'s folders" else "Hide ${a.email}'s folders")
        }
        IconButton(onClick = { menu = true }) { Icon(Symbols.MoreVert, "More for ${a.email}") }
        androidx.compose.material3.DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            androidx.compose.material3.DropdownMenuItem(
                text = { Text("Remove account") }, leadingIcon = { Icon(Symbols.Delete, null) },
                onClick = { menu = false; onRemove() },
            )
        }
    }
}

@Composable
private fun FolderItem(f: Folder, onOpen: () -> Unit, onToggle: () -> Unit) {
    NavigationDrawerItem(
        modifier = Modifier.padding(start = (f.depth * 16).dp),
        icon = { Icon(roleSymbol(f.role, f.open), null) },
        label = {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.Start) {
                Text(f.name, Modifier.weight(1f, fill = false), maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (f.parent) {
                    IconButton(onClick = onToggle, modifier = Modifier.size(32.dp)) {
                        Icon(if (f.collapsed) Symbols.ChevronRight else Symbols.ExpandMore, if (f.collapsed) "Show folders in ${f.name}" else "Hide folders in ${f.name}")
                    }
                }
            }
        },
        badge = { Count(f.unread) },
        selected = f.open,
        onClick = onOpen,
    )
}
