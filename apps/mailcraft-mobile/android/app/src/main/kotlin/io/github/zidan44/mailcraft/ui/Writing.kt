package io.github.zidan44.mailcraft.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LinearProgressIndicator
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
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import io.github.zidan44.mailcraft.Mail

/**
 * The message being written. What is typed is kept here while the same message is open (a
 * render that comes back mid-typing never moves the cursor) and sent to the core as it changes.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ComposeScreen(mail: Mail, snack: SnackbarHostState) {
    val c = mail.view.compose ?: return
    var to by remember(c.id) { mutableStateOf(c.to) }
    var cc by remember(c.id) { mutableStateOf(c.cc) }
    var subject by remember(c.id) { mutableStateOf(c.subject) }
    var body by remember(c.id) { mutableStateOf(c.body) }
    var menu by remember { mutableStateOf(false) }
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { TextButton(onClick = { mail.act("back") }) { Text("‹") } },
                title = { Text(subject.ifEmpty { "New message" }, maxLines = 1) },
                actions = {
                    Button(onClick = { mail.act("send") }, enabled = !c.sending && to.isNotBlank()) { Text("Send") }
                    TextButton(onClick = { menu = true }) { Text("⋮") }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        DropdownMenuItem(text = { Text("Discard") }, onClick = { menu = false; mail.act("discard") })
                    }
                },
            )
        },
        snackbarHost = { Snacks(snack) },
    ) { pad ->
        Column(Modifier.fillMaxSize().padding(pad).imePadding().verticalScroll(rememberScrollState()).padding(horizontal = 12.dp)) {
            if (c.sending) LinearProgressIndicator(Modifier.fillMaxWidth())
            c.error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(vertical = 4.dp)) }
            if (c.from.isNotEmpty()) Text("From: " + c.from, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(vertical = 4.dp))
            val field = Modifier.fillMaxWidth().padding(vertical = 2.dp)
            val email = KeyboardOptions(keyboardType = KeyboardType.Email, capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false)
            OutlinedTextField(to, { to = it; mail.act("edit", "to" to it) }, field, label = { Text("To") }, keyboardOptions = email)
            OutlinedTextField(cc, { cc = it; mail.act("edit", "cc" to it) }, field, label = { Text("Cc") }, keyboardOptions = email)
            OutlinedTextField(subject, { subject = it; mail.act("edit", "subject" to it) }, field, label = { Text("Subject") }, singleLine = true)
            OutlinedTextField(body, { body = it; mail.act("edit", "body" to it) }, field.padding(bottom = 12.dp), minLines = 10)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddAccountScreen(mail: Mail, snack: SnackbarHostState) {
    var email by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    var servers by remember { mutableStateOf(false) }
    var imap by remember { mutableStateOf("") }
    var smtp by remember { mutableStateOf("") }
    val busy = mail.view.adding
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { TextButton(onClick = { mail.act("back") }) { Text("‹") } },
                title = { Text("Add account") },
            )
        },
        snackbarHost = { Snacks(snack) },
    ) { pad ->
        Column(Modifier.fillMaxSize().padding(pad).imePadding().verticalScroll(rememberScrollState()).padding(16.dp)) {
            val field = Modifier.fillMaxWidth().padding(vertical = 4.dp)
            // Addresses and server names as typed: no capitals or corrections from the keyboard
            // (a keyboard turns "imap" into "IMAP").
            val plain = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false)
            OutlinedTextField(email, { email = it }, field, label = { Text("Email address") }, singleLine = true, keyboardOptions = plain.copy(keyboardType = KeyboardType.Email))
            OutlinedTextField(
                password, { password = it }, field, label = { Text("Password") }, singleLine = true,
                visualTransformation = PasswordVisualTransformation(), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            )
            OutlinedTextField(name, { name = it }, field, label = { Text("Your name (optional)") }, singleLine = true)
            TextButton(onClick = { servers = !servers }) { Text(if (servers) "Look up the servers instead" else "Type the servers") }
            if (servers) {
                OutlinedTextField(imap, { imap = it }, field, label = { Text("Incoming (IMAP) host:port") }, placeholder = { Text("mail.example.com:993") }, singleLine = true, keyboardOptions = plain.copy(keyboardType = KeyboardType.Uri))
                OutlinedTextField(smtp, { smtp = it }, field, label = { Text("Outgoing (SMTP) host:port") }, placeholder = { Text("mail.example.com:465") }, singleLine = true, keyboardOptions = plain.copy(keyboardType = KeyboardType.Uri))
            }
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth().padding(vertical = 8.dp))
            Button(
                onClick = {
                    mail.act(
                        "addAccount", "email" to email.trim(), "password" to password,
                        "name" to name.trim().ifEmpty { null },
                        "imap" to imap.trim().takeIf { servers && it.isNotEmpty() },
                        "smtp" to smtp.trim().takeIf { servers && it.isNotEmpty() },
                    )
                },
                enabled = !busy && email.isNotBlank() && password.isNotEmpty(),
                modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
            ) {
                Text(
                    when (mail.view.addingStep) {
                        "lookingUp" -> "Looking up the servers…"
                        "checking" -> "Checking the sign-in…"
                        else -> "Add"
                    }
                )
            }
            // The servers refused the sign-in (said above it): the person may still add it.
            if (mail.view.canAddAnyway) {
                TextButton(onClick = { mail.act("addAnyway") }, modifier = Modifier.fillMaxWidth()) { Text("Add anyway") }
            }
        }
    }
}
