package io.github.zidan44.mailcraft

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import org.json.JSONObject
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The passwords the Rust side asks to keep (its `secret` and `forget` messages), sealed with an
 * AES key that lives in the Android Keystore and never leaves it. What is stored in the app's
 * preferences is only the sealed text; a value that no longer opens (the key was reset) is
 * dropped, and MailCraft asks for that password again.
 */
class Secrets(context: Context) {
    private val prefs = context.getSharedPreferences("secrets", Context.MODE_PRIVATE)

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build()
        )
        return gen.generateKey()
    }

    fun put(entry: String, value: String) {
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.ENCRYPT_MODE, key())
        c.updateAAD(entry.toByteArray())
        val sealed = c.iv + c.doFinal(value.toByteArray())
        prefs.edit().putString(entry, Base64.encodeToString(sealed, Base64.NO_WRAP)).apply()
    }

    fun remove(entry: String) {
        prefs.edit().remove(entry).apply()
    }

    /** Every kept password, as the Rust side's `start` takes them: `{"entry": "password"}`. */
    fun all(): String {
        val out = JSONObject()
        val k = key()
        for ((entry, v) in prefs.all) {
            val bytes = (v as? String)?.let { runCatching { Base64.decode(it, Base64.NO_WRAP) }.getOrNull() } ?: continue
            if (bytes.size <= IV) continue
            val opened = runCatching {
                val c = Cipher.getInstance("AES/GCM/NoPadding")
                c.init(Cipher.DECRYPT_MODE, k, GCMParameterSpec(128, bytes, 0, IV))
                c.updateAAD(entry.toByteArray())
                String(c.doFinal(bytes, IV, bytes.size - IV))
            }.getOrNull()
            if (opened == null) remove(entry) else out.put(entry, opened)
        }
        return out.toString()
    }

    private companion object {
        const val ALIAS = "mailcraft-secrets"
        const val IV = 12
    }
}
