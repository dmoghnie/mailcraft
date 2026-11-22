package io.github.zidan44.mailcraft.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.zidan44.mailcraft.Mail
import io.github.zidan44.mailcraft.View
import kotlinx.coroutines.launch
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/** The list the app opens on: every inbox together with several accounts, else the inbox. */
private fun atHome(v: View): Boolean {
    val open = v.accounts.flatMap { it.folders }.firstOrNull { it.open }
    return if (v.accounts.size > 1) open == null && v.searching == null else open?.role == "inbox" && v.searching == null
}

private fun Mail.goHome() {
    val v = view
    if (v.accounts.size > 1) {
        act("unifiedInbox")
    } else {
        v.accounts.firstOrNull()?.let { a ->
            val inbox = a.folders.firstOrNull { it.role == "inbox" }?.path ?: "INBOX"
            act("openFolder", "account" to a.id, "folder" to inbox)
        }
    }
}

/**
 * The app: a drawer of accounts and folders over the screen the core says. The core's folders
 * screen is the drawer here, so the app opens on the inbox; Back goes back a screen, then to
 * the inbox, then leaves the app.
 */
@Composable
fun Screens(mail: Mail) {
    val v = mail.view
    val drawer = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    val snack = remember { SnackbarHostState() }

    mail.problem?.let { p ->
        AlertDialog(
            onDismissRequest = { mail.dismiss() },
            confirmButton = { TextButton(onClick = { mail.dismiss() }) { Text("OK") } },
            icon = { Icon(Symbols.Error, null) },
            title = { Text("MailCraft") },
            text = { Text(p) },
        )
    }
    LaunchedEffect(v.notice) {
        val n = v.notice ?: return@LaunchedEffect
        // Shown until read or timed out; either way the core forgets it.
        snack.showSnackbar(n, actionLabel = "OK", duration = SnackbarDuration.Long)
        mail.act("dismissNotice")
    }
    // The message last sent, as it goes: Sending…, a wait with Undo, Sent, or kept in the Outbox.
    // A refused one is open again with its reason (the compose screen says it).
    val out = v.outgoing
    LaunchedEffect(out?.id, out?.stage) {
        when (out?.stage) {
            null -> {}
            "sending" -> snack.showSnackbar("Sending…", duration = SnackbarDuration.Indefinite)
            "held" -> {
                val left = ((out.until ?: 0L) - System.currentTimeMillis() / 1000).coerceAtLeast(1)
                val r = snack.showSnackbar("Sending in $left s", actionLabel = "Undo", duration = SnackbarDuration.Indefinite)
                if (r == SnackbarResult.ActionPerformed) mail.act("undoSend")
            }
            "sent" -> { snack.showSnackbar("Sent"); mail.act("dismissSent") }
            "queued" -> { snack.showSnackbar("In the Outbox: it goes when you're back online", duration = SnackbarDuration.Long); mail.act("dismissSent") }
            else -> mail.act("dismissSent")
        }
    }
    // A held message goes on its own once its wait is over: the snackbar's Undo goes with it.
    LaunchedEffect(out?.id, out?.stage, out?.until) {
        if (out?.stage == "held") {
            val ms = ((out.until ?: 0L) * 1000 - System.currentTimeMillis()).coerceAtLeast(0)
            kotlinx.coroutines.delay(ms)
            snack.currentSnackbarData?.dismiss()
        }
    }
    // The folders screen is the drawer here: open the inbox instead.
    LaunchedEffect(v.ready, v.screen, v.accounts.size) {
        if (v.ready && v.screen == "folders" && v.accounts.isNotEmpty()) mail.goHome()
    }

    BackHandler(enabled = drawer.isOpen) { scope.launch { drawer.close() } }
    val home = v.screen == "messages" && atHome(v)
    BackHandler(enabled = !drawer.isOpen && v.ready && v.screen != "folders" && !home) {
        when {
            v.screen == "messages" && v.searching != null -> mail.act("clearSearch")
            v.screen == "messages" -> mail.goHome()
            else -> mail.act("back")
        }
    }

    when {
        !v.ready -> Centered { CircularProgressIndicator() }
        v.accounts.isEmpty() && v.screen != "addAccount" -> Welcome(mail)
        else -> ModalNavigationDrawer(
            drawerState = drawer,
            gesturesEnabled = v.screen == "messages" || drawer.isOpen,
            drawerContent = { Drawer(mail, unified = v.accounts.size > 1 && atHome(v)) { scope.launch { drawer.close() } } },
        ) {
            when (v.screen) {
                "message" -> MessageScreen(mail, snack)
                "compose" -> ComposeScreen(mail, snack)
                "addAccount" -> AddAccountScreen(mail, snack)
                "messages" -> MessagesScreen(mail, snack) { scope.launch { drawer.open() } }
                else -> Centered { CircularProgressIndicator() }
            }
        }
    }
}

@Composable
private fun Centered(content: @Composable () -> Unit) {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { content() }
}

/** No account yet: what MailCraft is, and the one thing to do. */
@Composable
private fun Welcome(mail: Mail) {
    Column(
        Modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(Symbols.Mail, null, Modifier.size(72.dp), tint = MaterialTheme.colorScheme.primary)
        Spacer(Modifier.height(16.dp))
        Text("Welcome to MailCraft", style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(8.dp))
        Text("Your mail, on your phone, from your own servers.", color = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.height(24.dp))
        Button(onClick = { mail.act("startAdding") }) {
            Icon(Symbols.PersonAdd, null)
            Spacer(Modifier.size(8.dp))
            Text("Add an account")
        }
    }
}

@Composable
internal fun Snacks(snack: SnackbarHostState) = SnackbarHost(snack)

/** A message's date as a list shows it: the time today, else the date. */
internal fun shortDate(seconds: Long): String {
    if (seconds <= 0) return ""
    val t = Instant.ofEpochSecond(seconds).atZone(ZoneId.systemDefault())
    return if (t.toLocalDate() == LocalDate.now()) {
        t.format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))
    } else {
        t.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM))
    }
}

internal fun longDate(seconds: Long): String =
    if (seconds <= 0) "" else Instant.ofEpochSecond(seconds).atZone(ZoneId.systemDefault()).format(DateTimeFormatter.ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.SHORT))

/** The name part of `Name <address>`, else the address. */
internal fun displayName(from: String): String {
    val i = from.indexOf('<')
    return if (i > 0) from.substring(0, i).trim().trim('"') else from.trim('<', '>', ' ')
}
