package com.chitik.telegram

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import com.chitik.telegram.nativebridge.OutgoingTextSendUi
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.launch
import java.util.Locale
import kotlin.math.roundToInt

data class ConversationPollOptionUi(
    val optionId: List<Int>,
    val text: String,
    val voterCount: Int?,
    val chosen: Boolean,
    val correct: Boolean?,
)

data class ConversationPollUi(
    val id: Long,
    val chatId: Long,
    val messageId: Long,
    val question: String,
    val options: List<ConversationPollOptionUi>,
    val totalVoters: Int?,
    val publicVoters: Boolean,
    val multipleChoice: Boolean,
    val quiz: Boolean,
    val closed: Boolean,
    val explanation: String?,
)
/**
 * Display-ready message data supplied by the Rust-backed state layer.
 *
 * [timestamp] is formatted by the caller for the user's locale and timezone.
 * [id] should be the stable Telegram message identifier scoped to [chatId].
 */
data class ConversationGeoPointUi(
    val latitude: Double,
    val longitude: Double,
    val accuracyRadiusMeters: Int?,
)

sealed interface ConversationLocationUi {
    val point: ConversationGeoPointUi?

    data class Point(
        override val point: ConversationGeoPointUi,
    ) : ConversationLocationUi

    data class Venue(
        override val point: ConversationGeoPointUi?,
        val title: String,
        val address: String,
        val provider: String,
        val venueId: String,
        val venueType: String?,
    ) : ConversationLocationUi
}

data class ConversationMessageUi(
    val id: String,
    val chatId: String,
    val text: String,
    val timestamp: String,
    val dateSeparator: String = "",
    val isOutgoing: Boolean,
    val attachments: List<MediaAttachmentUi> = emptyList(),
    val isRead: Boolean = false,
    val isUnread: Boolean = false,
    val poll: ConversationPollUi? = null,
    val pollVoting: Boolean = false,
    val pollError: String? = null,
    val reactions: List<ConversationReactionUi> = emptyList(),
    val reactionRevision: ReactionRevisionUi? = null,
    val forwardedFrom: String? = null,
    val serviceAction: ServiceActionUi? = null,
    val location: ConversationLocationUi? = null,
    val senderId: Long = 0L,
    val replyToMessageId: String? = null,
    val replyToSummary: String? = null,
)

data class ReactionRevisionUi(val generation: Long, val epoch: Long, val sequence: Long) : Comparable<ReactionRevisionUi> {
    override fun compareTo(other: ReactionRevisionUi): Int =
        compareValuesBy(this, other, ReactionRevisionUi::generation, ReactionRevisionUi::epoch, ReactionRevisionUi::sequence)
}

data class ConversationReactionUi(
    val type: String,
    val value: String,
    val count: Int,
    val chosenByMe: Boolean,
) {
    val display: String get() = if (type == "emoji") value else "✨"
}

data class ConversationScreenState(
    val chatId: String,
    val chatTitle: String,
    val messages: List<ConversationMessageUi>,
    val draft: String,
    val activityStatus: String? = null,
    val loading: Boolean = false,
    val sending: Boolean = false,
    val sendStatus: String? = null,
    val outgoingMediaSend: OutgoingTextSendUi? = null,
    val voiceRecordingChatId: Long? = null,
    val voiceRecordingDurationMs: Long = 0L,
    val voiceRecordingError: String? = null,
    val error: String? = null,
    val pendingReactionKeys: Set<String> = emptySet(),
    val reactionErrors: Map<String, String> = emptyMap(),
    val forwardingError: String? = null,
    val forwardSuccessCount: Long = 0,
    val selectionGeneration: Long = 0,
    val deletingMessages: Boolean = false,
    val deleteError: String? = null,
    val capabilities: ChatCapabilitiesUi = ChatCapabilitiesUi(),
    val pinnedMessageIds: List<String> = emptyList(),
    val pendingPinnedMessageIds: Set<String> = emptySet(),
    val pinnedError: String? = null,
    val historyPageLoadingBeforeMessageId: Long? = null,
    val historyPageCompletedBeforeMessageId: Long? = null,
    val historyPageError: String? = null,
    val messageMutationPending: Boolean = false,
    val messageMutationError: String? = null,
    val messageMutationChatId: Long = 0L,
    val messageMutationMessageId: Long = 0L,
    val messageMutationSuccessCount: Long = 0L,
    val messageMutationSuccessChatId: Long = 0L,
    val messageMutationSuccessMessageId: Long = 0L,
    val replyingToMessageId: String? = null,
    val replyingToPreview: String? = null,
    val jumpToMessageId: String? = null,
)

internal fun selectedForwardMessageIds(messages: List<ConversationMessageUi>, selectedIds: Set<Long>): List<Long> =
    messages.mapNotNull { it.id.toLongOrNull() }.filter { it in selectedIds }.distinct()

internal fun replyTargetIndex(messages: List<ConversationMessageUi>, messageId: String): Int =
    messages.indexOfFirst { it.id == messageId }

internal data class ConversationScrollDecision(
    val scrollToLatest: Boolean,
    /** Null means the current new-message affordance state should be preserved. */
    val showNewMessages: Boolean?,
)

internal fun decideConversationScroll(
    hasOpenedConversation: Boolean,
    isNearLatest: Boolean,
    latestMessageChanged: Boolean,
): ConversationScrollDecision = when {
    !hasOpenedConversation || isNearLatest ->
        ConversationScrollDecision(scrollToLatest = true, showNewMessages = false)
    latestMessageChanged ->
        ConversationScrollDecision(scrollToLatest = false, showNewMessages = true)
    else -> ConversationScrollDecision(scrollToLatest = false, showNewMessages = null)
}

internal fun isLatestConversationMessageVisible(latestMessageIndex: Int, visibleItemIndices: Set<Int>): Boolean =
    latestMessageIndex < 0 || latestMessageIndex in visibleItemIndices

internal fun newMessagesAfterAnchor(messageIds: List<String>, previousLatestMessageId: String?): Int {
    if (messageIds.isEmpty() || previousLatestMessageId == null) return 0
    val anchorIndex = messageIds.indexOfLast { it == previousLatestMessageId }
    return if (anchorIndex >= 0) messageIds.lastIndex - anchorIndex else 1
}

internal fun shouldShowPinnedTargetFallback(
    targetMessageId: String?,
    loadedMessageIds: List<String>,
    loadingBeforeMessageId: Long?,
    completedBeforeMessageId: Long?,
): Boolean {
    val target = targetMessageId?.toLongOrNull()?.takeIf { it > 0 && it < Long.MAX_VALUE } ?: return false
    if (targetMessageId in loadedMessageIds) return false
    val requestedBefore = target + 1L
    return loadingBeforeMessageId != requestedBefore && completedBeforeMessageId == requestedBefore
}

internal fun selectedMessageText(messages: List<ConversationMessageUi>, selectedIds: Set<Long>): String =
    messages.filter { message -> message.id.toLongOrNull()?.let { it in selectedIds } == true }
        .map { it.text.trim() }
        .filter(String::isNotEmpty)
        .joinToString("\n")

internal fun toggleSelectedMessageId(selectedIds: Set<Long>, messageId: Long): Set<Long> =
    if (messageId in selectedIds) selectedIds - messageId else selectedIds + messageId

internal data class PrunedMessageSelection(
    val selectedIds: Set<Long>,
    val shouldExitSelectionMode: Boolean,
)

internal fun pruneSelectedMessageIds(
    selectedIds: Set<Long>,
    visibleMessageIds: Set<Long>,
): PrunedMessageSelection {
    val remaining = selectedIds.intersect(visibleMessageIds)
    return PrunedMessageSelection(
        selectedIds = remaining,
        shouldExitSelectionMode = selectedIds.isNotEmpty() && remaining.isEmpty(),
    )
}

