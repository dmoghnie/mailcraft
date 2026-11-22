package io.github.zidan44.mailcraft

/**
 * The Rust library (`apps/mailcraft-mobile/src/android.rs`): five calls, JSON text both ways.
 * Each returns "" when all went well, or what went wrong; [poll] returns the next message or ""
 * when none came in time, and blocks, so it is called from a thread of its own.
 */
object Core {
    init {
        System.loadLibrary("mailcraftcore")
    }

    @JvmStatic external fun start(dir: String, secrets: String, offset: Int, zone: String): String
    @JvmStatic external fun send(action: String): String
    @JvmStatic external fun poll(timeoutMs: Long): String
    @JvmStatic external fun view(): String
    @JvmStatic external fun zone(offset: Int, zone: String): String
}
