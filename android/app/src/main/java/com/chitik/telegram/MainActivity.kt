package com.chitik.telegram

import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
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
    var selectedChatId by rememberSaveable { mutableLongStateOf(0L) }
    var draft by rememberSaveable { mutableStateOf("") }
    var replyToMessageId by rememberSaveable { mutableStateOf<Long?>(null) }
    var replyToPreview by rememberSaveable { mutableStateOf<String?>(null) }
    var jumpToMessageId by rememberSaveable { mutableStateOf<String?>(null) }
    var showForwardTargets by remember { mutableStateOf(false) }
    var forwardingSourceChatId by remember { mutableLongStateOf(0L) }
    var forwardingMessageIds by remember { mutableStateOf<List<Long>>(emptyList()) }
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

        selectedChatId == 0L -> ChatListScreen(
            chats = state.chats,
            selectedChatId = null,
            isLoading = state.busy,
            error = state.error,
            onRefresh = { viewModel.loadChats() },
            onChatSelected = { chatId ->
                selectedChatId = chatId
                replyToMessageId = null
                replyToPreview = null
                jumpToMessageId = null
                viewModel.loadMessages(chatId)
            },
        )

        else -> {
            val chat = state.chats.firstOrNull { it.id == selectedChatId }
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
                    chatTitle = chat?.title.orEmpty(),
                    messages = messages,
                    draft = draft,
                    loading = state.busy,
                    sending = false,
                    error = state.error,
                    forwardingError = state.forwardError,
                    forwardSuccessCount = state.forwardSuccessCount,
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
                        targetId.toLongOrNull()?.let { viewModel.loadMessagesBefore(selectedChatId, it + 1L) }
                        Toast.makeText(context, "Загружаю исходное сообщение", Toast.LENGTH_SHORT).show()
                    }
                },
                onJumpComplete = { jumpToMessageId = null },
                onSend = { chatId, text ->
                    chatId.toLongOrNull()?.let { numericChatId ->
                        val target = replyToMessageId
                        if (target != null) viewModel.replyMessage(numericChatId, target, text)
                        else viewModel.sendText(numericChatId, text)
                        draft = ""
                        replyToMessageId = null
                        replyToPreview = null
                    }
                },
                onPollVote = viewModel::votePoll,
                onForward = { sourceChatId, messageIds ->
                    forwardingSourceChatId = sourceChatId
                    forwardingMessageIds = messageIds
                    showForwardTargets = messageIds.isNotEmpty()
                },
                onRetry = { viewModel.loadMessages(selectedChatId) },
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
