Warning: truncated output (original token count: 10409)
Total output lines: 844

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
import com.chitik.telegram.notifications.restorePendingNotificationRoute
import com.chitik.telegram.deeplink.TelegramDeepLinkRouter
import com.chitik.telegram.deeplink.pendingTelegramLinkAfterNewIntent
import com.chitik.telegram.deeplink.TelegramJoinInviteIntent
import com.chitik.telegram.deeplink.TelegramLinkAppRouter
import com.chitik.telegram.deeplink.TelegramLinkIntent
import com.chitik.telegram.deeplink.TelegramLinkTarget

class MainActivity : ComponentActivity() {
    private companion object {
        const val STATE_PENDING_NOTIFICATION_CHAT_ID = "pending_notification_chat_id"
        const val STATE_PENDING_NOTIFICATION_ACCOUNT_USER_ID = "pending_notification_account_user_id"
    }

    private var pendingTelegramLink by mutableStateOf<TelegramLinkIntent?>(null)
    private var pendingNotificationRoute by mutableStateOf<TelegramNotificationRoute?>(null)
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
        pendingNotificationRoute = restorePendingNotificationRoute(
            savedChatId = savedInstanceState
                ?.takeIf { it.containsKey(STATE_PENDING_NOTIFICATION_CHAT_ID) }
                ?.getLong(STATE_PENDING_NOTIFICATION_CHAT_ID),
            savedAccountUserId = savedInstanceState
                ?.takeIf { it.containsKey(STATE_PENDING_NOTIFICATION_ACCOUNT_USER_ID) }
                ?.getLong(STATE_PENDING_NOTIFICATION_ACCOUNT_USER_ID),
            freshIntentRoute = TelegramNotificationRoute.parse(intent),
        )
        setContent {
            TelegramAppTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    TelegramRustApp(
                        incomingLink = pendingTelegramLink,
                        incomingNotificationRoute = pendingNotificationRoute,
                        onDismissNotification = { pendingNotificationRoute = null },
                        showEnableNotifications = !notificationPermissionGranted,
                        onEnableNotifications = { notificationPermissionRequester.request() },
        onDismissLink = { dismissPendingTelegramLink() },
                    )
                }
            }
        }
    }

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        val incomingLink = TelegramDeepLinkRouter.parseIntent(intent.action, intent.dataString)
        val incomingNotificationRoute = TelegramNotificationRoute.parse(intent)
        // Launcher and unrelated intents can be delivered to this singleTop Activity while a
        // link review is open. Keep the original route as the Activity intent so recreation can
        // parse it again; replace the base intent only when a recognized route arrives.
        if (incomingLink != null || incomingNotificationRoute != null) setIntent(intent)
        pendingTelegramLink = pendingTelegramLinkAfterNewIntent(pendingTelegramLink, incomingLink)
        pendingNotificationRoute = restorePendingNotificationRoute(
            savedChatId = pendingNotificationRoute?.chatId,
            savedAccountUserId = pendingNotificationRoute?.accountUserId,
            freshIntentRoute = incomingNotificationRoute,
        )
    }

    private fun dismissPendingTelegramLink() {
        pendingTelegramLink = null
        if (TelegramDeepLinkRouter.parseIntent(intent?.action, intent?.dataString) != null) {
            setIntent(
                android.content.Intent(this, MainActivity::class.java)
                    .setAction(android.content.Intent.ACTION_MAIN),
            )
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        pendingNotificationRoute?.let { route ->
            outState.putLong(STATE_PENDING_NOTIFICATION_CHAT_ID, route.chatId)
            outState.putLong(STATE_PENDING_NOTIFICATION_ACCOUNT_USER_ID, route.accountUserId)
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
    incomingNotificationRoute: TelegramNotificationRoute?,
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
    var pendingVoiceRecordingChatId by remember { mutableLongStateOf(0L) }
    var pendingVoiceRecordingSelectionGeneration by remember { mutableLongStateOf(-1L) }
    var activeLocationRequest by remember { mutableStateOf<LocationRequestHandle?>(null) }
    val currentSelectedChatId by rememberUpdatedState(selectedChatId)
    val currentSelectionGeneration by rememberUpdatedState(state.selectionGeneration)
    val activeVoiceRecordingChatId by rememberUpdatedState(state.voiceRecording.chatId)
    val voiceRecordingPermissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        val chatId = pendingVoiceRecordingChatId
        val requestGeneration = pendingVoiceRecordingSelectionGeneration
        pendingVoiceRecordingChatId = 0L
        pendingVoiceRecordingSelectionGeneration = -1L
        voiceRecordingChatAfterPermission(
            granted, chatId, currentSelectedChatId, requestGeneration, currentSelectionGeneration,
        )?.let {
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
                pendingVoiceRecordingSelectionGeneration = state.selectionGeneration
                voiceRecordingPermissionLauncher.launch(Manifest.permission.RECORD_AUDIO)
            }
        }
    }
    LaunchedEffect(selectedChatId) {
        pendingVoiceRecordingChatId = pendingVoicePermissionChatAfterNavigation(
            pendingChatId = pendingVoiceRecordingChatId,
            selectedChatId = selectedChatId,
        )
        if (pendingVoiceRecordingChatId == 0L) pendingVoiceRecordingSelectionGeneration = -1L
    }
    LaunchedEffect(selectedChatId, state.voiceRecording.chatId) {
        val recordingChatId = state.voiceRecording.chatId
        if (shouldCancelVoiceRecordingForChatChange(recordingChatId, selectedChatId)) {
            viewModel.cancelVoiceRecording(recordingChatId)
        }
    }
    LaunchedEffect(incomingNotificationRoute, state.sessionRestoreComplete, state.authorization) {
        val sessionDecision = decidePendingNotificationSessionRestore(
            restoreComplete = state.sessionRestoreComplete,
            authorizationReady = state.authorization == AuthorizationUiStep.Ready,
        )
        if (incomingNotificationRoute != null &&
            sessionDecision == PendingNotificationSessionDecision.Discard
        ) {
            onDismissNotification()
        }
    }
    LaunchedEffect(
        incomingNotificationRoute,
        state.authorization,
        state.accountIdentityResolved,
        state.accountUserId,
        state.chatsLoaded,
        state.chats,
        state.c…4409 tokens truncated…kerLauncher.launch(arrayOf("*/*")) },
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
