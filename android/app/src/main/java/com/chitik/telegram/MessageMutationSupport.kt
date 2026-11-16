package com.chitik.telegram

/** Mutations are disabled until the canonical Rust command and Telegram handler are present. */
internal enum class MessageMutationAction {
    Edit,
    Delete,
}

internal data class MessageMutationAvailability(
    val enabled: Boolean,
    val explanation: String,
)

internal fun messageMutationAvailability(action: MessageMutationAction): MessageMutationAvailability =
    MessageMutationAvailability(
        enabled = false,
        explanation = when (action) {
            MessageMutationAction.Edit -> "Редактирование пока не поддерживается серверной частью."
            MessageMutationAction.Delete -> "Удаление пока не поддерживается серверной частью."
        },
    )
