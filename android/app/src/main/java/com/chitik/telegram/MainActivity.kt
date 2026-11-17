package com.chitik.telegram

import android.Manifest
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.viewmodel.compose.viewModel
import com.chitik.telegram.nativebridge.NativeSessionViewModel
import com.chitik.telegram.nativebridge.NativeUiState
import com.chitik.telegram.nativebridge.mediaSendForChat
import com.chitik.telegram.nativebridge.textSendStatusForChat
import com.chitik.telegram.notifications.NotificationPermissionRequester
import com.chitik.telegram.notifications.NotificationRouteResolution
import com.chitik.telegram.notifications.TelegramNotificationRoute
import com.chitik.telegram.notifications.PendingNotificationSessionDecision
import com.chitik.telegram.notifications.decidePendingNotificationSessionRestore
import com.chitik.telegram.notifications.restorePendingNotificationChatId
import com.chitik.telegram.deeplink.TelegramDeepLinkRouter
import com.chitik.telegram.deeplink.TelegramJoinInviteIntent
import com.chitik.telegram.deeplink.TelegramLinkAppRouter
import com.chitik.telegram.deeplink.TelegramLinkIntent
import com.chitik.telegram.deeplink.TelegramLinkTarget

class MainActivity : ComponentActivity() {
    private companion object {
        const val STATE_PENDING_NOTIFICATION_CHAT_ID = "pending_notification_chat_id"
    }

    private var pendingTelegramLink by mutableStateOf<TelegramLinkIntent?>(null)
    private var pendingNotificationChatId by mutableStateOf<Long?>(null)
    private var notificationPermissionGranted by mutableStateOf(true)
    private lateinit var notificationPermissionRequester: NotificationPermissionRequester

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        notificationPermissionRequester = NotificationPermissionRequester(this) { granted ->
            notificationPermissionGranted = granted
        }
        notificationPermissionGranted = notificationPermissionRequester.isGranted()
        enableEdgeToEdge()
        pendingTelegramLink = TelegramDeepLinkRouter.parseIntent(intent?.action, intent?.dataString)
        pendingNotificationChatId = restorePendingNotificationChatId(
            savedChatId = savedInstanceState
                ?.takeIf { it.containsKey(STATE_PENDING_NOTIFICATION_CHAT_ID) }
                ?.getLong(STATE_PENDING_NOTIFICATION_CHAT_ID),
            freshIntentChatId = TelegramNotificationRoute.parse(intent)?.chatId,
        )
        setContent {
            TelegramAppTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    TelegramRustApp(
                        incomingLink = pendingTelegramLink,
                        incomingNotificationChatId = pendingNotificationChatId,
                        onDismissNotification = { pendingNotificationChatId = null },
                        showEnableNotifications = !notificationPermissionGranted,
                        onEnableNotifications = { notificationPermissionRequester.request() },
                        onDismissLink = { pendingTelegramLink = null },
                    )
                }
            }
        }
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        pendingTelegramLink = TelegramDeepLinkRouter.parseIntent(intent.action, intent.dataString)
        pendingNotificationChatId = restorePendingNotificationChatId(
            savedChatId = pendingNotificationChatId,
            freshIntentChatId = TelegramNotificationRoute.parse(intent)?.chatId,
        )
    }

    override fun onSaveInstanceState(outState: Bundle) {
        pendingNotificationChatId?.let { chatId ->
            outState.putLong(STATE_PENDING_NOTIFICATION_CHAT_ID, chatId)
        }
        super.onSaveInstanceState(outState)
    }

    override fun onResume() {
        super.onResume()
        if (::notificationPermissionRequester.isInitialized) {
            notificationPermissionGranted = notificationPermissionRequester.isGranted()
        }
    }
}

