package io.github.zidan44.mailcraft.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import io.github.zidan44.mailcraft.Mail
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/** The screen the core says, and what it said went wrong. */
@Composable
fun Screens(mail: Mail) {
    val v = mail.view
    mail.problem?.let { p ->
        AlertDialog(
            onDismissRequest = { mail.dismiss() },
            confirmButton = { TextButton(onClick = { mail.dismiss() }) { Text("OK") } },
            title = { Text("MailCraft") },
            text = { Text(p) },
        )
    }
    val snack = remember { SnackbarHostState() }
    LaunchedEffect(v.notice) {
        val n = v.notice ?: return@LaunchedEffect
        // Shown until read (OK) or timed out; either way the core forgets it.
        snack.showSnackbar(n, actionLabel = "OK", duration = SnackbarDuration.Long)
        mail.act("dismissNotice")
    }
    if (!v.ready) {
        Loading()
        return
    }
    when (v.screen) {
        "messages" -> MessagesScreen(mail, snack)
        "message" -> MessageScreen(mail, snack)
        "compose" -> ComposeScreen(mail, snack)
        "addAccount" -> AddAccountScreen(mail, snack)
        else -> FoldersScreen(mail, snack)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Loading() {
    Scaffold(topBar = { TopAppBar(title = { Text("MailCraft") }) }) { pad ->
        Box(Modifier.fillMaxSize().padding(pad), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
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
