package com.chitik.telegram.deeplink

/** A supported incoming link replaces the pending route; unrelated intents leave it intact. */
internal fun pendingTelegramLinkAfterNewIntent(
    current: TelegramLinkIntent?,
    incoming: TelegramLinkIntent?,
): TelegramLinkIntent? = incoming ?: current