/**
 * Stateless conversation UI. Message history and poll results always come from the Rust-backed
 * state. Only an unsubmitted multiple-choice selection is kept locally.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ConversationScreen(
    state: ConversationScreenState,
    onBack: () -> Unit,
    onDraftChange: (String) -> Unit,
    onSend: (chatId: String, text: String) -> Unit,
    onPollVote: (poll: ConversationPollUi, optionIds: List<List<Int>>) -> Unit,
    onReaction: (chatId: String, messageId: String, reaction: ConversationReactionUi?) -> Unit,
    onSendLocation: (chatId: String) -> Unit,
    onOpenLocation: (ConversationLocationUi) -> Unit,
    onForward: (chatId: Long, messageIds: List<Long>) -> Unit,
    onDelete: (chatId: Long, messageIds: List<Long>) -> Unit,
    onCopy: (text: String) -> Unit,
    onEditMessage: (chatId: Long, messageId: Long, text: String) -> Unit,
    onDeleteMessageForEveryone: (chatId: Long, messageId: Long) -> Unit,
    onDeleteMessageForMe: (chatId: Long, messageId: Long) -> Unit,
    onTogglePin: (chatId: String, messageId: String, isPinned: Boolean) -> Unit = { _, _, _ -> },
    onJumpToPinned: (messageId: String) -> Unit = {},
    onReplyMessage: (ConversationMessageUi) -> Unit = {},
    onOpenReplyTarget: (String) -> Unit = {},
    onCancelReply: () -> Unit = {},
    onPickPhotoOrVideo: (() -> Unit)? = null,
    onPickDocument: (() -> Unit)? = null,
    onRetryMediaSend: (Long) -> Unit = {},
    onCancelMediaSend: (Long) -> Unit = {},
    onJumpComplete: () -> Unit = {},
    onRetry: () -> Unit,
    onOpenProfile: () -> Unit,
    onDownloadMedia: (messageId: Long, attachmentIndex: Int, retry: Boolean) -> Unit,
    onCancelMedia: (messageId: Long, attachmentIndex: Int) -> Unit,
    onOpenDocument: (MediaAttachmentUi) -> Unit,
    onStartVoiceRecording: (chatId: Long) -> Unit = {},
    onStopVoiceRecording: (chatId: Long) -> Unit = {},
    onCancelVoiceRecording: (chatId: Long) -> Unit = {},
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val voicePlaybackCoordinator = remember(state.chatId) {
        VoicePlaybackCoordinator.create(context.applicationContext)
    }
    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(voicePlaybackCoordinator, lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_STOP -> voicePlaybackCoordinator.onHostStop()
                Lifecycle.Event.ON_START -> voicePlaybackCoordinator.onHostStart()
                else -> Unit
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
            voicePlaybackCoordinator.release()
        }
    }

    val listState = rememberLazyListState()
    val listScope = rememberCoroutineScope()
    var hasOpenedConversation by rememberSaveable(state.chatId) { mutableStateOf(false) }
    var showNewMessages by rememberSaveable(state.chatId) { mutableStateOf(false) }
    var pendingNewMessageCount by rememberSaveable(state.chatId) { mutableStateOf(0) }
    var lastObservedLatestMessageId by rememberSaveable(state.chatId) { mutableStateOf<String?>(null) }
    var viewerKey by rememberSaveable(state.chatId) { mutableStateOf<String?>(null) }
    var missingPinnedTarget by remember(state.chatId, state.selectionGeneration) { mutableStateOf<String?>(null) }
    var actionTarget by remember(state.chatId, state.selectionGeneration) { mutableStateOf<ConversationMessageUi?>(null) }
    var editingMessage by remember(state.chatId, state.selectionGeneration) { mutableStateOf<ConversationMessageUi?>(null) }
    var editDraft by remember(state.chatId, state.selectionGeneration) { mutableStateOf("") }
    var deletionTarget by remember(state.chatId, state.selectionGeneration) { mutableStateOf<ConversationMessageUi?>(null) }
    var selectedMessageIds by remember(state.chatId, state.selectionGeneration) { mutableStateOf<Set<Long>>(emptySet()) }
    var isSelectingMessages by remember(state.chatId, state.selectionGeneration) { mutableStateOf(false) }
    val selectionActive = isSelectingMessages || selectedMessageIds.isNotEmpty()
    var selectionMenuOpen by remember(state.chatId, state.selectionGeneration) { mutableStateOf(false) }
    var showDeleteConfirmation by remember(state.chatId, state.selectionGeneration) { mutableStateOf(false) }
    LaunchedEffect(state.messages, state.selectionGeneration) {
        if (selectedMessageIds.isNotEmpty()) {
            val currentIds = state.messages.mapNotNull { it.id.toLongOrNull() }.toSet()
            val pruned = pruneSelectedMessageIds(selectedMessageIds, currentIds)
            selectedMessageIds = pruned.selectedIds
            if (pruned.shouldExitSelectionMode) isSelectingMessages = false
        }
    }
    // Only a server-confirmed, fully successful forward ends selection mode.
    LaunchedEffect(state.forwardSuccessCount) {
        if (state.forwardSuccessCount > 0L) {
            selectedMessageIds = emptySet()
            isSelectingMessages = false
        }
    }
    LaunchedEffect(state.messageMutationSuccessCount) {
        if (
            state.messageMutationSuccessCount > 0L &&
            state.messageMutationSuccessChatId == state.chatId.toLongOrNull() &&
            (editingMessage?.id?.toLongOrNull() ?: deletionTarget?.id?.toLongOrNull()) == state.messageMutationSuccessMessageId
        ) {
            editingMessage = null
            deletionTarget = null
            editDraft = ""
        }
    }
    val contentMode = resolveAsyncContentMode(
        isLoading = state.loading,
        isEmpty = state.messages.isEmpty(),
        hasError = state.error != null,
    )
    val latestMessageId = state.messages.lastOrNull()?.id
    val unreadBoundaryIndex = remember(state.messages) {
        firstUnreadIncomingMessageIndex(state.messages)
    }
    val submitMessage = remember(state.chatId, state.draft, state.sending, state.capabilities, onSend) {
        {
            val text = state.draft.trim()
            if (text.isNotEmpty() && !state.sending && state.capabilities.canSendNow == true) {
                onSend(state.chatId, text)
            }
        }
    }
    val selectedMessages = state.messages.filter { message ->
        message.id.toLongOrNull()?.let { it in selectedMessageIds } == true
    }
    val orderedSelectedMessageIds = selectedForwardMessageIds(state.messages, selectedMessageIds)
    val selectionToolbarActions = messageSelectionToolbarActions(
        selectedMessageCount = selectedMessageIds.size,
        selectedMessages = selectedMessages,
        canForwardMessages = state.capabilities.canForwardMessages,
        canDeleteMessages = state.capabilities.canDeleteMessages,
        deletingMessages = state.deletingMessages,
    )

    LaunchedEffect(
        state.jumpToMessageId,
        state.messages,
        state.historyPageLoadingBeforeMessageId,
        state.historyPageCompletedBeforeMessageId,
        state.historyPageError,
    ) {
        val target = state.jumpToMessageId ?: return@LaunchedEffect
        val index = replyTargetIndex(state.messages, target)
        if (index >= 0) {
            listState.animateScrollToItem(index)
            missingPinnedTarget = null
            onJumpComplete()
        } else if (shouldShowPinnedTargetFallback(
                target,
                state.messages.map { it.id },
                state.historyPageLoadingBeforeMessageId,
                state.historyPageCompletedBeforeMessageId,
            )) {
            missingPinnedTarget = target
            onJumpComplete()
        }
    }

    LaunchedEffect(state.messages.size, latestMessageId, state.loading) {
        if (state.messages.isEmpty()) return@LaunchedEffect
        if (state.jumpToMessageId != null) {
            hasOpenedConversation = true
            lastObservedLatestMessageId = latestMessageId
            return@LaunchedEffect
        }

        val totalItems = listState.layoutInfo.totalItemsCount
        val lastVisibleIndex = listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: -1
        val isNearLatest = totalItems == 0 || lastVisibleIndex >= totalItems - 2
        val latestChanged = latestMessageId != null && latestMessageId != lastObservedLatestMessageId
        val decision = decideConversationScroll(
            hasOpenedConversation = hasOpenedConversation,
            isNearLatest = isNearLatest,
            latestMessageChanged = latestChanged,
        )

        if (decision.scrollToLatest) {
            listState.animateScrollToItem(state.messages.lastIndex)
            pendingNewMessageCount = 0
        } else if (decision.showNewMessages == true) {
            pendingNewMessageCount += newMessagesAfterAnchor(
                messageIds = state.messages.map { it.id },
                previousLatestMessageId = lastObservedLatestMessageId,
            ).coerceAtLeast(1)
        }
        decision.showNewMessages?.let { showNewMessages = it }
        hasOpenedConversation = true
        lastObservedLatestMessageId = latestMessageId
    }

    LaunchedEffect(listState, showNewMessages, state.messages.size) {
        snapshotFlow {
            val latestMessageIndex = state.messages.lastIndex
            isLatestConversationMessageVisible(
                latestMessageIndex = latestMessageIndex,
                visibleItemIndices = listState.layoutInfo.visibleItemsInfo.map { item -> item.index }.toSet(),
            )
        }.collect { isAtLatest ->
            if (isAtLatest) {
                showNewMessages = false
                pendingNewMessageCount = 0
            }
        }
    }

    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = {
                    Row(
                        modifier = Modifier.clickable(onClick = onOpenProfile),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        if (!selectionActive) AvatarPlaceholder(
                            state.chatTitle.ifBlank { "Диалог" },
                            size = 40.dp,
                        )
                        Column {
                            Text(
                                text = if (selectionActive) {
                                    "${selectedMessageIds.size} выбрано"
                                } else {
                                    state.chatTitle.ifBlank { "Диалог" }
                                },
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.SemiBold,
                            )
                            if (state.loading) {
                                Text(
                                    text = "Обновление…",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                            state.activityStatus?.let { status ->
                                Text(
                                    text = status,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.primary,
                                )
                            }
                        }
                    }
                },
                navigationIcon = {
                    TextButton(onClick = {
                        if (selectionActive) {
                            selectedMessageIds = emptySet()
                            isSelectingMessages = false
                        } else onBack()
                    }) {
                        Text("←", style = MaterialTheme.typography.titleLarge)
                    }
                },
                actions = {
                    if (selectionActive) {
                        TextButton(
                            onClick = {
                                if (state.capabilities.canForwardMessages == true) {
                                    state.chatId.toLongOrNull()?.let { onForward(it, orderedSelectedMessageIds) }
                                }
                            },
                            enabled = selectionToolbarActions.canForward && orderedSelectedMessageIds.isNotEmpty(),
                        ) { Text("Переслать") }
                        TextButton(
                            onClick = { showDeleteConfirmation = true },
                            enabled = selectionToolbarActions.canDelete,
                        ) { Text("Удалить") }
                        Box {
                            TextButton(onClick = { selectionMenuOpen = true }) { Text("⋮") }
                            DropdownMenu(
                                expanded = selectionMenuOpen,
                                onDismissRequest = { selectionMenuOpen = false },
                            ) {
                                DropdownMenuItem(
                                    text = { Text("Копировать") },
                                    enabled = selectionToolbarActions.canCopy,
                                    onClick = {
                                        onCopy(selectedMessageText(state.messages, selectedMessageIds))
                                        selectedMessageIds = emptySet()
                                        isSelectingMessages = false
                                        selectionMenuOpen = false
                                    },
                                )
                                DropdownMenuItem(
                                    text = { Text("Снять выделение") },
                                    onClick = {
                                        selectedMessageIds = emptySet()
                                        isSelectingMessages = false
                                        selectionMenuOpen = false
                                    },
                                )
                            }
                        }
                    } else {
                        TextButton(onClick = { isSelectingMessages = true }) {
                            Text("Выбрать")
                        }
                    }
                },
            )
        },
        bottomBar = {
            if (selectionActive) {
                TextButton(onClick = {
                    selectedMessageIds = emptySet()
                    isSelectingMessages = false
                }) { Text("Отменить выбор") }
            } else if (editingMessage != null) {
                val target = editingMessage
                val matchingMutation = target != null &&
                    state.messageMutationChatId == target.chatId.toLongOrNull() &&
                    state.messageMutationMessageId == target.id.toLongOrNull()
                Column {
                    if (matchingMutation) {
                        state.messageMutationError?.let { error ->
                            Text(error, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(horizontal = 12.dp))
                        }
                    }
                    ComposerBar(
                        draft = editDraft,
                        sending = matchingMutation && state.messageMutationPending,
                        canSend = true,
                        canSendMedia = false,
                        onDraftChange = { editDraft = it },
                        onSend = {
                            val chatId = target?.chatId?.toLongOrNull()
                            val messageId = target?.id?.toLongOrNull()
                            if (chatId != null && messageId != null) onEditMessage(chatId, messageId, editDraft.trim())
                        },
                        sendLabel = "Сохранить",
                        placeholder = "Новый текст",
                        onCancel = {
                            editingMessage = null
                            editDraft = ""
                        },
                        showLocationAction = false,
                    )
                }
            } else {
                Column {
                    if (state.capabilities.canSendNow != true) {
                        val notice = when {
                            state.capabilities.canSendNow == null -> "Права отправки пока неизвестны. Обновите список чатов."
                            state.capabilities.slowModeEnabled == true &&
                                (state.capabilities.slowModeRemainingSeconds ?: 0L) > 0L ->
                                "Медленный режим: подождите ${state.capabilities.slowModeRemainingSeconds} с."
                            else -> "Telegram сейчас запрещает отправку в этом чате."
                        }
                        Text(
                            text = notice,
                            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    state.sendStatus?.let { status ->
                        Text(
                            text = status,
                            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                            style = MaterialTheme.typography.bodySmall,
                            color = if (state.outgoingMediaSend?.isFailure == true) {
                                MaterialTheme.colorScheme.error
                            } else {
                                MaterialTheme.colorScheme.onSurfaceVariant
                            },
                        )
                    }
                    state.outgoingMediaSend?.let { mediaSend ->
                        if (mediaSend.status == "uploading") {
                            LinearProgressIndicator(
                                progress = { mediaSend.progressPercent?.div(100f) ?: 0f },
                                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                            )
                        }
                        mediaSend.reason?.takeIf {
                            mediaSend.isFailure || mediaSend.status == "awaiting_confirmation"
                        }?.let { reason ->
                            Text(
                                text = reason,
                                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 2.dp),
                                style = MaterialTheme.typography.bodySmall,
                                color = if (mediaSend.isFailure) MaterialTheme.colorScheme.error
                                else MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        if (mediaSend.canRetry || mediaSend.canCancel) {
                            Row(
                                modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp),
                                horizontalArrangement = Arrangement.End,
                            ) {
                                if (mediaSend.canRetry) {
                                    TextButton(onClick = { onRetryMediaSend(mediaSend.clientId) }) {
                                        Text("Повторить")
                                    }
                                }
                                if (mediaSend.canCancel) {
                                    TextButton(onClick = { onCancelMediaSend(mediaSend.clientId) }) {
                                        Text("Отменить")
                                    }
                                }
                            }
                        }
                    }
                    ComposerBar(
                        draft = state.draft,
                        sending = state.sending,
                        canSend = state.capabilities.canSendNow == true,
                        canSendMedia = state.capabilities.canSendMedia == true,
                        voiceRecording = state.voiceRecordingChatId != null &&
                            state.voiceRecordingChatId == state.chatId.toLongOrNull(),
                        voiceRecordingDurationMs = state.voiceRecordingDurationMs,
                        voiceRecordingError = state.voiceRecordingError,
                        onStartVoiceRecording = {
                            state.chatId.toLongOrNull()?.let(onStartVoiceRecording)
                        },
                        onStopVoiceRecording = {
                            state.chatId.toLongOrNull()?.let(onStopVoiceRecording)
                        },
                        onCancelVoiceRecording = {
                            state.chatId.toLongOrNull()?.let(onCancelVoiceRecording)
                        },
                        onDraftChange = onDraftChange,
                        onSend = submitMessage,
                        onSendLocation = { onSendLocation(state.chatId) },
                        onPickPhotoOrVideo = onPickPhotoOrVideo,
                        onPickDocument = onPickDocument,
                        replyingToPreview = state.replyingToPreview,
                        onCancelReply = onCancelReply,
                    )
                }
            }
        },
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding),
        ) {
            if (state.pinnedMessageIds.isNotEmpty()) {
                val pinnedId = state.pinnedMessageIds.last()
                Surface(
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp),
                    shape = RoundedCornerShape(12.dp),
                    color = MaterialTheme.colorScheme.secondaryContainer,
                    contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Text(
                            text = if (state.pinnedMessageIds.size > 1) {
                                "Закреплено: #$pinnedId (+${state.pinnedMessageIds.size - 1})"
                            } else {
                                "Закреплено: #$pinnedId"
                            },
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                            style = MaterialTheme.typography.labelLarge,
                        )
                        TextButton(onClick = { onJumpToPinned(pinnedId) }) { Text("Перейти") }
                    }
                }
            }
            state.pinnedError?.let { message ->
                Text(
                    text = "Ошибка закрепления: $message",
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            missingPinnedTarget?.let { target ->
                Row(
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = state.historyPageError?.let { error ->
                            "Не удалось загрузить сообщение #$target: $error"
                        } ?: "Закреплённое сообщение #$target не найдено. Возможно, оно удалено.",
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(onClick = {
                        missingPinnedTarget = null
                        onJumpToPinned(target)
                    }) { Text("Повторить") }
                }
            }
            state.error?.takeIf {
                asyncContentErrorPlacement(contentMode) == AsyncContentErrorPlacement.Inline &&
                    state.deleteError == null
            }?.let { message ->
                ErrorRetryState(
                    title = "Не удалось загрузить сообщения",
                    message = message,
                    retryLabel = "Повторить",
                    retryEnabled = !state.loading,
                    onRetry = onRetry,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                )
            }
            state.forwardingError?.let { message ->
                Text(
                    text = "Ошибка пересылки: $message",
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                )
            }
            state.deleteError?.let { message ->
                Column(modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp)) {
                    Text(
                        text = "Ошибка удаления: $message",
                        color = MaterialTheme.colorScheme.error,
                    )
                    TextButton(onClick = onRetry, enabled = !state.loading) {
                        Text("Обновить сообщения")
                    }
                }
            }

            when (contentMode) {
                AsyncContentMode.Loading -> LoadingState(
                    message = "Загрузка сообщений…",
                    modifier = Modifier.fillMaxSize(),
                )

                AsyncContentMode.Empty -> EmptyState(
                    title = "Сообщений пока нет",
                    modifier = Modifier.fillMaxSize().padding(24.dp),
                )

                AsyncContentMode.Error -> state.error?.takeIf { state.deleteError == null }?.let { message ->
                    Box(
                        modifier = Modifier.fillMaxSize().weight(1f).padding(24.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        ErrorRetryState(
                            title = "Не удалось загрузить сообщения",
                            message = message,
                            retryLabel = "Повторить",
                            retryEnabled = !state.loading,
                            onRetry = onRetry,
                        )
                    }
                }

                AsyncContentMode.Content -> {
                    Box(modifier = Modifier.fillMaxSize().weight(1f)) {
                        LazyColumn(
                            state = listState,
                            modifier = Modifier.fillMaxSize(),
                            contentPadding = PaddingValues(horizontal = 12.dp, vertical = 16.dp),
                            verticalArrangement = Arrangement.spacedBy(0.dp),
                        ) {
                            itemsIndexed(
                                items = state.messages,
                                key = { _, message -> "${message.chatId}:${message.id}" },
                            ) { index, message ->
                                val groupPosition = conversationMessageGroupPosition(state.messages, index)
                                val previousDate = state.messages.getOrNull(index - 1)?.dateSeparator
                                val showsDateSeparator = message.dateSeparator.isNotBlank() && message.dateSeparator != previousDate
                                Column(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .padding(
                                            top = when {
                                                !groupPosition.firstInGroup -> 2.dp
                                                showsDateSeparator -> 0.dp
                                                else -> 8.dp
                                            },
                                        ),
                                ) {
                                    if (showsDateSeparator) {
                                        ConversationDateSeparator(message.dateSeparator)
                                    }
                                    if (index == unreadBoundaryIndex) {
                                        ConversationUnreadSeparator()
                                    }
                                    MessageBubble(
                                        message = message,
                                        groupPosition = groupPosition,
                                        voicePlaybackCoordinator = voicePlaybackCoordinator,
                                        onPollVote = onPollVote,
                                        onOpenLocation = onOpenLocation,
                                        onDownloadMedia = { attachmentIndex, retry ->
                                            message.id.toLongOrNull()?.let { messageId ->
                                                onDownloadMedia(messageId, attachmentIndex, retry)
                                            }
                                        },
                                        onCancelMedia = { attachmentIndex ->
                                            message.id.toLongOrNull()?.let { messageId ->
                                                onCancelMedia(messageId, attachmentIndex)
                                            }
                                        },
                                        onOpenViewer = { attachment ->
                                            viewerKey = mediaViewerKey(state.chatId, message.id, attachment.index)
                                        },
                                        onOpenDocument = onOpenDocument,
                                        onOpenReplyTarget = onOpenReplyTarget,
                                        reactionPending = "${message.chatId}:${message.id}" in state.pendingReactionKeys,
                                        reactionError = state.reactionErrors["${message.chatId}:${message.id}"],
                                        onReaction = { reaction -> onReaction(message.chatId, message.id, reaction) },
                                        selectionMode = selectionActive,
                                        selected = message.id.toLongOrNull()?.let { it in selectedMessageIds } == true,
                                        onClick = {
                                            message.id.toLongOrNull()?.let { id ->
                                                if (selectionActive) {
                                                    selectedMessageIds = toggleSelectedMessageId(selectedMessageIds, id)
                                                }
                                            }
                                        },
                                        onLongPress = {
                                            val messageId = message.id.toLongOrNull()
                                            if (selectionActive && messageId != null) {
                                                selectedMessageIds = toggleSelectedMessageId(selectedMessageIds, messageId)
                                            } else {
                                                actionTarget = message
                                            }
                                        },
                                    )
                                }
                            }

                            if (state.loading) {
                                item(key = "history-loading") {
                                    Row(
                                        modifier = Modifier
                                            .fillMaxWidth()
                                            .padding(vertical = 8.dp),
                                        horizontalArrangement = Arrangement.Center,
                                    ) {
                                        CircularProgressIndicator(
                                            modifier = Modifier.width(20.dp),
                                            strokeWidth = 2.dp,
                                        )
                                    }
                                }
                            }
                        }
                        if (showNewMessages) {
                            Button(
                                onClick = {
                                    listScope.launch {
                                        state.messages.lastIndex.takeIf { it >= 0 }?.let { index ->
                                            listState.animateScrollToItem(index)
                                        }
                                    }
                                    showNewMessages = false
                                    pendingNewMessageCount = 0
                                },
                                modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 12.dp),
                            ) {
                                Text("Новые сообщения${pendingNewMessageCount.takeIf { it > 0 }?.let { " · $it" }.orEmpty()} ↓")
                            }
                        }
                    }
                }
            }
        }
    }

    val viewerSelection = remember(state.chatId, state.messages, viewerKey) {
        resolveMediaViewerSelection(state.chatId, state.messages, viewerKey)
    }

    actionTarget?.let { message ->
        MessageActionDialog(
            message = message,
            mutationPending = state.messageMutationPending,
            canPin = state.capabilities.canPinMessages == true,
            pinned = message.id in state.pinnedMessageIds,
            pinPending = message.id in state.pendingPinnedMessageIds,
            onDismiss = { actionTarget = null },
            onReply = {
                onReplyMessage(message)
                actionTarget = null
            },
            onSelectForForward = {
                message.id.toLongOrNull()?.let { selectedMessageIds = selectedMessageIds + it }
                actionTarget = null
            },
            onEdit = {
                editingMessage = message
                editDraft = message.text
                actionTarget = null
            },
            onDelete = {
                deletionTarget = message
                actionTarget = null
            },
            onTogglePin = {
                onTogglePin(message.chatId, message.id, message.id in state.pinnedMessageIds)
                actionTarget = null
            },
            onCopy = { text ->
                onCopy(text)
                actionTarget = null
            },
        )
    }

    deletionTarget?.let { target ->
        val targetChatId = target.chatId.toLongOrNull()
        val targetMessageId = target.id.toLongOrNull()
        val matchingMutation = targetChatId != null && targetMessageId != null &&
            state.messageMutationChatId == targetChatId &&
            state.messageMutationMessageId == targetMessageId
        val pending = matchingMutation && state.messageMutationPending
        val mutationPending = state.messageMutationPending
        AlertDialog(
            onDismissRequest = { if (!mutationPending) deletionTarget = null },
            title = { Text("Удалить сообщение?") },
            text = {
                Column {
                    Text(
                        if (supportsDeleteForMe(targetChatId ?: 0L)) {
                            "Выберите область удаления. Удаление только для вас удалит сообщение из этого аккаунта."
                        } else {
                            "Удаление только для вас недоступно в каналах; можно удалить сообщение у всех, если Telegram разрешит это действие."
                        },
                    )
                    if (matchingMutation) {
                        state.messageMutationError?.let { error ->
                            Text(error, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(top = 8.dp))
                        }
                    }
                }
            },
            confirmButton = {
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    TextButton(
                        enabled = !mutationPending && targetChatId != null && targetMessageId != null &&
                            supportsDeleteForMe(targetChatId),
                        onClick = {
                            if (targetChatId != null && targetMessageId != null && supportsDeleteForMe(targetChatId)) {
                                onDeleteMessageForMe(targetChatId, targetMessageId)
                            }
                        },
                    ) { Text(if (pending) "Удаляем…" else "Удалить у меня") }
                    TextButton(
                        enabled = !mutationPending && targetChatId != null && targetMessageId != null,
                        onClick = {
                            if (targetChatId != null && targetMessageId != null) {
                                onDeleteMessageForEveryone(targetChatId, targetMessageId)
                            }
                        },
                    ) { Text(if (pending) "Удаляем…" else "Удалить у всех") }
                }
            },
            dismissButton = {
                TextButton(enabled = !pending, onClick = { deletionTarget = null }) { Text("Отмена") }
            },
        )
    }

    viewerSelection?.let { selection ->
        MediaViewerDialog(
            attachment = selection.current.attachment,
            onDismiss = { viewerKey = null },
            onRetry = {
                onDownloadMedia(
                    selection.current.messageId,
                    selection.current.attachment.index,
                    true,
                )
                viewerKey = null
            },
            itemPosition = selection.index + 1,
            itemCount = selection.items.size,
            onPrevious = selection.previous?.let { previous ->
                {
                    viewerKey = mediaViewerKey(
                        state.chatId,
                        previous.messageId.toString(),
                        previous.attachment.index,
                    )
                }
            },
            onNext = selection.next?.let { next ->
                {
                    viewerKey = mediaViewerKey(
                        state.chatId,
                        next.messageId.toString(),
                        next.attachment.index,
                    )
                }
            },
        )
    }

    if (showDeleteConfirmation) {
        val selectedIds = selectedForwardMessageIds(state.messages, selectedMessageIds)
        AlertDialog(
            onDismissRequest = { showDeleteConfirmation = false },
            title = { Text("Удалить выбранные сообщения?") },
            text = { Text("Будет удалено сообщений: ${selectedIds.size}") },
            confirmButton = {
                TextButton(
                    enabled = selectedIds.isNotEmpty() && !state.deletingMessages,
                    onClick = {
                        state.chatId.toLongOrNull()?.let { onDelete(it, selectedIds) }
                        showDeleteConfirmation = false
                    },
                ) { Text("Удалить") }
            },
            dismissButton = {
                TextButton(onClick = { showDeleteConfirmation = false }) { Text("Отмена") }
            },
        )
    }
}

@Composable
private fun ConversationDateSeparator(label: String) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
        horizontalArrangement = Arrangement.Center,
    ) {
        Surface(
            shape = RoundedCornerShape(14.dp),
            color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.92f),
            contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
        ) {
            Text(
                text = label,
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 5.dp),
                style = MaterialTheme.typography.labelMedium,
                textAlign = TextAlign.Center,
            )
        }
    }
}

@Composable
private fun ConversationUnreadSeparator() {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
        horizontalArrangement = Arrangement.Center,
    ) {
        Surface(
            shape = RoundedCornerShape(14.dp),
            color = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
        ) {
            Text(
                text = "Непрочитанные сообщения",
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 5.dp),
                style = MaterialTheme.typography.labelMedium,
                textAlign = TextAlign.Center,
            )
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun MessageBubble(
    message: ConversationMessageUi,
    groupPosition: ConversationMessageGroupPosition,
    voicePlaybackCoordinator: VoicePlaybackCoordinator,
    onPollVote: (ConversationPollUi, List<List<Int>>) -> Unit,
    onOpenLocation: (ConversationLocationUi) -> Unit,
    onDownloadMedia: (attachmentIndex: Int, retry: Boolean) -> Unit,
    onCancelMedia: (attachmentIndex: Int) -> Unit,
    onOpenViewer: (MediaAttachmentUi) -> Unit,
    onOpenDocument: (MediaAttachmentUi) -> Unit,
    onOpenReplyTarget: (String) -> Unit,
    reactionPending: Boolean,
    reactionError: String?,
    onReaction: (ConversationReactionUi?) -> Unit,
    selectionMode: Boolean,
    selected: Boolean,
    onClick: () -> Unit,
    onLongPress: () -> Unit,
) {
    val selectedForSemantics = selected
    message.serviceAction?.let { action ->
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center) {
            Surface(
                color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.72f),
                shape = RoundedCornerShape(14.dp),
            ) {
                Text(
                    text = "•  ${action.displayText()}  •",
                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 8.dp),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        return
    }
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = if (message.isOutgoing) Arrangement.End else Arrangement.Start,
    ) {
        Surface(
            modifier = Modifier
                .fillMaxWidth(0.86f)
                .combinedClickable(
                    onClickLabel = messageClickAccessibilityLabel(selectionMode),
                    onLongClickLabel = messageContextActionLabel(selectionMode),
                    onClick = onClick,
                    onLongClick = onLongPress,
                )
                .semantics {
                    contentDescription = messageBubbleAccessibilityLabel(message)
                    if (selectionMode) this.selected = selectedForSemantics
                },
            shape = RoundedCornerShape(
                topStart = if (!message.isOutgoing && !groupPosition.firstInGroup) TelegramUiTokens.messageGroupCorner else TelegramUiTokens.messageCorner,
                topEnd = if (message.isOutgoing && !groupPosition.firstInGroup) TelegramUiTokens.messageGroupCorner else TelegramUiTokens.messageCorner,
                bottomEnd = if (message.isOutgoing && groupPosition.lastInGroup) TelegramUiTokens.messageTailCorner else TelegramUiTokens.messageCorner,
                bottomStart = if (!message.isOutgoing && groupPosition.lastInGroup) TelegramUiTokens.messageTailCorner else TelegramUiTokens.messageCorner,
            ),
            color = if (selected) MaterialTheme.colorScheme.secondaryContainer else if (message.isOutgoing) {
                MaterialTheme.colorScheme.primaryContainer
            } else {
                MaterialTheme.colorScheme.surface
            },
            contentColor = if (message.isOutgoing) {
                MaterialTheme.colorScheme.onPrimaryContainer
            } else {
                MaterialTheme.colorScheme.onSurfaceVariant
            },
        ) {
            Column(modifier = Modifier.padding(horizontal = 11.dp, vertical = 7.dp)) {
                message.replyToMessageId?.let { targetId ->
                    TextButton(onClick = { onOpenReplyTarget(targetId) }, contentPadding = PaddingValues(0.dp)) {
                        Text(message.replyToSummary ?: "Сообщение $targetId недоступно",
                            maxLines = 2, overflow = TextOverflow.Ellipsis,
                            style = MaterialTheme.typography.labelMedium)
                    }
                }
                message.location?.let { location ->
                    LocationMessageCard(location = location, onClick = { onOpenLocation(location) })
                    Spacer(Modifier.height(8.dp))
                }
                message.attachments.forEach { attachment ->
                    MediaAttachmentCard(
                        attachment = attachment,
                        voicePlaybackCoordinator = voicePlaybackCoordinator,
                        onDownload = { retry -> onDownloadMedia(attachment.index, retry) },
                        onCancel = { onCancelMedia(attachment.index) },
                        onOpenViewer = { onOpenViewer(attachment) },
                        onOpenDocument = { onOpenDocument(attachment) },
                    )
                    Spacer(Modifier.height(8.dp))
                }
                message.forwardedFrom?.let { source ->
                    Text(
                        text = "Переслано от $source",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (message.text.isNotBlank()) {
                    Text(
                        text = message.text,
                        style = MaterialTheme.typography.bodyLarge,
                    )
                }
                message.poll?.let { poll ->
                    if (message.text.isNotBlank()) {
                        Spacer(Modifier.height(8.dp))
                    }
                    PollContent(
                        poll = poll,
                        loading = message.pollVoting,
                        error = message.pollError,
                        onVote = { optionIds -> onPollVote(poll, optionIds) },
                    )
                }
                ReactionRow(message.reactions, reactionPending, reactionError, onReaction)
                if (message.timestamp.isNotBlank()) {
                    Spacer(Modifier.height(2.dp))
                    Row(
                        modifier = Modifier.align(Alignment.End),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = message.timestamp,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        if (message.isOutgoing) {
                            Spacer(Modifier.width(4.dp))
                            Text(
                                text = if (message.isRead) "✓✓" else "✓",
                                style = MaterialTheme.typography.labelSmall,
                                color = if (message.isRead) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            }
        }
    }
}


@Composable
private fun MessageActionDialog(
    message: ConversationMessageUi,
    mutationPending: Boolean,
    canPin: Boolean,
    pinned: Boolean,
    pinPending: Boolean,
    onDismiss: () -> Unit,
    onReply: () -> Unit,
    onSelectForForward: () -> Unit,
    onEdit: () -> Unit,
    onDelete: () -> Unit,
    onTogglePin: () -> Unit,
    onCopy: (String) -> Unit,
) {
    val pinAction = messagePinActionPresentation(
        message = message,
        canPin = canPin,
        isPinned = pinned,
        isPending = pinPending,
        mutationPending = mutationPending,
    )
    val copyText = messageContextCopyText(message)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Действия с сообщением") },
        text = {
            Column {
                TextButton(onClick = onReply, enabled = !mutationPending) { Text("Ответить") }
                copyText?.let { text ->
                    TextButton(onClick = { onCopy(text) }, enabled = !mutationPending) { Text("Копировать") }
                }
                TextButton(onClick = onSelectForForward, enabled = !mutationPending) { Text("Выбрать для пересылки") }
                pinAction.label?.let { label ->
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        if (pinAction.pending) {
                            CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
                        }
                        TextButton(onClick = onTogglePin, enabled = pinAction.enabled) { Text(label) }
                    }
                }
                TextButton(onClick = onEdit, enabled = !mutationPending && canEditMessageText(message)) { Text("Изменить") }
                TextButton(onClick = onDelete, enabled = !mutationPending) { Text("Удалить…") }
                Text("«Удалить у меня» работает только в личных и обычных группах; каналы требуют удаления у всех.")
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Закрыть") } },
    )
}

@Composable
private fun ReactionRow(
    reactions: List<ConversationReactionUi>,
    pending: Boolean,
    error: String?,
    onReaction: (ConversationReactionUi?) -> Unit,
) {
    var pickerOpen by remember { mutableStateOf(false) }
    val common = listOf("👍", "❤️", "😂", "😮", "😢", "🔥")

    Row(
        modifier = Modifier.fillMaxWidth().padding(top = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        if (reactions.isNotEmpty()) {
            Row(
                modifier = Modifier.weight(1f).horizontalScroll(rememberScrollState()),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                reactions.forEach { reaction ->
                    Surface(
                        modifier = Modifier
                            .clickable(enabled = !pending) {
                                onReaction(if (reaction.chosenByMe) null else reaction)
                            }
                            .semantics {
                                contentDescription = reactionChipAccessibilityLabel(reaction)
                            },
                        shape = RoundedCornerShape(16.dp),
                        color = if (reaction.chosenByMe) MaterialTheme.colorScheme.secondaryContainer
                        else MaterialTheme.colorScheme.surface,
                        contentColor = MaterialTheme.colorScheme.onSurface,
                        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                    ) {
                        Text(
                            text = "${reaction.display} ${reaction.count}",
                            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
                            style = MaterialTheme.typography.labelMedium,
                        )
                    }
                }
            }
        }
        TextButton(
            onClick = { pickerOpen = true },
            enabled = !pending,
            modifier = Modifier.semantics {
                contentDescription = reactionPickerAccessibilityLabel
            },
        ) { Text("+") }
        DropdownMenu(expanded = pickerOpen, onDismissRequest = { pickerOpen = false }) {
            common.forEach { emoji ->
                DropdownMenuItem(
                    text = { Text(emoji) },
                    onClick = {
                        pickerOpen = false
                        onReaction(ConversationReactionUi("emoji", emoji, 0, false))
                    },
                )
            }
        }
        if (pending) {
            CircularProgressIndicator(modifier = Modifier.width(14.dp), strokeWidth = 2.dp)
        }
    }
    if (error != null) {
        Text(error, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.labelSmall)
    }
}

@Composable
private fun PollContent(
    poll: ConversationPollUi,
    loading: Boolean,
    error: String?,
    onVote: (List<List<Int>>) -> Unit,
) {
    val serverChosen = remember(poll.options) {
        poll.options.filter { it.chosen }.map { it.optionId }.toSet()
    }
    var selected by remember(poll.id, serverChosen) { mutableStateOf(serverChosen) }
    val enabled = !poll.closed && !loading
    val resultsVisible = poll.totalVoters != null ||
        poll.options.any { it.voterCount != null || it.chosen || it.correct != null }

    Text(
        text = poll.question,
        style = MaterialTheme.typography.titleMedium,
        fontWeight = FontWeight.SemiBold,
    )
    Spacer(Modifier.height(6.dp))

    poll.options.forEach { option ->
        val checked = option.optionId in selected
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(enabled = enabled) {
                    selected = selectPollOption(selected, option.optionId, poll.multipleChoice)
                },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (poll.multipleChoice) {
                Checkbox(
                    checked = checked,
                    enabled = enabled,
                    onCheckedChange = null,
                )
            } else {
                RadioButton(
                    selected = checked,
                    enabled = enabled,
                    onClick = null,
                )
            }

            Column(modifier = Modifier.weight(1f)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = option.text,
                        modifier = Modifier.weight(1f),
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    optionStatus(option)?.let { status ->
                        Text(
                            text = status,
                            style = MaterialTheme.typography.labelMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                    }
                }
                if (resultsVisible && option.voterCount != null) {
                    val percent = pollPercent(option.voterCount, poll.totalVoters)
                    Text(
                        text = buildString {
                            if (percent != null) append("$percent% · ")
                            append(option.voterCount)
                            append(if (option.voterCount == 1) " голос" else " голосов")
                        },
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }

    if (!poll.closed) {
        Spacer(Modifier.height(4.dp))
        Button(
            onClick = { onVote(selected.toList()) },
            enabled = enabled && selected.isNotEmpty(),
            modifier = Modifier.fillMaxWidth(),
        ) {
            if (loading) {
                CircularProgressIndicator(
                    modifier = Modifier.width(18.dp),
                    strokeWidth = 2.dp,
                )
            } else {
                Text(if (poll.quiz) "Отправить ответ" else "Голосовать")
            }
        }
    } else if (loading) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.Center,
        ) {
            CircularProgressIndicator(
                modifier = Modifier.width(18.dp),
                strokeWidth = 2.dp,
            )
        }
    }

    if (poll.totalVoters != null) {
        Spacer(Modifier.height(4.dp))
        Text(
            text = "Всего проголосовало: ${poll.totalVoters}",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    if (poll.closed) {
        Text(
            text = "Опрос закрыт",
            style = MaterialTheme.typography.labelSmall,
            fontWeight = FontWeight.SemiBold,
        )
    }
    poll.explanation?.takeIf { it.isNotBlank() }?.let { explanation ->
        Spacer(Modifier.height(4.dp))
        Text(
            text = explanation,
            style = MaterialTheme.typography.bodySmall,
        )
    }
    error?.takeIf { it.isNotBlank() }?.let { message ->
        Spacer(Modifier.height(4.dp))
        Text(
            text = message,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.error,
        )
    }
}

private fun optionStatus(option: ConversationPollOptionUi): String? = when {
    option.correct == true -> "✓"
    option.chosen && option.correct == false -> "✕"
    option.chosen -> "Выбрано"
    else -> null
}

internal fun selectPollOption(
    selected: Set<List<Int>>,
    optionId: List<Int>,
    multipleChoice: Boolean,
): Set<List<Int>> = if (!multipleChoice) {
    setOf(optionId)
} else if (optionId in selected) {
    selected.filterNot { it == optionId }.toSet()
} else {
    selected + setOf(optionId)
}

internal fun pollPercent(voterCount: Int?, totalVoters: Int?): Int? {
    val votes = voterCount ?: return null
    val total = totalVoters ?: return null
    if (votes < 0 || total <= 0) return null
    return ((votes.toDouble() / total.toDouble()) * 100.0)
        .roundToInt()
        .coerceIn(0, 100)
}

@Composable
private fun MediaAttachmentCard(
    attachment: MediaAttachmentUi,
    voicePlaybackCoordinator: VoicePlaybackCoordinator,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
    onOpenViewer: () -> Unit,
    onOpenDocument: () -> Unit,
) {
    when (attachment.kind) {
        MediaKindUi.Photo -> PhotoAttachmentCard(
            attachment = attachment,
            onDownload = onDownload,
            onCancel = onCancel,
            onOpen = onOpenViewer,
        )

        MediaKindUi.Document -> if (isInlineImageMedia(attachment)) {
            PhotoAttachmentCard(
                attachment = attachment,
                onDownload = onDownload,
                onCancel = onCancel,
                onOpen = onOpenViewer,
            )
        } else {
            DocumentAttachmentCard(
                attachment = attachment,
                onDownload = onDownload,
                onCancel = onCancel,
                onOpen = onOpenDocument,
            )
        }

        MediaKindUi.Video,
        MediaKindUi.VideoNote,
        -> VideoAttachmentCard(
            attachment = attachment,
            onDownload = onDownload,
            onCancel = onCancel,
            onOpen = onOpenViewer,
        )

        MediaKindUi.Voice -> VoiceAttachmentCard(
            attachment = attachment,
            coordinator = voicePlaybackCoordinator,
            onDownload = onDownload,
            onCancelDownload = onCancel,
        )

        else -> DocumentAttachmentCard(
            attachment = attachment,
            onDownload = onDownload,
            onCancel = onCancel,
            onOpen = onOpenDocument,
        )
    }
}

@Composable
private fun PhotoAttachmentCard(
    attachment: MediaAttachmentUi,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
    onOpen: () -> Unit,
) {
    val file = attachment.downloadedFile
    var decodeFailed by remember(file?.absolutePath) { mutableStateOf(false) }
    val ratio = attachment.aspectRatio?.coerceIn(0.25f, 4f) ?: (4f / 3f)

    Box(
        modifier = Modifier
            .fillMaxWidth()
            .aspectRatio(ratio)
            .heightIn(min = 120.dp, max = 360.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(MaterialTheme.colorScheme.surface.copy(alpha = 0.35f))
            .clickable(enabled = file != null && !decodeFailed, onClick = onOpen),
        contentAlignment = Alignment.Center,
    ) {
        if (file != null && !decodeFailed) {
            AsyncImage(
                model = file,
                contentDescription = mediaAttachmentOpenAccessibilityLabel(attachment),
                contentScale = ContentScale.Crop,
                onSuccess = { decodeFailed = false },
                onError = { decodeFailed = true },
                modifier = Modifier
                    .fillMaxSize()
                    .background(MaterialTheme.colorScheme.surfaceVariant),
            )
        } else {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                Text(if (decodeFailed) "Изображение повреждено" else "Фото")
            }
        }

        MediaTransferOverlay(
            attachment = attachment,
            localFileMissing = attachment.downloadState is MediaDownloadUiState.Downloaded && file == null,
            localDecodeFailed = decodeFailed,
            onDownload = onDownload,
            onCancel = onCancel,
        )
    }
}

@Composable
private fun VideoAttachmentCard(
    attachment: MediaAttachmentUi,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
    onOpen: () -> Unit,
) {
    val file = attachment.downloadedFile
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(180.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(Color.Black.copy(alpha = 0.72f))
            .clickable(enabled = file != null, onClick = onOpen),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text(
                text = if (file != null) "▶" else "Видео",
                style = MaterialTheme.typography.headlineMedium,
                color = Color.White,
            )
            formatMediaDuration(attachment.durationMs)?.let {
                Text(
                    text = it,
                    style = MaterialTheme.typography.labelMedium,
                    color = Color.White,
                )
            }
        }

        MediaTransferOverlay(
            attachment = attachment,
            localFileMissing = attachment.downloadState is MediaDownloadUiState.Downloaded && file == null,
            onDownload = onDownload,
            onCancel = onCancel,
        )
    }
}

@Composable
private fun DocumentAttachmentCard(
    attachment: MediaAttachmentUi,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
    onOpen: () -> Unit,
) {
    val file = attachment.downloadedFile
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = file != null, onClick = onOpen),
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.surface.copy(alpha = 0.38f),
    ) {
        Column(
            modifier = Modifier.padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text(
                text = sanitizeDisplayFilename(attachment.filename),
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.SemiBold,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            val details = listOfNotNull(
                formatMediaSize(attachment.sizeBytes),
                attachment.mimeType?.takeIf { it.isNotBlank() },
                formatMediaDuration(attachment.durationMs),
            ).joinToString(" • ")
            if (details.isNotBlank()) {
                Text(
                    text = details,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (file != null) {
                Text(
                    text = "Нажмите, чтобы открыть",
                    style = MaterialTheme.typography.labelMedium,
                )
            }
            MediaTransferControls(
                attachment = attachment,
                localFileMissing = attachment.downloadState is MediaDownloadUiState.Downloaded && file == null,
                onDownload = onDownload,
                onCancel = onCancel,
            )
        }
    }
}

@Composable
private fun MediaTransferOverlay(
    attachment: MediaAttachmentUi,
    localFileMissing: Boolean,
    localDecodeFailed: Boolean = false,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
) {
    val showOverlay = attachment.downloadState !is MediaDownloadUiState.Downloaded ||
        localFileMissing ||
        localDecodeFailed
    if (!showOverlay) return

    Surface(
        modifier = Modifier.fillMaxSize(),
        color = Color.Black.copy(alpha = 0.56f),
    ) {
        Box(contentAlignment = Alignment.Center) {
            MediaTransferControls(
                attachment = attachment,
                localFileMissing = localFileMissing,
                localDecodeFailed = localDecodeFailed,
                onDownload = onDownload,
                onCancel = onCancel,
                lightContent = true,
            )
        }
    }
}

@Composable
private fun MediaTransferControls(
    attachment: MediaAttachmentUi,
    localFileMissing: Boolean,
    localDecodeFailed: Boolean = false,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
    lightContent: Boolean = false,
) {
    val textColor = if (lightContent) Color.White else MaterialTheme.colorScheme.onSurface
    Column(
        modifier = Modifier.padding(10.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        when (val download = attachment.downloadState) {
            MediaDownloadUiState.NotDownloaded -> {
                Button(onClick = { onDownload(false) }) {
                    Text("Скачать")
                }
            }

            MediaDownloadUiState.Queued -> {
                CircularProgressIndicator(modifier = Modifier.width(28.dp))
                Text("В очереди", color = textColor)
                TextButton(onClick = onCancel) { Text("Отмена") }
            }

            is MediaDownloadUiState.Downloading -> {
                val total = download.totalBytes
                val fraction = total
                    ?.takeIf { it > 0L }
                    ?.let { (download.bytesTransferred.toDouble() / it).coerceIn(0.0, 1.0).toFloat() }
                if (fraction != null) {
                    LinearProgressIndicator(
                        progress = { fraction },
                        modifier = Modifier.fillMaxWidth(0.72f),
                    )
                    Text("${(fraction * 100).toInt()}%", color = textColor)
                } else {
                    CircularProgressIndicator(modifier = Modifier.width(28.dp))
                    Text(
                        formatMediaSize(download.bytesTransferred) ?: "Загрузка…",
                        color = textColor,
                    )
                }
                TextButton(onClick = onCancel) { Text("Отмена") }
            }

            is MediaDownloadUiState.Failed -> {
                Text(download.reason ?: "Не удалось скачать", color = textColor)
                Button(onClick = { onDownload(true) }) {
                    Text("Повторить")
                }
            }

            MediaDownloadUiState.Cancelled -> {
                Text("Загрузка отменена", color = textColor)
                Button(onClick = { onDownload(true) }) {
                    Text("Повторить")
                }
            }

            is MediaDownloadUiState.Downloaded -> {
                if (localFileMissing || localDecodeFailed) {
                    Text(
                        if (localDecodeFailed) "Файл не удалось прочитать" else "Файл удалён из кэша",
                        color = textColor,
                    )
                    Button(onClick = { onDownload(localDecodeFailed) }) {
                        Text("Скачать заново")
                    }
                }
            }
        }
    }
}

@Composable
private fun LocationMessageCard(
    location: ConversationLocationUi,
    onClick: () -> Unit,
) {
    val point = location.point
    val title = when (location) {
        is ConversationLocationUi.Point -> "Location"
        is ConversationLocationUi.Venue -> location.title
    }
    val subtitle = when (location) {
        is ConversationLocationUi.Point -> formatCoordinates(location.point)
        is ConversationLocationUi.Venue -> location.address
    }
    val accuracy = point?.accuracyRadiusMeters?.let { "Accuracy ±$it m" }

    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = point != null, onClick = onClick),
        shape = RoundedCornerShape(14.dp),
        tonalElevation = 2.dp,
    ) {
        Row(
            modifier = Modifier.padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text("📍", style = MaterialTheme.typography.headlineSmall)
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                )
                if (subtitle.isNotBlank()) {
                    Text(
                        text = subtitle,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (location is ConversationLocationUi.Venue && point != null) {
                    Text(
                        text = formatCoordinates(point),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                accuracy?.let {
                    Text(
                        text = it,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

private fun formatCoordinates(point: ConversationGeoPointUi): String =
    String.format(Locale.US, "%.5f, %.5f", point.latitude, point.longitude)

@Composable
private fun ComposerBar(
    draft: String,
    sending: Boolean,
    canSend: Boolean,
    canSendMedia: Boolean,
    voiceRecording: Boolean = false,
    voiceRecordingDurationMs: Long = 0L,
    voiceRecordingError: String? = null,
    onStartVoiceRecording: (() -> Unit)? = null,
    onStopVoiceRecording: (() -> Unit)? = null,
    onCancelVoiceRecording: (() -> Unit)? = null,
    onDraftChange: (String) -> Unit,
    onSend: () -> Unit,
    sendLabel: String = "Отправить",
    placeholder: String = "Сообщение",
    onCancel: (() -> Unit)? = null,
    onSendLocation: () -> Unit = {},
    onPickPhotoOrVideo: (() -> Unit)? = null,
    onPickDocument: (() -> Unit)? = null,
    showLocationAction: Boolean = true,
    replyingToPreview: String? = null,
    onCancelReply: () -> Unit = {},
) {
    val canSubmit = draft.isNotBlank() && !sending && canSend && !voiceRecording
    var attachmentMenuExpanded by remember { mutableStateOf(false) }
    val showAttachmentAction = canSendMedia &&
        (onPickPhotoOrVideo != null || onPickDocument != null)

    Surface(
        tonalElevation = 2.dp,
        modifier = Modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .imePadding(),
    ) {
        Column {
            HorizontalDivider()
            voiceRecordingError?.let { error ->
                Text(
                    text = error,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            replyingToPreview?.let { preview ->
                Row(
                    modifier = Modifier.fillMaxWidth().padding(start = 12.dp, end = 4.dp, top = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(preview, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.labelMedium)
                    TextButton(onClick = onCancelReply) { Text("Отмена") }
                }
            }
            if (voiceRecording) {
                Row(
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(
                        text = "●",
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.titleMedium,
                    )
                    Text(
                        text = formatVoiceRecordingDuration(voiceRecordingDurationMs),
                        modifier = Modifier.weight(1f),
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.Medium,
                    )
                    TextButton(onClick = { onCancelVoiceRecording?.invoke() }) {
                        Text("Отмена")
                    }
                    Button(
                        onClick = { onStopVoiceRecording?.invoke() },
                        enabled = !sending,
                        shape = CircleShape,
                        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 10.dp),
                    ) {
                        Text("Отправить")
                    }
                }
            } else {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 8.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.Bottom,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    if (showLocationAction) {
                        TextButton(
                            onClick = onSendLocation,
                            modifier = Modifier.semantics {
                                contentDescription = composerLocationActionAccessibilityLabel()
                            },
                            enabled = !sending && canSend && canSendMedia,
                            contentPadding = PaddingValues(horizontal = 8.dp, vertical = 12.dp),
                        ) {
                            Text("📍")
                        }
                    }
                    if (showAttachmentAction) {
                        Box {
                            TextButton(
                                onClick = { attachmentMenuExpanded = true },
                                modifier = Modifier.semantics {
                                    contentDescription = composerAttachmentActionAccessibilityLabel(
                                        hasPhotoVideoPicker = onPickPhotoOrVideo != null,
                                        hasDocumentPicker = onPickDocument != null,
                                    )
                                },
                                enabled = !sending && canSend,
                                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 12.dp),
                            ) {
                                Text("📎")
                            }
                            DropdownMenu(
                                expanded = attachmentMenuExpanded,
                                onDismissRequest = { attachmentMenuExpanded = false },
                            ) {
                                onPickPhotoOrVideo?.let { pick ->
                                    DropdownMenuItem(
                                        text = { Text("Фото или видео") },
                                        onClick = {
                                            attachmentMenuExpanded = false
                                            pick()
                                        },
                                    )
                                }
                                onPickDocument?.let { pick ->
                                    DropdownMenuItem(
                                        text = { Text("Файл") },
                                        onClick = {
                                            attachmentMenuExpanded = false
                                            pick()
                                        },
                                    )
                                }
                            }
                        }
                    }
                    Box(
                        modifier = Modifier
                            .weight(1f)
                            .heightIn(min = TelegramUiTokens.composerMinHeight),
                        contentAlignment = Alignment.CenterStart,
                    ) {
                        if (draft.isEmpty()) {
                            Text(
                                text = placeholder,
                                modifier = Modifier.padding(horizontal = 2.dp, vertical = 9.dp),
                                style = MaterialTheme.typography.bodyLarge.copy(
                                    fontSize = TelegramUiTokens.composerTextSize,
                                ),
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                        BasicTextField(
                            value = draft,
                            onValueChange = onDraftChange,
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(vertical = 9.dp)
                                .heightIn(min = 26.dp),
                            enabled = !sending,
                            textStyle = MaterialTheme.typography.bodyLarge.copy(
                                fontSize = TelegramUiTokens.composerTextSize,
                                color = MaterialTheme.colorScheme.onSurface,
                            ),
                            cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                            maxLines = 6,
                            keyboardOptions = KeyboardOptions(
                                keyboardType = KeyboardType.Text,
                                imeAction = ImeAction.Send,
                            ),
                            keyboardActions = KeyboardActions(
                                onSend = { if (canSubmit) onSend() },
                            ),
                        )
                    }
                    onCancel?.let { cancel -> TextButton(onClick = cancel, enabled = !sending) { Text("Отмена") } }
                    if (draft.isBlank() && canSendMedia && onStartVoiceRecording != null && sendLabel == "Отправить") {
                        androidx.compose.material3.IconButton(
                            onClick = onStartVoiceRecording,
                            enabled = !sending && canSend,
                            modifier = Modifier.size(48.dp),
                        ) {
                            androidx.compose.material3.Icon(
                                painter = painterResource(R.drawable.ic_voice_mic),
                                contentDescription = "Записать голосовое сообщение",
                            )
                        }
                    } else {
                        Button(
                            onClick = onSend,
                            enabled = canSubmit,
                            modifier = if (sendLabel == "Отправить") {
                                Modifier
                                    .size(48.dp)
                                    .semantics {
                                        contentDescription = composerSendActionAccessibilityLabel(sending)
                                    }
                            } else {
                                Modifier
                            },
                            shape = CircleShape,
                            contentPadding = PaddingValues(0.dp),
                        ) {
                            Text(if (sending) "…" else if (sendLabel == "Отправить") "➤" else sendLabel)
                        }
                    }
                }
            }
        }
    }
}
