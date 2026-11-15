package com.chitik.telegram.nativebridge

import android.content.Context
import android.content.SharedPreferences
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.nio.ByteBuffer
import java.nio.charset.StandardCharsets
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.json.JSONObject

/**
 * Stores only Telegram's API ID and API hash encrypted at rest.
 *
 * The AES key is generated in Android Keystore and is not exportable. Ciphertext
 * is held in this app's private SharedPreferences. Phone numbers, login codes,
 * 2FA passwords, session paths, and session database contents are never written
 * here. The session path is re-derived from app-private filesDir on every load.
 */
object TelegramRuntimeConfigStore {
    private const val KEYSTORE_PROVIDER = "AndroidKeyStore"
    private const val KEY_ALIAS = "telegram.runtime-config.aes.v1"
    private const val PREFERENCES_NAME = "telegram_runtime_config"
    private const val ENCRYPTED_CONFIG_KEY = "encrypted_config"
    private const val API_ID_FIELD = "api_id"
    private const val API_HASH_FIELD = "api_hash"
    private const val GCM_IV_BYTES = 12
    private const val GCM_TAG_BITS = 128
    private const val GCM_TAG_BYTES = GCM_TAG_BITS / 8
    private const val AAD = "com.chitik.telegram.runtime-config.v1"
    private const val MIN_ENCRYPTED_BYTES = GCM_IV_BYTES + GCM_TAG_BYTES + 1

    private val lock = Any()

    /**
     * Validate and save runtime API config. The returned session path is derived
     * from app-private filesDir and is not stored in preferences.
     */
    fun save(
        context: Context,
        apiIdInput: String?,
        apiHashInput: String?,
    ): TelegramRuntimeConfig = synchronized(lock) {
        val appContext = context.applicationContext
        val config = TelegramRuntimeConfigProvider.fromUserInput(
            appContext,
            apiIdInput,
            apiHashInput,
        )
        val plaintext = JSONObject()
            .put(API_ID_FIELD, config.apiId)
            .put(API_HASH_FIELD, config.apiHash)
            .toString()
            .toByteArray(StandardCharsets.UTF_8)
        val encrypted = encrypt(plaintext)

        if (!preferences(appContext).edit()
                .putString(ENCRYPTED_CONFIG_KEY, Base64.encodeToString(encrypted, Base64.NO_WRAP))
                .commit()
        ) {
            throw IllegalStateException("Could not save Telegram API configuration.")
        }
        config
    }

    /**
     * Load and validate saved API config, or return null when none has been saved.
     *
     * Throws [IllegalStateException] if the saved value cannot be decrypted or
     * validated; callers can offer [clear] and ask the user to enter it again.
     */
    fun load(context: Context): TelegramRuntimeConfig? = synchronized(lock) {
        val appContext = context.applicationContext
        val encoded = preferences(appContext).getString(ENCRYPTED_CONFIG_KEY, null)
            ?: return@synchronized null
        val encrypted = try {
            Base64.decode(encoded, Base64.NO_WRAP)
        } catch (error: IllegalArgumentException) {
            throw IllegalStateException("Saved Telegram API configuration is unreadable; clear it and enter it again.", error)
        }
        if (encrypted.size < MIN_ENCRYPTED_BYTES) {
            throw IllegalStateException("Saved Telegram API configuration is incomplete; clear it and enter it again.")
        }

        val plaintext = try {
            decrypt(encrypted)
        } catch (error: Exception) {
            throw IllegalStateException("Could not decrypt saved Telegram API configuration; clear it and enter it again.", error)
        }
        val stored = try {
            JSONObject(String(plaintext, StandardCharsets.UTF_8))
        } catch (error: Exception) {
            throw IllegalStateException("Saved Telegram API configuration is invalid; clear it and enter it again.", error)
        }

        TelegramRuntimeConfigProvider.fromUserInput(
            appContext,
            stored.optInt(API_ID_FIELD, 0).toString(),
            stored.optString(API_HASH_FIELD, ""),
        )
    }

    /** Remove the encrypted API config and its Keystore key. Does not delete the session DB. */
    fun clear(context: Context) = synchronized(lock) {
        val appContext = context.applicationContext
        if (!preferences(appContext).edit().remove(ENCRYPTED_CONFIG_KEY).commit()) {
            throw IllegalStateException("Could not clear Telegram API configuration.")
        }

        val keyStore = KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
        if (keyStore.containsAlias(KEY_ALIAS)) {
            keyStore.deleteEntry(KEY_ALIAS)
        }
    }

    private fun preferences(context: Context): SharedPreferences =
        context.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)

    private fun encrypt(plaintext: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, getOrCreateKey())
        cipher.updateAAD(AAD.toByteArray(StandardCharsets.UTF_8))
        val iv = cipher.iv
        check(iv.size == GCM_IV_BYTES) { "Android Keystore returned an unexpected AES-GCM IV size." }
        val ciphertext = cipher.doFinal(plaintext)
        return ByteBuffer.allocate(iv.size + ciphertext.size)
            .put(iv)
            .put(ciphertext)
            .array()
    }

    private fun decrypt(encrypted: ByteArray): ByteArray {
        val iv = encrypted.copyOfRange(0, GCM_IV_BYTES)
        val ciphertext = encrypted.copyOfRange(GCM_IV_BYTES, encrypted.size)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, getOrCreateKey(), GCMParameterSpec(GCM_TAG_BITS, iv))
        cipher.updateAAD(AAD.toByteArray(StandardCharsets.UTF_8))
        return cipher.doFinal(ciphertext)
    }

    private fun getOrCreateKey(): SecretKey {
        val keyStore = KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
        (keyStore.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }

        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE_PROVIDER)
        val specification = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setRandomizedEncryptionRequired(true)
            .build()
        generator.init(specification)
        return generator.generateKey()
    }
}
