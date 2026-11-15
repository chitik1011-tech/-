package com.chitik.telegram

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
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
    var selectedChatId by rememberSaveable { mutableLongStateOf(0L) }
    var draft by rememberSaveable { mutableStateOf("") }
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
                viewModel.loadMessages(chatId)
            },
        )

        else -> {
            val chat = state.chats.firstOrNull { it.id == selectedChatId }
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
                    chatTitle = chat?.title.orEmpty(),
                    messages = messages,
                    draft = draft,
                    loading = state.busy,
                    sending = false,
                    error = state.error,
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
                onRetry = { viewModel.loadMessages(selectedChatId) },
            )
        }
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
