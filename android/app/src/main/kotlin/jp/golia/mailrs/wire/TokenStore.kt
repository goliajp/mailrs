package jp.golia.mailrs.wire

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The session token, encrypted with a key the app never sees.
 *
 * `androidx.security:security-crypto`'s `EncryptedSharedPreferences` is
 * the obvious choice and is **deprecated** as of 1.1.0 — verified by
 * disassembling the released artifact rather than recalled. So this does
 * the thing it wrapped: an AES-GCM key in the Android Keystore, and the
 * ciphertext in ordinary private preferences. The key material stays in
 * the TEE and cannot be read back, so a copied prefs file is useless.
 *
 * The manifest sets `allowBackup="false"` for the same reason: cloud
 * backup would copy the ciphertext off the device, and a token is worth
 * as much as the password that made it.
 *
 * Deleting the key (uninstall, or the user clearing app data) makes the
 * stored blob undecryptable. That reads as "not signed in", which is the
 * truthful outcome — [read] returns null rather than throwing.
 */
class TokenStore(context: Context) {

    private val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun read(): Session? {
        val server = prefs.getString(KEY_SERVER, null) ?: return null
        val blob = prefs.getString(KEY_TOKEN, null) ?: return null
        val token = decrypt(blob) ?: return null
        return Session(server, token, prefs.getString(KEY_ADDRESS, null).orEmpty())
    }

    fun write(session: Session) {
        prefs.edit()
            .putString(KEY_SERVER, session.server)
            .putString(KEY_TOKEN, encrypt(session.token))
            .putString(KEY_ADDRESS, session.address)
            .apply()
    }

    fun clear() {
        prefs.edit().clear().apply()
    }

    private fun secretKey(): SecretKey {
        val ks = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (ks.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build()
        )
        return generator.generateKey()
    }

    /** `base64(iv) : base64(ciphertext)`. The IV is not a secret. */
    private fun encrypt(plain: String): String {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, secretKey())
        val body = cipher.doFinal(plain.toByteArray())
        return b64(cipher.iv) + ":" + b64(body)
    }

    private fun decrypt(blob: String): String? {
        val parts = blob.split(":")
        if (parts.size != 2) return null
        return runCatching {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, secretKey(), GCMParameterSpec(TAG_BITS, unb64(parts[0])))
            String(cipher.doFinal(unb64(parts[1])))
        }.getOrNull()
    }

    private fun b64(bytes: ByteArray) = android.util.Base64.encodeToString(bytes, android.util.Base64.NO_WRAP)

    private fun unb64(s: String) = android.util.Base64.decode(s, android.util.Base64.NO_WRAP)

    /**
     * Where we are signed in, and as whom.
     *
     * The address is stored because the app has no other way to learn
     * it — and two things need it: Settings, to say whose mailbox this
     * is, and reply-all, which excludes yourself by comparing against
     * it. It was absent for long enough that both were quietly wrong.
     */
    data class Session(val server: String, val token: String, val address: String)

    private companion object {
        const val PREFS = "mailrs.session"
        const val KEY_SERVER = "server"
        const val KEY_TOKEN = "token"
        const val KEY_ADDRESS = "address"
        const val KEYSTORE = "AndroidKeyStore"
        const val KEY_ALIAS = "mailrs.session.key"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val TAG_BITS = 128
    }
}
