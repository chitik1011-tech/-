package com.chitik.telegram

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Badge
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/**
 * UI projection of a Rust ChatSummary and its optional last MessageSummary.
 *
 * The parent should construct these values from CoreState/ChatsLoaded; this
 * component deliberately has no built-in sample or placeholder chats.
 */
data class ChatListEntry(
    val id: Long,
    val title: String,
    val unreadCount: Long,
    val lastMessage: ChatListPreview?,
)

data class ChatListPreview(
    val text: String,
    val timestampUnix: Long,
    val outgoing: Boolean,
)

/**
 * Displays real chats supplied by the Rust-backed state layer.
 *
 * Loading, errors, and empty results are explicit inputs so that transport and
 * authorization behavior remain owned by the caller.
 */
@Composable
fun ChatListScreen(
    chats: List<ChatListEntry>,
    selectedChatId: Long?,
    isLoading: Boolean,
    error: String?,
    onRefresh: () -> Unit,
    onChatSelected: (Long) -> Unit,
    modifier: Modifier = Modifier,
) {
    val contentMode = resolveAsyncContentMode(isLoading, chats.isEmpty(), error != null)

    Column(
        modifier = modifier.fillMaxSize(),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 20.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = "Chats",
                    style = MaterialTheme.typography.headlineSmall,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    text = when {
                        isLoading && chats.isEmpty() -> "Loading conversations…"
                        chats.isEmpty() -> "Your conversations"
                        chats.size == 1 -> "1 conversation"
                        else -> "${chats.size} conversations"
                    },
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.width(12.dp))
            OutlinedButton(
                onClick = onRefresh,
                enabled = !isLoading,
            ) {
                if (isLoading) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(16.dp),
                        strokeWidth = 2.dp,
                    )
                    Spacer(Modifier.width(8.dp))
                }
                Text(if (isLoading) "Loading" else "Refresh")
            }
        }

        if (error != null && contentMode != AsyncContentMode.Loading) {
            ErrorRetryState(
                title = "Couldn’t load chats",
                message = error,
                retryLabel = "Try again",
                retryEnabled = !isLoading,
                onRetry = onRefresh,
            )
        }

        when (contentMode) {
            AsyncContentMode.Loading -> {
                LoadingState(
                    message = "Loading chats…",
                    modifier = Modifier.weight(1f).fillMaxWidth().padding(24.dp),
                )
            }

            AsyncContentMode.Empty -> {
                EmptyState(
                    title = "No conversations yet",
                    description = "When Telegram returns your chats, they’ll appear here.",
                    actionLabel = "Refresh chats",
                    onAction = onRefresh,
                    modifier = Modifier.weight(1f).fillMaxWidth().padding(24.dp),
                )
            }

            AsyncContentMode.Error -> Unit

            AsyncContentMode.Content -> {
                LazyColumn(
                    modifier = Modifier.weight(1f).fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    items(chats, key = ChatListEntry::id) { chat ->
                        ChatListRow(
                            chat = chat,
                            selected = chat.id == selectedChatId,
                            onClick = { onChatSelected(chat.id) },
                            modifier = Modifier.padding(horizontal = 12.dp),
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun ChatListRow(
    chat: ChatListEntry,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Card(
        onClick = onClick,
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(18.dp),
        colors = CardDefaults.cardColors(
            containerColor = if (selected) {
                MaterialTheme.colorScheme.secondaryContainer
            } else {
                MaterialTheme.colorScheme.surfaceContainerLow
            },
        ),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 14.dp, vertical = 13.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            AvatarPlaceholder(
                displayName = chat.title,
                selected = selected,
                modifier = Modifier.size(48.dp),
            )

            Spacer(Modifier.width(14.dp))

            Column(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = chat.title,
                        modifier = Modifier.weight(1f),
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = if (chat.unreadCount > 0) FontWeight.SemiBold else FontWeight.Medium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    chat.lastMessage?.let { preview ->
                        Spacer(Modifier.width(8.dp))
                        Text(
                            text = MessageTimestampFormatter.format(preview.timestampUnix),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    val preview = chat.lastMessage
                    Text(
                        text = when {
                            preview == null -> ""
                            preview.outgoing -> "You: ${preview.text}"
                            else -> preview.text
                        },
                        modifier = Modifier.weight(1f),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    if (chat.unreadCount > 0) {
                        Spacer(Modifier.width(8.dp))
                        Badge {
                            Text(
                                text = if (chat.unreadCount > 99) "99+" else chat.unreadCount.toString(),
                                modifier = Modifier.padding(horizontal = 3.dp),
                            )
                        }
                    }
                }
            }
        }
    }
}
