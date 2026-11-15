package com.chitik.telegram

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
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
    var editConfig by rememberSaveable { mutableStateOf(false) }
    var selectedChatId by rememberSaveable { mutableLongStateOf(0L) }
    var draft by rememberSaveable { mutableStateOf("") }

    when {
        !state.libraryLoaded -> StartupScreen(state)
        !state.telegramConfigured || editConfig -> TelegramConfigScreen(
            state = state,
            onSave = { apiId, apiHash ->
                editConfig = false
                viewModel.saveTelegramConfig(apiId, apiHash)
            },
            onUseSaved = { editConfig = false },
            onClear = {
                selectedChatId = 0L
                viewModel.clearTelegramConfig()
            },
        )
        state.authorization != AuthorizationUiStep.Ready -> AuthorizationScreen(
            state = AuthorizationUiState(
                authorization = state.authorization,
                isLoading = state.busy,
                error = state.error,
            ),
            onStart = viewModel::sendStartCommand,
            onSubmitPhoneNumber = viewModel::submitPhoneNumber,
            onSubmitCode = viewModel::submitCode,
            onSubmitPassword = viewModel::submitPassword,
            onBack = { editConfig = true },
        )
        selectedChatId == 0L -> ChatListScreen(
            chats = state.chats,
            selectedChatId = null,
            isLoading = state.busy,
            error = state.error,
            onRefresh = viewModel::loadChats,
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

@Composable
private fun TelegramConfigScreen(
    state: NativeUiState,
    onSave: (String, String) -> Unit,
    onUseSaved: () -> Unit,
    onClear: () -> Unit,
) {
    var apiId by rememberSaveable { mutableStateOf("") }
    var apiHash by rememberSaveable { mutableStateOf("") }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 24.dp, vertical = 40.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Text(
            text = "Telegram API",
            style = MaterialTheme.typography.headlineMedium,
            fontWeight = FontWeight.SemiBold,
        )
        Text(
            text = "API ID and API hash are validated, encrypted with Android Keystore, and stored only in app-private storage.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        OutlinedTextField(
            value = apiId,
            onValueChange = { apiId = it.filter(Char::isDigit) },
            label = { Text("API ID") },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            singleLine = true,
            enabled = !state.busy,
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = apiHash,
            onValueChange = { apiHash = it },
            label = { Text("API hash") },
            singleLine = true,
            enabled = !state.busy,
            visualTransformation = PasswordVisualTransformation(),
            modifier = Modifier.fillMaxWidth(),
        )

        state.error?.let {
            Text(
                text = it,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
            )
        }

        Button(
            onClick = { onSave(apiId, apiHash) },
            enabled = apiId.isNotBlank() && apiHash.isNotBlank() && !state.busy,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(if (state.busy) "Connecting…" else "Save and connect")
        }

        if (state.telegramConfigured) {
            OutlinedButton(
                onClick = onUseSaved,
                enabled = !state.busy,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("Keep saved configuration")
            }
            OutlinedButton(
                onClick = onClear,
                enabled = !state.busy,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("Clear saved API configuration")
            }
        }
    }
}
