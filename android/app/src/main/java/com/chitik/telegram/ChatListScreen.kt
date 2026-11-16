package com.chitik.telegram

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.People
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Close
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/** Data projected exclusively from Rust CoreState; never inject sample chats. */
data class ChatListEntry(
    val id: Long,
    val title: String,
    val unreadCount: Long,
    val lastMessage: ChatListPreview?,
    val capabilities: ChatCapabilitiesUi = ChatCapabilitiesUi(),
)

data class ChatListPreview(
    val text: String,
    val timestampUnix: Long,
    val outgoing: Boolean,
)

/** Telegram-density conversation list retaining the existing Rust-backed callbacks. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatListScreen(
    chats: List<ChatListEntry>,
    selectedChatId: Long?,
    isLoading: Boolean,
    error: String?,
    onRefresh: () -> Unit,
    onOpenContacts: () -> Unit,
    onChatSelected: (Long) -> Unit,
    modifier: Modifier = Modifier,
) {
    val contentMode = resolveAsyncContentMode(isLoading, chats.isEmpty(), error != null)
    var searching by rememberSaveable { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }
    val visibleChats = filterLoadedChats(chats, query)
    Column(modifier = modifier.fillMaxSize().background(MaterialTheme.colorScheme.surface)) {
        TopAppBar(
            title = { Text("Telegram", fontWeight = FontWeight.SemiBold) },
            navigationIcon = {
                IconButton(onClick = onOpenContacts) {
                    Icon(Icons.Default.Menu, contentDescription = "Open contacts")
                }
            },
            actions = {
                IconButton(onClick = { searching = !searching; if (!searching) query = "" }) {
                    Icon(if (searching) Icons.Default.Close else Icons.Default.Search, contentDescription = if (searching) "Close chat search" else "Search loaded chats")
                }
                IconButton(onClick = onRefresh, enabled = !isLoading) {
                    if (isLoading) CircularProgressIndicator(modifier = Modifier.size(20.dp), strokeWidth = 2.dp)
                    else Icon(Icons.Default.Refresh, contentDescription = "Refresh conversations")
                }
                IconButton(onClick = onOpenContacts) {
                    Icon(Icons.Default.People, contentDescription = "Contacts")
                }
            },
        )
        if (searching) {
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
                placeholder = { Text("Search loaded chats") },
                singleLine = true,
                leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
            )
        }
        if (error != null && contentMode != AsyncContentMode.Loading) {
            ErrorRetryState(
                title = "Couldn’t load chats", message = error,
                retryLabel = "Try again", retryEnabled = !isLoading, onRetry = onRefresh,
            )
        }
        when (contentMode) {
            AsyncContentMode.Loading -> LoadingState(
                message = "Loading chats…",
                modifier = Modifier.weight(1f).fillMaxWidth().padding(24.dp),
            )
            AsyncContentMode.Empty -> EmptyState(
                title = "No conversations yet",
                description = "Your Telegram conversations will appear here.",
                actionLabel = "Refresh chats",
                onAction = onRefresh,
                modifier = Modifier.weight(1f).fillMaxWidth().padding(24.dp),
            )
            AsyncContentMode.Error -> Unit
            AsyncContentMode.Content -> LazyColumn(
                modifier = Modifier.weight(1f).fillMaxWidth(),
            ) {
                if (visibleChats.isEmpty()) {
                    item(key = "search_empty") {
                        Text(
                            "No matching loaded chats",
                            modifier = Modifier.padding(24.dp),
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                items(visibleChats, key = ChatListEntry::id) { chat ->
                    ChatListRow(
                        chat = chat,
                        selected = chat.id == selectedChatId,
                        onClick = { onChatSelected(chat.id) },
                    )
                    HorizontalDivider(
                        modifier = Modifier.padding(start = TelegramUiTokens.dividerInset),
                        thickness = 0.5.dp,
                        color = MaterialTheme.colorScheme.outlineVariant,
                    )
                }
            }
        }
    }
}

@Composable
private fun ChatListRow(chat: ChatListEntry, selected: Boolean, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(
                if (selected) MaterialTheme.colorScheme.secondaryContainer
                else MaterialTheme.colorScheme.surface
            )
            .clickable(onClick = onClick)
            .heightIn(min = TelegramUiTokens.chatRowHeight)
            .padding(horizontal = TelegramUiTokens.horizontalInset, vertical = 9.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        AvatarPlaceholder(chat.title, size = TelegramUiTokens.avatar, selected = selected)
        Spacer(Modifier.width(TelegramUiTokens.rowGap))
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    text = chat.title,
                    modifier = Modifier.weight(1f),
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                chat.lastMessage?.let { preview ->
                    Spacer(Modifier.width(8.dp))
                    Text(
                        text = MessageTimestampFormatter.format(preview.timestampUnix),
                        style = MaterialTheme.typography.labelSmall,
                        color = if (chat.unreadCount > 0) MaterialTheme.colorScheme.primary
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                    )
                }
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
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
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                if (chat.unreadCount > 0) {
                    Spacer(Modifier.width(8.dp))
                    UnreadCounterBadge(count = chat.unreadCount)
                }
            }
        }
    }
}

/** Search only currently loaded, real Rust-backed chats; no server query is implied. */
internal fun filterLoadedChats(chats: List<ChatListEntry>, query: String): List<ChatListEntry> {
    val needle = query.trim()
    if (needle.isEmpty()) return chats
    return chats.filter { chat ->
        chat.title.contains(needle, ignoreCase = true) ||
            chat.lastMessage?.text?.contains(needle, ignoreCase = true) == true
    }
}
