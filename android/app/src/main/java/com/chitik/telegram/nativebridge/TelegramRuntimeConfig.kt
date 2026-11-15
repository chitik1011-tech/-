package com.chitik.telegram.nativebridge

import android.content.Context
import java.io.File
import java.util.Locale

data class TelegramRuntimeConfig internal constructor(
    val apiId: Int,
    val apiHash: String,
    val sessionDatabasePath: String,
)

object TelegramRuntimeConfigProvider {
    private const val SESSION_DIRECTORY = "telegram"
    private const val SESSION_DATABASE = "session.sqlite"
    private val apiHashPattern = Regex("^[0-9a-f]{32}$")

    fun fromUserInput(
        context: Context,
        apiIdInput: String?,
        apiHashInput: String?,
    ): TelegramRuntimeConfig {
        val apiId = apiIdInput
            ?.trim()
            ?.toIntOrNull()
            ?.takeIf { it > 0 }
            ?: throw IllegalArgumentException("Telegram API ID must be a positive integer.")

        val apiHash = apiHashInput
            ?.trim()
            ?.lowercase(Locale.ROOT)
            ?.takeIf { apiHashPattern.matches(it) }
            ?: throw IllegalArgumentException(
                "Telegram API hash must contain exactly 32 hexadecimal characters.",
            )

        val privateRoot = context.applicationContext.filesDir.canonicalFile
        val sessionDirectory = File(privateRoot, SESSION_DIRECTORY)
        if (!sessionDirectory.isDirectory &&
            !sessionDirectory.mkdirs() &&
            !sessionDirectory.isDirectory
        ) {
            throw IllegalStateException("Could not create the private Telegram session directory.")
        }

        val databaseFile = File(sessionDirectory, SESSION_DATABASE).canonicalFile
        val privatePrefix = privateRoot.path.trimEnd(File.separatorChar) + File.separator
        if (!databaseFile.path.startsWith(privatePrefix)) {
            throw IllegalStateException("Telegram session path must remain inside app-private storage.")
        }

        return TelegramRuntimeConfig(
            apiId = apiId,
            apiHash = apiHash,
            sessionDatabasePath = databaseFile.path,
        )
    }
}
