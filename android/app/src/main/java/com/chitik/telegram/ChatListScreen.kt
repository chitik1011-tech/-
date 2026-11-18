package com.chitik.telegram

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
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
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberDrawerState
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.People
import androidx.compose.material.icons.filled.Chat
import androidx.compose.material.icons.filled.Logout
import androidx.compose.material.icons.filled.ManageSearch
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Close
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import kotlinx.coroutines.flow.collect

/** Data projected exclusively from Rust CoreState; never inject sample chats. */
data class ChatListEntry(
    val id: Long,
    val title: String,
    val unreadCount: Long,
    val lastMessage: ChatListPreview?,
    val capabilities: ChatCapabilitiesUi = ChatCapabilitiesUi(),
    val unreadMentionsCount: Long? = null,
    val unreadReactionsCount: Long? = null,
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
    onLogout: () -> Unit,
    onChatSelected: (Long) -> Unit,
    hasMoreChats: Boolean = false,
    isLoadingMoreChats: Boolean = false,
    chatPageError: String? = null,
    onLoadMoreChats: () -> Unit = {},
    onRetryChatPage: () -> Unit = {},
    onOpenMessageSearch: (() -> Unit)? = null,
    showEnableNotifications: Boolean = false,
    onEnableNotifications: () -> Unit = {},
    modifier: Modifier = Modifier,
) {
    val contentMode = resolveAsyncContentMode(isLoading, chats.isEmpty(), error != null)
    val drawerState = rememberDrawerState(initialValue = DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    var searching by rememberSaveable { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }
    var confirmLogout by rememberSaveable { mutableStateOf(false) }
    val visibleChats = filterLoadedChats(chats, query)
    val listState = rememberLazyListState()
    val latestLoadMore by rememberUpdatedState(onLoadMoreChats)
    LaunchedEffect(listState, visibleChats.size, hasMoreChats, isLoadingMoreChats, chatPageError, query) {
        if (!hasMoreChats || isLoadingMoreChats || chatPageError != null || query.isNotBlank()) {
            return@LaunchedEffect
        }
        snapshotFlow { listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: -1 }
            .collect { lastVisibleIndex ->
                if (lastVisibleIndex >= (visibleChats.size - 5).coerceAtLeast(0)) {
                    latestLoadMore()
                }
            }
    }
    ModalNavigationDrawer(
        modifier = modifier.fillMaxSize(),
        drawerState = drawerState,
        drawerContent = {
            ModalDrawerSheet {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(MaterialTheme.colorScheme.primaryContainer)
                        .padding(horizontal = 20.dp, vertical = 24.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Text("Telegram", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
                    Text("Chats and people", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                NavigationDrawerItem(
                    label = { Text("Chats") },
                    selected = true,
                    icon = { Icon(Icons.Default.Chat, contentDescription = null) },
                    onClick = {
                        scope.launch {
                            performChatListDrawerAction(
                                action = ChatListDrawerAction.CHATS,
                                closeDrawer = { drawerState.close() },
                                onOpenContacts = onOpenContacts,
                                onRequestLogout = { confirmLogout = true },
                            )
                        }
                    },
                    modifier = Modifier.padding(horizontal = 12.dp),
                )
                NavigationDrawerItem(
                    label = { Text("Contacts") },
                    selected = false,
                    icon = { Icon(Icons.Default.People, contentDescription = null) },
                    onClick = {
                        scope.launch {
                            performChatListDrawerAction(
                                action = ChatListDrawerAction.CONTACTS,
                                closeDrawer = { drawerState.close() },
                                onOpenContacts = onOpenContacts,
                                onRequestLogout = { confirmLogout = true },
                            )
                        }
                    },
                    modifier = Modifier.padding(horizontal = 12.dp),
                )
                Spacer(Modifier.weight(1f))
                NavigationDrawerItem(
                    label = { Text("Log out") },
                    selected = false,
                    icon = { Icon(Icons.Default.Logout, contentDescription = null) },
                    onClick = {
                        scope.launch {
                            performChatListDrawerAction(
                                action = ChatListDrawerAction.LOG_OUT,
                                closeDrawer = { drawerState.close() },
                                onOpenContacts = onOpenContacts,
                                onRequestLogout = { confirmLogout = true },
                            )
                        }
                    },
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                )
            }
        },
    ) {
        Column(modifier = Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surface)) {
            TopAppBar(
                title = { Text("Telegram", fontWeight = FontWeight.SemiBold) },
                navigationIcon = {
                    IconButton(onClick = { scope.launch { drawerState.open() } }) {
                        Icon(Icons.Default.Menu, contentDescription = "Open navigation menu")
                    }
                },
                actions = {
                    if (showEnableNotifications) {
                        TextButton(onClick = onEnableNotifications) {
                            Text("Enable alerts")
                        }
                    }
                    IconButton(onClick = { searching = !searching; if (!searching) query = "" }) {
                        Icon(if (searching) Icons.Default.Close else Icons.Default.Search, contentDescription = if (searching) "Close chat search" else "Search loaded chats")
                    }
                    chatListMessageSearchAction(onOpenMessageSearch)?.let { action ->
                        IconButton(onClick = action.onClick) {
                            Icon(Icons.Default.ManageSearch, contentDescription = "Search messages")
                        }
                    }
                    IconButton(onClick = onRefresh, enabled = !isLoading) {
                        if (isLoading) CircularProgressIndicator(modifier = Modifier.size(20.dp), strokeWidth = 2.dp)
                        else Icon(Icons.Default.Refresh, contentDescription = "Refresh conversations")
                    }
                },
            )
            if (confirmLogout) {
                AlertDialog(
                    onDismissRequest = { confirmLogout = false },
                    title = { Text("Log out of Telegram?") },
                    text = { Text("This signs out this Telegram session. You can sign in again with the same account.") },
                    confirmButton = {
                        TextButton(onClick = {
                            confirmLogout = false
                            onLogout()
                        }) { Text("Log out") }
                    },
                    dismissButton = {
                        TextButton(onClick = { confirmLogout = false }) { Text("Cancel") }
                    },
                )
            }
            if (searching) {
                val keyboardController = LocalSoftwareKeyboardController.current
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = TelegramUiTokens.chatSearchInset, vertical = 4.dp)
                        .heightIn(min = TelegramUiTokens.chatSearchHeight)
                        .background(
                            color = MaterialTheme.colorScheme.surfaceVariant,
                            shape = RoundedCornerShape(TelegramUiTokens.chatSearchCorner),
                        )
                        .padding(horizontal = TelegramUiTokens.chatSearchHorizontalInset),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    BasicTextField(
                        value = query,
                        onValueChange = { query = it },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                        textStyle = MaterialTheme.typography.bodyLarge.copy(
                            color = MaterialTheme.colorScheme.onSurface,
                        ),
                        cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                        keyboardActions = KeyboardActions(onSearch = { keyboardController?.hide() }),
                        decorationBox = { innerTextField ->
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Icon(
                                    Icons.Default.Search,
                                    contentDescription = null,
                                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                                Spacer(Modifier.width(12.dp))
                                Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                                    if (query.isEmpty()) {
                                        Text(
                                            "Search loaded chats",
                                            style = MaterialTheme.typography.bodyLarge,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            maxLines = 1,
                                        )
                                    }
                                    innerTextField()
                                }
                                if (query.isNotEmpty()) {
                                    IconButton(onClick = { query = "" }) {
                                        Icon(
                                            Icons.Default.Close,
                                            contentDescription = "Clear chat search",
                                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                }
                            }
                        },
                    )
                }
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
                    actionLabel = if (hasMoreChats && query.isBlank()) "Load more chats" else "Refresh chats",
                    onAction = if (hasMoreChats && query.isBlank()) onLoadMoreChats else onRefresh,
                    modifier = Modifier.weight(1f).fillMaxWidth().padding(24.dp),
                )
                AsyncContentMode.Error -> Unit
                AsyncContentMode.Content -> LazyColumn(
                    modifier = Modifier.weight(1f).fillMaxWidth(),
                    state = listState,
                ) {
                    if (visibleChats.isEmpty()) {
                        item(key = "search_empty") {
                            Column(
                                modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 48.dp),
                                horizontalAlignment = Alignment.CenterHorizontally,
                                verticalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                Icon(
                                    Icons.Default.Search,
                                    contentDescription = null,
                                    modifier = Modifier.size(28.dp),
                                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                                Text(
                                    "No matching loaded chats",
                                    style = MaterialTheme.typography.bodyLarge,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
                    items(visibleChats, key = ChatListEntry::id) { chat ->
                        ChatListRow(
                            chat = chat,
                            selected = chat.id == selectedChatId,
                            onClick = { onChatSelected(chat.id) },
                        )
                        HorizontalDivider(
                            modifier = Modifier.padding(start = TelegramUiTokens.chatListDividerInset),
                            thickness = 0.5.dp,
                            color = MaterialTheme.colorScheme.outlineVariant,
                        )
                    }
                    if (chatPageError != null) {
                        item(key = "chat_page_error") {
                            ErrorRetryState(
                                title = "Couldn’t load more chats",
                                message = chatPageError,
                                retryLabel = "Retry",
                                retryEnabled = !isLoadingMoreChats,
                                onRetry = onRetryChatPage,
                                modifier = Modifier.padding(16.dp),
                            )
                        }
                    } else if (hasMoreChats && isLoadingMoreChats) {
                        item(key = "chat_page_loading") {
                            Box(
                                modifier = Modifier.fillMaxWidth().padding(20.dp),
                                contentAlignment = Alignment.Center,
                            ) {
                                CircularProgressIndicator(modifier = Modifier.size(24.dp))
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ChatListRow(chat: ChatListEntry, selected: Boolean, onClick: () -> Unit) {
    val presentation = chat.toChatListRowPresentation()
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(
                if (selected) MaterialTheme.colorScheme.secondaryContainer
                else MaterialTheme.colorScheme.surface
            )
            .clickable(onClick = onClick)
            .semantics(mergeDescendants = true) {
                contentDescription = presentation.accessibilityLabel
            }
            .heightIn(min = TelegramUiTokens.chatRowHeight)
            .padding(horizontal = TelegramUiTokens.horizontalInset, vertical = TelegramUiTokens.chatRowVerticalInset),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        AvatarPlaceholder(presentation.title, size = TelegramUiTokens.avatar, selected = selected)
        Spacer(Modifier.width(TelegramUiTokens.rowGap))
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    text = presentation.title,
                    modifier = Modifier.weight(1f),
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                presentation.timestamp?.let { timestamp ->
                    Spacer(Modifier.width(8.dp))
                    Text(
                        text = timestamp,
                        style = MaterialTheme.typography.labelSmall,
                        color = if (chat.unreadCount > 0) MaterialTheme.colorScheme.primary
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                    )
                }
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (presentation.outgoingPreview) {
                    Text(
                        text = "✓",
                        color = MaterialTheme.colorScheme.primary,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    Spacer(Modifier.width(4.dp))
                }
                Text(
                    text = presentation.preview.orEmpty(),
                    modifier = Modifier.weight(1f),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (presentation.unreadLabel != null) {
                    Spacer(Modifier.width(8.dp))
                    UnreadCounterBadge(count = chat.unreadCount)
                }
                presentation.unreadMentionsLabel?.let { label ->
                    Spacer(Modifier.width(4.dp))
                    UnreadMentionBadge(label)
                }
                presentation.unreadReactionsLabel?.let { label ->
                    Spacer(Modifier.width(4.dp))
                    UnreadReactionBadge(label)
                }
            }
        }
    }
}

internal data class ChatListMessageSearchAction(val onClick: () -> Unit)

internal fun chatListMessageSearchAction(
    onOpenMessageSearch: (() -> Unit)?,
): ChatListMessageSearchAction? = onOpenMessageSearch?.let(::ChatListMessageSearchAction)

/** Search only currently loaded, real Rust-backed chats; no server query is implied. */
internal fun filterLoadedChats(chats: List<ChatListEntry>, query: String): List<ChatListEntry> {
    val needle = query.trim()
    if (needle.isEmpty()) return chats
    return chats.filter { chat ->
        chat.title.contains(needle, ignoreCase = true) ||
            chat.lastMessage?.text?.contains(needle, ignoreCase = true) == true
    }
}

/** Real destinations/actions available from the chat-list navigation drawer. */
internal enum class ChatListDrawerAction {
    CHATS,
    CONTACTS,
    LOG_OUT,
}

/** Closes the drawer before dispatching the action selected by the user. */
internal suspend fun performChatListDrawerAction(
    action: ChatListDrawerAction,
    closeDrawer: suspend () -> Unit,
    onOpenContacts: () -> Unit,
    onRequestLogout: () -> Unit,
) {
    closeDrawer()
    when (action) {
        ChatListDrawerAction.CHATS -> Unit
        ChatListDrawerAction.CONTACTS -> onOpenContacts()
        ChatListDrawerAction.LOG_OUT -> onRequestLogout()
    }
}
