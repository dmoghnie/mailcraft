package io.github.zidan44.mailcraft

import android.content.Context
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import org.json.JSONArray
import org.json.JSONObject
import java.util.TimeZone

/** The core's view model (`crates/mobile/src/view.rs`), read from its JSON. */
data class View(
    val screen: String = "folders",
    val ready: Boolean = false,
    val accounts: List<Account> = emptyList(),
    val title: String = "",
    val messages: List<Row> = emptyList(),
    val total: Long = 0,
    val message: Message? = null,
    val compose: Compose? = null,
    val searching: String? = null,
    val adding: Boolean = false,
    val notice: String? = null,
)

data class Folder(val name: String, val path: String, val role: String, val unread: Long, val total: Long)
data class Account(val id: String, val email: String, val name: String, val state: String, val error: String?, val folders: List<Folder>)
data class Row(val key: String, val from: String, val subject: String, val preview: String, val date: Long, val unread: Boolean, val flagged: Boolean, val attachments: Boolean)
data class Message(val key: String, val from: String, val to: String, val cc: String, val subject: String, val date: Long, val text: String, val attachments: List<String>)
data class Compose(val id: Long, val from: String, val to: String, val cc: String, val subject: String, val body: String, val sending: Boolean, val error: String?)

private fun JSONObject.text(k: String): String = if (isNull(k)) "" else optString(k, "")
private fun JSONObject.textOrNull(k: String): String? = if (isNull(k)) null else optString(k).ifEmpty { null }
private fun <T> JSONArray?.each(f: (JSONObject) -> T): List<T> =
    if (this == null) emptyList() else (0 until length()).mapNotNull { optJSONObject(it)?.let(f) }

fun parseView(o: JSONObject): View = View(
    screen = o.text("screen").ifEmpty { "folders" },
    ready = o.optBoolean("ready"),
    accounts = o.optJSONArray("accounts").each { a ->
        Account(
            a.text("id"), a.text("email"), a.text("name"), a.text("state"), a.textOrNull("error"),
            a.optJSONArray("folders").each { Folder(it.text("name"), it.text("path"), it.text("role"), it.optLong("unread"), it.optLong("total")) },
        )
    },
    title = o.text("title"),
    messages = o.optJSONArray("messages").each {
        Row(it.text("key"), it.text("from"), it.text("subject"), it.text("preview"), it.optLong("date"), it.optBoolean("unread"), it.optBoolean("flagged"), it.optBoolean("attachments"))
    },
    total = o.optLong("total"),
    message = o.optJSONObject("message")?.let { m ->
        Message(
            m.text("key"), m.text("from"), m.text("to"), m.text("cc"), m.text("subject"), m.optLong("date"), m.text("text"),
            m.optJSONArray("attachments").each { it.text("name") },
        )
    },
    compose = o.optJSONObject("compose")?.let {
        Compose(it.optLong("id"), it.text("from"), it.text("to"), it.text("cc"), it.text("subject"), it.text("body"), it.optBoolean("sending"), it.textOrNull("error"))
    },
    searching = o.textOrNull("searching"),
    adding = o.optBoolean("adding"),
    notice = o.textOrNull("notice"),
)

/**
 * The app's one link to the core: starts it, sends the person's actions, and on a thread of its
 * own takes what comes back (the view to draw, passwords for the keystore).
 */
class Mail(context: Context) {
    var view by mutableStateOf(View())
        private set
    var problem by mutableStateOf<String?>(null)
        private set

    private val secrets = Secrets(context)
    private val main = Handler(Looper.getMainLooper())

    init {
        val tz = TimeZone.getDefault()
        val said = Core.start(context.filesDir.resolve("mail").path, secrets.all(), tz.getOffset(System.currentTimeMillis()) / 1000, tz.id)
        if (said.isNotEmpty()) {
            problem = said
        } else {
            Thread({ listen() }, "mailcraft-shell").apply { isDaemon = true }.start()
        }
    }

    private fun listen() {
        while (true) {
            val m = Core.poll(1000)
            if (m.isEmpty()) continue
            val o = runCatching { JSONObject(m) }.getOrNull() ?: continue
            when (o.optString("kind")) {
                "render" -> o.optJSONObject("view")?.let { v -> val parsed = parseView(v); main.post { view = parsed } }
                "secret" -> secrets.put(o.optString("entry"), o.optString("value"))
                "forget" -> secrets.remove(o.optString("entry"))
                "notice" -> o.optString("text").let { t -> main.post { problem = t } }
            }
        }
    }

    fun send(action: JSONObject) {
        val said = Core.send(action.toString())
        if (said.isNotEmpty()) problem = said
    }

    fun act(type: String, vararg fields: Pair<String, Any?>) {
        val o = JSONObject().put("type", type)
        for ((k, v) in fields) if (v != null) o.put(k, v)
        send(o)
    }

    fun timeZoneChanged() {
        val tz = TimeZone.getDefault()
        Core.zone(tz.getOffset(System.currentTimeMillis()) / 1000, tz.id)
    }

    fun dismiss() {
        problem = null
        act("dismissNotice")
    }
}
