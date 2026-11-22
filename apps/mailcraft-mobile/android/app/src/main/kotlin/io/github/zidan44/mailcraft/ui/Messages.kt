package io.github.zidan44.mailcraft.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.zidan44.mailcraft.Mail
import kotlinx.coroutines.delay
import io.github.zidan44.mailcraft.Row as MailRow

/**
 * The message list: a search bar on top (the drawer's button, the search field, the account's
 * avatar), the rows with their avatars, swipe right to archive and left to delete, pull to
 * check for mail, and New mail, which shrinks to its icon once the list scrolls.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MessagesScreen(mail: Mail, snack: SnackbarHostState, openDrawer: () -> Unit) {
    val v = mail.view
    val list = rememberLazyListState()
    val atTop by remember { derivedStateOf { list.firstVisibleItemIndex == 0 } }
    var refreshing by remember { mutableStateOf(false) }
    LaunchedEffect(refreshing) {
        if (refreshing) {
            delay(1200)
            refreshing = false
        }
    }
    Scaffold(
        topBar = { SearchBar(mail, openDrawer) },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = { mail.act("compose", "kind" to "new") },
                icon = { Icon(Symbols.Edit, null) },
                text = { Text("New mail") },
                expanded = atTop,
            )
        },
        snackbarHost = { Snacks(snack) },
    ) { pad ->
        PullToRefreshBox(
            isRefreshing = refreshing,
            onRefresh = { refreshing = true; mail.act("refresh") },
            modifier = Modifier.fillMaxSize().padding(pad),
        ) {
            if (v.messages.isEmpty()) {
                Empty(if (v.searching != null) "Nothing found" else "You're all caught up")
                return@PullToRefreshBox
            }
            LazyColumn(Modifier.fillMaxSize(), state = list) {
                item {
                    Text(
                        v.title, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.padding(start = 20.dp, top = 4.dp, bottom = 4.dp),
                    )
                }
                items(v.messages, key = { it.key }) { m ->
                    SwipeRow(m, onAct = { a -> mail.act("row", "key" to m.key, "action" to a) }) {
                        MessageRow(m, onOpen = { mail.act("openMessage", "key" to m.key) }, onRow = { a -> mail.act("row", "key" to m.key, "action" to a) })
                    }
                }
                if (v.messages.size < v.total) {
                    item {
                        TextButton(onClick = { mail.act("loadMore") }, modifier = Modifier.fillMaxWidth().padding(8.dp)) {
                            Text("Load more (${v.total - v.messages.size} older)")
                        }
                    }
                }
                item { Spacer(Modifier.height(88.dp)) }
            }
        }
    }
}

@Composable
private fun Empty(text: String) {
    Column(Modifier.fillMaxSize().padding(32.dp), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
        Icon(Symbols.DoneAll, null, Modifier.size(56.dp), tint = MaterialTheme.colorScheme.outline)
        Spacer(Modifier.height(12.dp))
        Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** Gmail's kind of search bar: a rounded field with the drawer's button and the account's avatar. */
@Composable
private fun SearchBar(mail: Mail, openDrawer: () -> Unit) {
    val v = mail.view
    var query by remember { mutableStateOf(v.searching ?: "") }
    Surface(
        shape = RoundedCornerShape(28.dp),
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        modifier = Modifier.statusBarsPadding().fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp).height(56.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (v.searching != null) {
                IconButton(onClick = { query = ""; mail.act("clearSearch") }) { Icon(Symbols.ArrowBack, "Close search") }
            } else {
                IconButton(onClick = openDrawer) { Icon(Symbols.Menu, "Accounts and folders") }
            }
            Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                if (query.isEmpty()) Text("Search in mail", color = MaterialTheme.colorScheme.onSurfaceVariant)
                BasicTextField(
                    value = query,
                    onValueChange = { query = it },
                    singleLine = true,
                    textStyle = MaterialTheme.typography.bodyLarge.copy(color = MaterialTheme.colorScheme.onSurface),
                    cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    keyboardActions = KeyboardActions(onSearch = { if (query.isNotBlank()) mail.act("search", "query" to query.trim()) }),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            if (query.isNotEmpty()) {
                IconButton(onClick = { query = ""; if (v.searching != null) mail.act("clearSearch") }) { Icon(Symbols.Close, "Clear") }
            } else {
                // The account whose mail this is (the first one, for every inbox together).
                val a = v.accounts.firstOrNull { acc -> acc.folders.any { it.open } } ?: v.accounts.firstOrNull()
                if (a != null) {
                    Box(Modifier.padding(end = 8.dp).clickable(onClick = openDrawer)) { AvatarCircle(a.avatar, 32.dp) }
                }
            }
        }
    }
}