@Composable
private fun TelegramRustApp(
    incomingLink: TelegramLinkIntent?,
    onDismissLink: () -> Unit,
    incomingNotificationChatId: Long?,
    onDismissNotification: () -> Unit,
    showEnableNotifications: Boolean,
    onEnableNotifications: () -> Unit,
    viewModel: NativeSessionViewModel = viewModel(),
) {
    val state by viewModel.state.collectAsState()
    val context = LocalContext.current
    val lifecycleOwner = remember(context) { context.findLifecycleOwner() }
    var selectedChatId by rememberSaveable { mutableLongStateOf(0L) }
    var showContacts by rememberSaveable { mutableStateOf(false) }
    var draft by rememberSaveable { mutableStateOf("") }
    var lastRestoredFailureId by rememberSaveable { mutableLongStateOf(0L) }
    var replyToMessageId by rememberSaveable { mutableStateOf<Long?>(null) }
    var replyToPreview by rememberSaveable { mutableStateOf<String?>(null) }
    var jumpToMessageId by rememberSaveable { mutableStateOf<String?>(null) }
    var replySelectionGeneration by rememberSaveable { mutableLongStateOf(state.selectionGeneration) }
    var pendingLocationChatId by rememberSaveable { mutableLongStateOf(0L) }
    var pendingVoiceRecordingChatId by rememberSaveable { mutableLongStateOf(0L) }
    var activeLocationRequest by remember { mutableStateOf<LocationRequestHandle?>(null) }
    val currentSelectedChatId by rememberUpdatedState(selectedChatId)
    val activeVoiceRecordingChatId by rememberUpdatedState(state.voiceRecording.chatId)
    val voiceRecordingPermissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        val chatId = pendingVoiceRecordingChatId
        pendingVoiceRecordingChatId = 0L
        voiceRecordingChatAfterPermission(granted, chatId, currentSelectedChatId)?.let {
            viewModel.startVoiceRecording(it)
        }
        if (!granted && chatId != 0L && currentSelectedChatId == chatId) {
            Toast.makeText(context, "Разрешите доступ к микрофону для записи голосового сообщения", Toast.LENGTH_LONG).show()
        }
    }
    val requestVoiceRecording: (Long) -> Unit = { chatId ->
        if (chatId != 0L && selectedChatId == chatId) {
            val permissionGranted = context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
            if (permissionGranted) {
                viewModel.startVoiceRecording(chatId)
            } else {
                pendingVoiceRecordingChatId = chatId
                voiceRecordingPermissionLauncher.launch(Manifest.permission.RECORD_AUDIO)
            }
        }
    }
    LaunchedEffect(selectedChatId) {
        pendingVoiceRecordingChatId = pendingVoicePermissionChatAfterNavigation(
            pendingChatId = pendingVoiceRecordingChatId,
            selectedChatId = selectedChatId,
        )
    }
    LaunchedEffect(selectedChatId, state.voiceRecording.chatId) {
        val recordingChatId = state.voiceRecording.chatId
        if (shouldCancelVoiceRecordingForChatChange(recordingChatId, selectedChatId)) {
            viewModel.cancelVoiceRecording(recordingChatId)
        }
    }
    LaunchedEffect(incomingNotificationChatId, state.sessionRestoreComplete, state.authorization) {
        val sessionDecision = decidePendingNotificationSessionRestore(
            restoreComplete = state.sessionRestoreComplete,
            authorizationReady = state.authorization == AuthorizationUiStep.Ready,
        )
        if (incomingNotificationChatId != null &&
            sessionDecision == PendingNotificationSessionDecision.Discard
        ) {
            onDismissNotification()
        }
    }
    LaunchedEffect(
        incomingNotificationChatId,
        state.authorization,
        state.chatsLoaded,
        state.chats,
        state.chatPagination,
    ) {
        val resolution = com.chitik.telegram.notifications.resolveNotificationRoute(
            chatId = incomingNotificationChatId,
            authorized = state.authorization == AuthorizationUiStep.Ready,
            chatsLoaded = state.chatsLoaded,
            availableChatIds = state.chats.map(ChatListEntry::id),
            chatListExhausted = state.chatPagination.exhausted && !state.chatPagination.loading,
        )
        when (resolution) {
            NotificationRouteResolution.Wait -> {
                if (state.authorization == AuthorizationUiStep.Ready && !state.chatPagination.loading) {
                    if (!state.chatPagination.loaded && state.chatPagination.error == null) {
                        viewModel.loadChats()
                    } else if (state.chatPagination.loaded && state.chatPagination.error == null) {
                        viewModel.loadNextChatsPage()
                    }
                }
            }
            is NotificationRouteResolution.OpenChat -> {
                selectedChatId = resolution.chatId
                showContacts = false
                viewModel.loadMessages(resolution.chatId)
                viewModel.loadPinnedMessages(resolution.chatId)
                onDismissNotification()
            }
            NotificationRouteResolution.Discard -> onDismissNotification()
        }
    }
    val queuePickedMedia: (Uri?) -> Unit = { uri ->
        val chatId = selectedChatId
        val caption = draft
        if (uri != null && chatId != 0L) {
            viewModel.sendMediaFromUri(chatId, uri, caption) { accepted ->
                if (accepted && selectedChatId == chatId && draft == caption) draft = ""
            }
        }
    }
    val photoPickerLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.PickVisualMedia(),
        queuePickedMedia,
    )
    val documentPickerLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
        queuePickedMedia,
    )
    DisposableEffect(lifecycleOwner) {
        if (lifecycleOwner == null) {
            onDispose { }
        } else {
            val observer = LifecycleEventObserver { _, event ->
                if (event == Lifecycle.Event.ON_STOP) {
                    activeLocationRequest?.cancel()
                    activeLocationRequest = null
                    pendingLocationChatId = 0L
                    activeVoiceRecordingChatId?.let { chatId -> viewModel.cancelVoiceRecording(chatId) }
                }
            }
            lifecycleOwner.lifecycle.addObserver(observer)
            onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
        }
    }
    DisposableEffect(lifecycleOwner, selectedChatId, viewModel) {
        if (lifecycleOwner == null) {
            viewModel.setNotificationVisibility(false, null)
            onDispose { viewModel.setNotificationVisibility(false, null) }
        } else {
            fun updateVisibility(foreground: Boolean) {
                viewModel.setNotificationVisibility(
                    foreground = foreground,
                    chatId = selectedChatId.takeIf { it != 0L },
                )
            }
            updateVisibility(lifecycleOwner.lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED))
            val observer = LifecycleEventObserver { _, event ->
                when (event) {
                    Lifecycle.Event.ON_START -> updateVisibility(true)
                    Lifecycle.Event.ON_STOP -> updateVisibility(false)
                    else -> Unit
                }
            }
            lifecycleOwner.lifecycle.addObserver(observer)
            onDispose {
                lifecycleOwner.lifecycle.removeObserver(observer)
                viewModel.setNotificationVisibility(false, null)
            }
        }
    }
    DisposableEffect(selectedChatId) {
        onDispose {
            activeLocationRequest?.cancel()
            activeLocationRequest = null
            pendingLocationChatId = 0L
        }
    }
    LaunchedEffect(state.failedTextSendId, state.failedTextSendText, selectedChatId) {
        val failedId = state.failedTextSendId
        if (failedId != null && failedId != lastRestoredFailureId &&
            selectedChatId == state.outbox[failedId]?.chatId) {
            if (draft.isBlank()) draft = state.failedTextSendText.orEmpty()
            lastRestoredFailureId = failedId
        }
    }
    val sendCurrentLocation: (Long) -> Unit = { chatId ->
        activeLocationRequest?.cancel()
        activeLocationRequest = requestCurrentLocation(context) { result ->
            if (selectedChatId == chatId) {
                result.onSuccess { point ->
                    viewModel.sendLocation(chatId, point.latitude, point.longitude, point.accuracyRadiusMeters)
                }.onFailure {
                    viewModel.reportLocationError("Unable to get current location")
                }
            }
        }
    }
    val locationPermissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { grants ->
        val chatId = pendingLocationChatId
        pendingLocationChatId = 0L
        val granted = grants[Manifest.permission.ACCESS_FINE_LOCATION] == true ||
            grants[Manifest.permission.ACCESS_COARSE_LOCATION] == true
        if (granted && chatId != 0L && selectedChatId == chatId) sendCurrentLocation(chatId)
        else if (chatId != 0L && selectedChatId == chatId) viewModel.reportLocationError("Location permission is required to send your location")
    }
    var showForwardTargets by remember { mutableStateOf(false) }
    var forwardingSourceChatId by remember { mutableLongStateOf(0L) }
    var forwardingMessageIds by remember { mutableStateOf<List<Long>>(emptyList()) }
    var profilePeerId by rememberSaveable { mutableLongStateOf(0L) }
    LaunchedEffect(state.selectionGeneration, state.authorization) {
        showForwardTargets = false
        forwardingMessageIds = emptyList()
        if (replySelectionGeneration != state.selectionGeneration || state.authorization != AuthorizationUiStep.Ready) {
            replyToMessageId = null
            replyToPreview = null
            jumpToMessageId = null
            replySelectionGeneration = state.selectionGeneration
        }
    }
    var confirmedJoinIntent by remember(incomingLink) { mutableStateOf<TelegramJoinInviteIntent?>(null) }

    when {
        !authorizationUiCanOpen(state.libraryLoaded, state.sessionRestoreComplete) -> StartupScreen(state)

        state.authorization != AuthorizationUiStep.Ready -> AuthorizationScreen(
            state = AuthorizationUiState(
                authorization = state.authorization,
                loginCodeDeliveryMethod = state.loginCodeDeliveryMethod,
                isTelegramConfigured = state.telegramConfigured,
                isLoading = state.busy,
                error = state.error,
            ),
            onStart = viewModel::sendStartCommand,
            onSubmitPhoneNumber = viewModel::submitPhoneNumber,
            onSubmitCode = viewModel::submitCode,
            onSubmitPassword = viewModel::submitPassword,
            onBack = {
                if (state.telegramConfigured) {
                    selectedChatId = 0L
                    viewModel.restartAuthorization()
                }
            },
        )

        selectedChatId == 0L && showContacts -> ContactsScreen(
            contacts = state.contacts,
            total = state.contactTotal,
            isLoading = state.contactsLoading,
            error = state.error,
            onBack = { showContacts = false },
            onRefresh = { viewModel.loadContacts(offset = 0) },
            onLoadMore = { viewModel.loadContacts(offset = state.contacts.size) },
            onContactSelected = { contact ->
                selectedChatId = contact.userId
                replyToMessageId = null
                replyToPreview = null
                jumpToMessageId = null
                viewModel.loadMessages(contact.userId)
                viewModel.loadPinnedMessages(contact.userId)
            },
        )

        selectedChatId == 0L -> ChatListScreen(
            chats = state.chats,
            selectedChatId = null,
            isLoading = state.busy || state.chatPagination.loading,
            error = state.error ?: state.chatPagination.error.takeIf { state.chats.isEmpty() },
            onRefresh = {
                if (state.chatPagination.error != null) viewModel.retryChatsPage()
                else viewModel.loadChats()
            },
            hasMoreChats = state.chatPagination.nextCursor != null && !state.chatPagination.exhausted,
            isLoadingMoreChats = state.chatPagination.loading && state.chatsLoaded,
            chatPageError = state.chatPagination.error.takeIf { state.chats.isNotEmpty() },
            onLoadMoreChats = viewModel::loadNextChatsPage,
            onRetryChatPage = viewModel::retryChatsPage,
            showEnableNotifications = showEnableNotifications,
            onEnableNotifications = onEnableNotifications,
            onOpenContacts = {
                showContacts = true
                viewModel.loadContacts(offset = 0)
            },
            onLogout = {
                onDismissNotification()
                selectedChatId = 0L
                showContacts = false
                draft = ""
                replyToMessageId = null
                replyToPreview = null
                jumpToMessageId = null
                viewModel.logout()
            },
            onChatSelected = { chatId ->
                selectedChatId = chatId
                replyToMessageId = null
                replyToPreview = null
                jumpToMessageId = null
                viewModel.loadMessages(chatId)
                viewModel.loadPinnedMessages(chatId)
            },
        )

        else -> {
            val chat = state.chats.firstOrNull { it.id == selectedChatId }
            val contact = state.contacts.firstOrNull { it.userId == selectedChatId }
            val rawMessages = state.messages[selectedChatId].orEmpty()
            val messagesById = rawMessages.associateBy { it.id }
            val messages = rawMessages.map { message ->
                val pollId = message.poll?.id
                val replyId = message.replyToMessageId
                val original = replyId?.let(messagesById::get)
                val summary = replyId?.let {
                    if (original == null) "Сообщение $it недоступно"
                    else {
                        val sender = if (original.isOutgoing) "Вы"
                        else original.senderId.takeIf { id -> id != 0L }?.let { id -> "Пользователь $id" } ?: "Пользователь"
                        val body = original.text.takeIf(String::isNotBlank)
                            ?: original.attachments.firstOrNull()?.kind?.name?.lowercase()
                            ?: "Медиа"
                        "$sender: $body"
                    }
                }
                message.copy(
                    pollVoting = pollId != null && pollId in state.votingPollIds,
                    pollError = pollId?.let(state.pollVoteErrors::get),
                    replyToSummary = summary,
                )
            }

            ConversationScreen(
                state = ConversationScreenState(
                    chatId = selectedChatId.toString(),
                    chatTitle = chat?.title ?: contact?.displayName.orEmpty(),
                    messages = messages,
                    activityStatus = com.chitik.telegram.nativebridge.chatActivityStatus(
                        state.chatActivities[selectedChatId].orEmpty(),
                    ),
                    draft = draft,
                    loading = state.busy,
                    sending = state.mediaImportInProgress ||
                        (state.sendingText && state.activeTextSendChatId == selectedChatId),
                    sendStatus = if (state.mediaImportInProgress) "Подготовка файла…" else
                        state.textSendStatusForChat(selectedChatId),
                    outgoingMediaSend = state.mediaSendForChat(selectedChatId),
                    voiceRecordingChatId = state.voiceRecording.chatId,
                    voiceRecordingDurationMs = state.voiceRecording.durationMs,
                    voiceRecordingError = voiceErrorForSelectedChat(
                        state.voiceRecording.error,
                        state.voiceRecording.errorChatId,
                        selectedChatId,
                    ),
                    error = state.textSendError ?: state.error,
                    pendingReactionKeys = state.pendingReactionKeys,
                    reactionErrors = state.reactionErrors,
                    forwardingError = state.forwardError,
                    forwardSuccessCount = state.forwardSuccessCount,
                    selectionGeneration = state.selectionGeneration,
                    deletingMessages = state.deletingMessages,
                    deleteError = state.deleteError,
                    messageMutationPending = state.messageMutationPending,
                    messageMutationError = state.messageMutationError,
                    messageMutationChatId = state.messageMutationChatId,
                    messageMutationMessageId = state.messageMutationMessageId,
                    messageMutationSuccessCount = state.messageMutationSuccessCount,
                    messageMutationSuccessChatId = state.messageMutationSuccessChatId,
                    messageMutationSuccessMessageId = state.messageMutationSuccessMessageId,
                    capabilities = state.chatCapabilities[selectedChatId] ?: ChatCapabilitiesUi(),
                    pinnedMessageIds = state.pinnedMessages[selectedChatId].orEmpty().map(Long::toString),
                    pendingPinnedMessageIds = state.pendingPinnedMessageIds
                        .filter { it.startsWith("$selectedChatId:") }
                        .map { it.substringAfter(':') }
                        .toSet(),
                    pinnedError = state.pinnedFailureKey
                        ?.takeIf { it.startsWith("$selectedChatId:") }
                        ?.let { key -> "Сообщение #${key.substringAfter(':')}: ${state.pinnedError.orEmpty()}" },
                    historyPageLoadingBeforeMessageId = state.loadingHistoryBefore[selectedChatId],
                    historyPageCompletedBeforeMessageId = state.completedHistoryBefore[selectedChatId],
                    historyPageError = state.historyPageErrors[selectedChatId],
                    replyingToMessageId = replyToMessageId?.toString(),
                    replyingToPreview = replyToPreview,
                    jumpToMessageId = jumpToMessageId,
                ),
                onBack = {
                    selectedChatId = 0L
                    draft = ""
                    replyToMessageId = null
                    replyToPreview = null
                    jumpToMessageId = null
                },
                onDraftChange = { draft = it },
                onReplyMessage = { message ->
                    replyToMessageId = message.id.toLongOrNull()
                    replyToPreview = message.text.takeIf(String::isNotBlank)
                        ?: message.attachments.firstOrNull()?.kind?.name?.lowercase()
                        ?: "Медиа"
                },
                onCancelReply = {
                    replyToMessageId = null
                    replyToPreview = null
                },
                onOpenReplyTarget = { targetId ->
                    jumpToMessageId = targetId
                    if (replyTargetIndex(messages, targetId) < 0) {
                        targetId.toLongOrNull()?.takeIf { it in 1 until Long.MAX_VALUE }?.let {
                            viewModel.loadMessagesBefore(selectedChatId, it + 1L)
                        }
                        Toast.makeText(context, "Загружаю исходное сообщение", Toast.LENGTH_SHORT).show()
                    }
                },
                onJumpComplete = { jumpToMessageId = null },
                onPickPhotoOrVideo = {
                    photoPickerLauncher.launch(
                        PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageAndVideo),
                    )
                },
                onPickDocument = { documentPickerLauncher.launch(arrayOf("*/*")) },
                onRetryMediaSend = viewModel::retryMediaSend,
                onCancelMediaSend = viewModel::cancelMediaSend,
                onStartVoiceRecording = requestVoiceRecording,
                onStopVoiceRecording = { chatId ->
                    if (selectedChatId == chatId && state.voiceRecording.chatId == chatId) {
                        viewModel.stopAndSendVoiceRecording(chatId)
                    }
                },
                onCancelVoiceRecording = { chatId ->
                    if (state.voiceRecording.chatId == chatId) viewModel.cancelVoiceRecording(chatId)
                },
                onSend = { chatId, text ->
                    chatId.toLongOrNull()?.let { numericChatId ->
                        val target = replyToMessageId
                        if (target != null) {
                            if (viewModel.replyMessage(numericChatId, target, text)) {
                                draft = ""
                                replyToMessageId = null
                                replyToPreview = null
                            }
                        } else if (viewModel.sendText(numericChatId, text)) {
                            draft = ""
                        }
                    }
                },
                onPollVote = viewModel::votePoll,
                onEditMessage = viewModel::editMessage,
                onDeleteMessageForEveryone = viewModel::deleteMessageForEveryone,
                onDeleteMessageForMe = viewModel::deleteMessageForMe,
                onReaction = viewModel::setMessageReaction,
                onSendLocation = { chatId ->
                    val numericChatId = chatId.toLongOrNull() ?: return@ConversationScreen
                    val granted =
                        context.checkSelfPermission(Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED ||
                            context.checkSelfPermission(Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED
                    if (granted) sendCurrentLocation(numericChatId) else {
                        pendingLocationChatId = numericChatId
                        locationPermissionLauncher.launch(arrayOf(
                            Manifest.permission.ACCESS_COARSE_LOCATION,
                            Manifest.permission.ACCESS_FINE_LOCATION,
                        ))
                    }
                },
                onOpenLocation = { location ->
                    val point = location.point ?: return@ConversationScreen
                    val label = when (location) {
                        is ConversationLocationUi.Point -> "Location"
                        is ConversationLocationUi.Venue -> location.title
                    }
                    if (!openLocationInMap(context, point.latitude, point.longitude, label)) {
                        viewModel.reportLocationError("No map application is available")
                    }
                },
                onForward = { sourceChatId, messageIds ->
                    forwardingSourceChatId = sourceChatId
                    forwardingMessageIds = messageIds
                    showForwardTargets = messageIds.isNotEmpty()
                },
                onDelete = viewModel::deleteMessages,
                onCopy = { text ->
                    if (text.isNotBlank()) {
                        context.getSystemService(android.content.ClipboardManager::class.java)
                            ?.setPrimaryClip(android.content.ClipData.newPlainText("Telegram messages", text))
                        Toast.makeText(context, "Сообщения скопированы", Toast.LENGTH_SHORT).show()
                    }
                },
                onRetry = { viewModel.loadMessages(selectedChatId) },
                onTogglePin = { chatId, messageId, isPinned ->
                    val parsedChat = chatId.toLongOrNull()
                    val parsedMessage = messageId.toLongOrNull()
                    if (parsedChat != null && parsedMessage != null) {
                        viewModel.toggleMessagePin(parsedChat, parsedMessage, isPinned)
                    }
                },
                onJumpToPinned = { messageId ->
                    jumpToMessageId = messageId
                    if (messages.none { it.id == messageId }) {
                        messageId.toLongOrNull()?.takeIf { it > 0 && it < Long.MAX_VALUE }?.let {
                            viewModel.loadMessagesBefore(selectedChatId, it + 1L)
                        }
                    }
                },
                onOpenProfile = {
                    profilePeerId = selectedChatId
                    viewModel.loadProfile(selectedChatId)
                },
                onDownloadMedia = { messageId, attachmentIndex, retry ->
                    viewModel.downloadMedia(
                        chatId = selectedChatId,
                        messageId = messageId,
                        attachmentIndex = attachmentIndex,
                        retry = retry,
                    )
                },
                onCancelMedia = { messageId, attachmentIndex ->
                    viewModel.cancelMediaDownload(
                        chatId = selectedChatId,
                        messageId = messageId,
                        attachmentIndex = attachmentIndex,
                    )
                },
                onOpenDocument = { attachment ->
                    when (MediaFileOpener.open(context, attachment)) {
                        OpenMediaResult.Opened -> Unit
                        OpenMediaResult.MissingFile -> Toast.makeText(
                            context,
                            "Файл больше не находится в кэше",
                            Toast.LENGTH_SHORT,
                        ).show()
                        OpenMediaResult.UnsafePath -> Toast.makeText(
                            context,
                            "Открытие файла заблокировано",
                            Toast.LENGTH_SHORT,
                        ).show()
                        OpenMediaResult.NoHandler -> Toast.makeText(
                            context,
                            "Нет приложения для этого типа файла",
                            Toast.LENGTH_SHORT,
                        ).show()
                        is OpenMediaResult.Failed -> Toast.makeText(
                            context,
                            "Не удалось открыть файл",
                            Toast.LENGTH_SHORT,
                        ).show()
                    }
                },
            )
        }
    }
    if (profilePeerId != 0L) {
        PeerProfileDialog(
            profile = state.profiles[profilePeerId],
            loading = state.profileLoadingPeerId == profilePeerId,
            error = state.profileError,
            onDismiss = { profilePeerId = 0L },
        )
    }
    if (showForwardTargets) {
        AlertDialog(
            onDismissRequest = { showForwardTargets = false },
            title = { Text("Куда переслать?") },
            text = {
                androidx.compose.foundation.lazy.LazyColumn(modifier = Modifier.heightIn(max = 360.dp)) {
                    val sourceMessages = state.messages[forwardingSourceChatId].orEmpty()
                        .associateBy { it.id.toLongOrNull() }
                    val requiresMedia = forwardingMessageIds.any { messageId ->
                        sourceMessages[messageId]?.attachments?.isNotEmpty() != false
                    }
                    items(
                        state.chats.filter { target ->
                            target.id != forwardingSourceChatId &&
                                target.capabilities.canSendNow == true &&
                                (!requiresMedia || target.capabilities.canSendMedia == true)
                        },
                        key = { it.id },
                    ) { target ->
                        TextButton(
                            onClick = {
                                viewModel.forwardMessages(forwardingSourceChatId, forwardingMessageIds, target.id)
                                showForwardTargets = false
                            },
                            modifier = Modifier.fillMaxWidth(),
                        ) { Text(target.title, maxLines = 1) }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { showForwardTargets = false }) { Text("Закрыть") } },
        )
    }
    incomingLink?.let { link ->
        val title = if (link.requiresConfirmation) "Review Telegram invite" else "Telegram link received"
        val description = when (val target = link.target) {
            is TelegramLinkTarget.Peer -> "Profile link for @${target.username}; peer resolution is required before opening it."
            is TelegramLinkTarget.Message -> "Message ${target.messageId} in @${target.username}."
            is TelegramLinkTarget.PrivateChannelMessage -> "Message ${target.messageId} in private channel ${target.channelId}."
            is TelegramLinkTarget.Invite -> "Invite recognized. Joining will not happen just by opening this link."
            is TelegramLinkTarget.ComposeMessage -> "Compose link received${target.recipient?.let { " for @$it" }.orEmpty()}. The text is not sent automatically."
        }
        AlertDialog(
            onDismissRequest = onDismissLink,
            title = { Text(title) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(description)
                    (link.target as? TelegramLinkTarget.ComposeMessage)?.text?.let { Text(it) }
                    if (confirmedJoinIntent != null) {
                        Text("Confirmation recorded. No join request has been sent.")
                    }
                }
            },
            confirmButton = {
                val inviteRoute = remember(link) { TelegramLinkAppRouter.route(link) }
                if (link.requiresConfirmation && confirmedJoinIntent == null) {
                    TextButton(onClick = {
                        confirmedJoinIntent = TelegramLinkAppRouter.confirmInvite(inviteRoute)
                    }) { Text("Confirm invite") }
                } else {
                    TextButton(onClick = onDismissLink) { Text("Close") }
                }
            },
            dismissButton = if (link.requiresConfirmation && confirmedJoinIntent == null) {
                { TextButton(onClick = onDismissLink) { Text("Cancel") } }
            } else {
                null
            },
        )
    }
}

@Composable
private fun StartupScreen(state: NativeUiState) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(28.dp),
        verticalArrangement = Arrangement.Center,
    ) {
        Text(
            text = "Telegram Rust",
            style = MaterialTheme.typography.headlineMedium,
            fontWeight = FontWeight.SemiBold,
        )
        Spacer(Modifier.height(12.dp))
        if (state.busy) {
            CircularProgressIndicator()
            Spacer(Modifier.height(12.dp))
        }
        Text(
            text = state.error ?: "Loading native Rust core…",
            color = if (state.error == null) {
                MaterialTheme.colorScheme.onSurfaceVariant
            } else {
                MaterialTheme.colorScheme.error
            },
        )
    }
}


private tailrec fun Context.findLifecycleOwner(): LifecycleOwner? = when (this) {
    is LifecycleOwner -> this
    is ContextWrapper -> {
        val base = baseContext
        if (base === this) null else base.findLifecycleOwner()
    }
    else -> null
}
