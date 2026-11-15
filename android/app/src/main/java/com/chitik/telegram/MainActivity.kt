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
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.chitik.telegram.nativebridge.NativeSessionViewModel
import com.chitik.telegram.nativebridge.NativeUiState

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                Surface(modifier = Modifier.fillMaxSize()) {
                    TelegramRustApp()
                }
            }
        }
    }
}

@Composable
private fun TelegramRustApp(viewModel: NativeSessionViewModel = viewModel()) {
    val state by viewModel.state.collectAsState()
    var selectedChatId by rememberSaveable { mutableLongStateOf(0L) }
    var draft by rememberSaveable { mutableStateOf("") }

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
            val messages = state.messages[selectedChatId].orEmpty()

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
                onRetry = { viewModel.loadMessages(selectedChatId) },
            )
        }
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
