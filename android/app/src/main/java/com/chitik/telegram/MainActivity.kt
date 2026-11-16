package com.chitik.telegram

import android.Manifest
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.PackageManager
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
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
import com.chitik.telegram.deeplink.TelegramDeepLinkRouter
import com.chitik.telegram.deeplink.TelegramJoinInviteIntent
import com.chitik.telegram.deeplink.TelegramLinkAppRouter
import com.chitik.telegram.deeplink.TelegramLinkIntent
import com.chitik.telegram.deeplink.TelegramLinkTarget

class MainActivity : ComponentActivity() {
    private var pendingTelegramLink by mutableStateOf<TelegramLinkIntent?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        pendingTelegramLink = TelegramDeepLinkRouter.parseIntent(intent?.action, intent?.dataString)
        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                Surface(modifier = Modifier.fillMaxSize()) {
                    TelegramRustApp(
                        incomingLink = pendingTelegramLink,
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
    }
}

@Composable
private fun TelegramRustApp(
    incomingLink: TelegramLinkIntent?,
    onDismissLink: () -> Unit,
    viewModel: NativeSessionViewModel = viewModel(),
) {
    val state by viewModel.state.collectAsState()
    val context = LocalContext.current
    val lifecycleOwner = remember(context) { context.findLifecycleOwner() }
    var selectedChatId by rememberSaveable { mutableLongStateOf(0L) }
    var showContacts by rememberSaveable { mutableStateOf(false) }
    var draft by rememberSaveable { mutableStateOf("") }
    var pendingLocationChatId by rememberSaveable { mutableLongStateOf(0L) }
    var activeLocationRequest by remember { mutableStateOf<LocationRequestHandle?>(null) }
    DisposableEffect(lifecycleOwner) {
        if (lifecycleOwner == null) {
            onDispose { }
        } else {
            val observer = LifecycleEventObserver { _, event ->
                if (event == Lifecycle.Event.ON_STOP) {
                    activeLocationRequest?.cancel()
                    activeLocationRequest = null
                    pendingLocationChatId = 0L
                }
            }
            lifecycleOwner.lifecycle.addObserver(observer)
            onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
        }
    }
    DisposableEffect(selectedChatId) {
        onDispose {
            activeLocationRequest?.cancel()
            activeLocationRequest = null
            pendingLocationChatId = 0L
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
    }
    var confirmedJoinIntent by remember(incomingLink) { mutableStateOf<TelegramJoinInviteIntent?>(null) }

    when {
        !state.libraryLoaded -> StartupScreen(state)

        state.authorization != AuthorizationUiStep.Ready -> AuthorizationScreen(
            state = AuthorizationUiState(
                authorization = state.authorization,
                isTelegramConfigured = state.telegramConfigured,
                isLoading = state.busy,
                error = state.error,
            ),
            onConfigureTelegram = viewModel::saveTelegramConfig,
            onStart = viewModel::sendStartCommand,
            onSubmitPhoneNumber = viewModel::submitPhoneNumber,
            onSubmitCode = viewModel::submitCode,
            onSubmitPassword = viewModel::submitPassword,
            onBack = {
                if (state.telegramConfigured) {
                    selectedChatId = 0L
                    viewModel.clearTelegramConfig()
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
                viewModel.loadMessages(contact.userId)
            },
        )

        selectedChatId == 0L -> ChatListScreen(
            chats = state.chats,
            selectedChatId = null,
            isLoading = state.busy,
            error = state.error,
            onRefresh = { viewModel.loadChats() },
            onOpenContacts = {
                showContacts = true
                viewModel.loadContacts(offset = 0)
            },
            onChatSelected = { chatId ->
                selectedChatId = chatId
                viewModel.loadMessages(chatId)
            },
        )

        else -> {
            val chat = state.chats.firstOrNull { it.id == selectedChatId }
            val contact = state.contacts.firstOrNull { it.userId == selectedChatId }
            val messages = state.messages[selectedChatId].orEmpty().map { message ->
                val pollId = message.poll?.id
                if (pollId == null) {
                    message
                } else {
                    message.copy(
                        pollVoting = pollId in state.votingPollIds,
                        pollError = state.pollVoteErrors[pollId],
                    )
                }
            }

            ConversationScreen(
                state = ConversationScreenState(
                    chatId = selectedChatId.toString(),
                    chatTitle = chat?.title ?: contact?.displayName.orEmpty(),
                    messages = messages,
                    draft = draft,
                    loading = state.busy,
                    sending = false,
                    error = state.error,
                    forwardingError = state.forwardError,
                    forwardSuccessCount = state.forwardSuccessCount,
                    selectionGeneration = state.selectionGeneration,
                    deletingMessages = state.deletingMessages,
                    deleteError = state.deleteError,
                ),
                onBack = {
                    selectedChatId = 0L
                    draft = ""
                },
                onDraftChange = { draft = it },
                onSend = { chatId, text ->
                    chatId.toLongOrNull()?.let { numericChatId ->
                        viewModel.sendText(numericChatId, text)
                        draft = ""
                    }
                },
                onPollVote = viewModel::votePoll,
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
                    items(state.chats.filter { it.id != forwardingSourceChatId }, key = { it.id }) { target ->
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