/** Swipe right to archive, left to delete; the colour and the symbol show as the row moves. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SwipeRow(m: MailRow, onAct: (String) -> Unit, content: @Composable () -> Unit) {
    val state = rememberSwipeToDismissBoxState()
    SwipeToDismissBox(
        state = state,
        onDismiss = { value ->
            when (value) {
                SwipeToDismissBoxValue.StartToEnd -> onAct("archive")
                SwipeToDismissBoxValue.EndToStart -> onAct("delete")
                SwipeToDismissBoxValue.Settled -> {}
            }
        },
        backgroundContent = {
            val toEnd = state.dismissDirection == SwipeToDismissBoxValue.StartToEnd
            val color = if (toEnd) Color(0xFF2E7D32) else MaterialTheme.colorScheme.error
            Box(
                Modifier.fillMaxSize().background(color).padding(horizontal = 24.dp),
                contentAlignment = if (toEnd) Alignment.CenterStart else Alignment.CenterEnd,
            ) {
                Icon(if (toEnd) Symbols.Archive else Symbols.Delete, if (toEnd) "Archive" else "Delete", tint = Color.White)
            }
        },
    ) { content() }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun MessageRow(m: MailRow, onOpen: () -> Unit, onRow: (String) -> Unit) {
    var menu by remember { mutableStateOf(false) }
    val bold = if (m.unread) FontWeight.Bold else FontWeight.Normal
    Box(Modifier.background(MaterialTheme.colorScheme.surface)) {
        Row(
            Modifier.fillMaxWidth().combinedClickable(onClick = onOpen, onLongClick = { menu = true }).padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.Top,
        ) {
            AvatarCircle(m.avatar, 40.dp)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(displayName(m.from), Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, fontWeight = bold)
                    Text(
                        shortDate(m.date), style = MaterialTheme.typography.labelMedium, fontWeight = bold,
                        color = if (m.unread) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(m.subject.ifEmpty { "(no subject)" }, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, fontWeight = bold)
                    if (m.attachments) Icon(Symbols.AttachFile, "Has attachments", Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    if (m.flagged) Icon(Symbols.FlagFilled, "Flagged", Modifier.size(16.dp), tint = MaterialTheme.colorScheme.error)
                }
                Text(m.preview, maxLines = 2, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            DropdownMenuItem(text = { Text("Archive") }, leadingIcon = { Icon(Symbols.Archive, null) }, onClick = { menu = false; onRow("archive") })
            DropdownMenuItem(
                text = { Text(if (m.unread) "Mark as read" else "Mark as unread") },
                leadingIcon = { Icon(if (m.unread) Symbols.MarkEmailRead else Symbols.MarkEmailUnread, null) },
                onClick = { menu = false; onRow("read") },
            )
            DropdownMenuItem(text = { Text(if (m.flagged) "Clear flag" else "Flag") }, leadingIcon = { Icon(Symbols.Flag, null) }, onClick = { menu = false; onRow("flag") })
            DropdownMenuItem(text = { Text("Delete") }, leadingIcon = { Icon(Symbols.Delete, null) }, onClick = { menu = false; onRow("delete") })
        }
    }
}
