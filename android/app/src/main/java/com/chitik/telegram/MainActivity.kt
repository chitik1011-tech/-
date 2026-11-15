package com.chitik.telegram

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.AnimatedContent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.weight
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.chitik.telegram.nativebridge.AuthorizationUiState
import com.chitik.telegram.nativebridge.ChatUi
import com.chitik.telegram.nativebridge.MessageUi
import com.chitik.telegram.nativebridge.NativeSessionViewModel
import com.chitik.telegram.nativebridge.NativeUiState
import kotlinx.coroutines.delay

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

private enum class AppDestination(val label: String) {
    Core("Core"),
    Authorization("Auth"),
    Chats("Chats"),
}

@Composable
private fun TelegramRustApp(viewModel: NativeSessionViewModel = viewModel()) {
    val nativeState by viewModel.state.collectAsState()
    var showSplash by rememberSaveable { mutableStateOf(true) }
    var destination by rememberSaveable { mutableStateOf(AppDestination.Core) }

    LaunchedEffect(Unit) {
        delay(650)
        showSplash = false
    }

    LaunchedEffect(nativeState.telegramConfigured, nativeState.authorization) {
        destination = when {
            !nativeState.telegramConfigured -> AppDestination.Authorization
            nativeState.authorization == AuthorizationUiState.Ready -> AppDestination.Chats
            else -> AppDestination.Authorization
        }
    }

    AnimatedContent(targetState = showSplash, label = "startup") { splash ->
        if (splash) {
            StartScreen(nativeState)
        } else {
            MainShell(
                state = nativeState,
                destination = destination,
                onDestinationChanged = { destination = it },
                onProbe = viewModel::probeRoundTrip,
                onStartCore = viewModel::sendStartCommand,
                onConfigureTelegram = viewModel::configureTelegram,
                onPhone = viewModel::submitPhoneNumber,
                onCode = viewModel::submitCode,
                onPassword = viewModel::submitPassword,
                onRefreshChats = viewModel::loadChats,
                onLoadMessages = viewModel::loadMessages,
                onSendText = viewModel::sendText,
            )
        }
    }
}

@Composable
private fun StartScreen(state: NativeUiState) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .padding(32.dp),
        contentAlignment = Alignment.Center,
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                text = "Telegram Rust Mobile",
                style = MaterialTheme.typography.headlineMedium,
                fontWeight = FontWeight.SemiBold,
            )
            Spacer(Modifier.height(12.dp))
            if (state.busy) {
                CircularProgressIndicator()
                Spacer(Modifier.height(12.dp))
            }
            Text(
                text = when {
                    !state.libraryLoaded -> "Checking native bridge…"
                    state.telegramConfigured -> "Starting Rust Telegram backend…"
                    else -> "Rust core loaded"
                },
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun MainShell(
    state: NativeUiState,
    destination: AppDestination,
    onDestinationChanged: (AppDestination) -> Unit,
    onProbe: () -> Unit,
    onStartCore: () -> Unit,
    onConfigureTelegram: (String, String) -> Unit,
    onPhone: (String) -> Unit,
    onCode: (String) -> Unit,
    onPassword: (String) -> Unit,
    onRefreshChats: () -> Unit,
    onLoadMessages: (Long, Int) -> Unit,
    onSendText: (Long, String) -> Unit,
) {
    Scaffold(
        bottomBar = {
            NavigationBar {
                AppDestination.entries.forEach { item ->
                    NavigationBarItem(
                        selected = destination == item,
                        onClick = { onDestinationChanged(item) },
                        icon = { Text(item.label.take(1)) },
                        label = { Text(item.label) },
                    )
                }
            }
        },
    ) { padding ->
        when (destination) {
            AppDestination.Core -> CoreStatusScreen(state, padding, onProbe, onStartCore)
            AppDestination.Authorization -> AuthorizationScreen(
                state = state,
                padding = padding,
                onConfigureTelegram = onConfigureTelegram,
                onStartCore = onStartCore,
                onPhone = onPhone,
                onCode = onCode,
                onPassword = onPassword,
            )
            AppDestination.Chats -> ChatListScreen(
                state = state,
                padding = padding,
                onRefresh = onRefreshChats,
                onLoadMessages = onLoadMessages,
                onSendText = onSendText,
            )
        }
    }
}

@Composable
private fun CoreStatusScreen(
    state: NativeUiState,
    padding: PaddingValues,
    onProbe: () -> Unit,
    onStartCore: () -> Unit,
) {
    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .padding(padding),
        contentPadding = PaddingValues(20.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        item {
            Text(
                text = "Rust core status",
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.SemiBold,
            )
        }
        item {
            StatusCard(
                title = "Native library",
                value = if (state.libraryLoaded) "Loaded" else "Unavailable",
                detail = state.coreVersion,
            )
        }
        item {
            StatusCard(
                title = "Telegram backend",
                value = if (state.telegramConfigured) "Grammers / MTProto" else "Not configured",
                detail = "Authorization: ${state.authorization.name}",
            )
        }
        item {
            StatusCard(
                title = "Native handle",
                value = if (state.nativeHandle != 0L) "Active" else "Not created",
                detail = if (state.nativeHandle != 0L) {
                    "Opaque handle #${state.nativeHandle}"
                } else {
                    "No Rust session"
                },
            )
        }
        item {
            StatusCard(
                title = "Last Rust → Android event",
                value = state.lastEventType,
                detail = state.lastEventPayload.ifBlank { "No callback received yet" },
            )
        }
        state.error?.let { error ->
            item {
                ErrorCard(error)
            }
        }
        item {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Button(onClick = onProbe, enabled = state.libraryLoaded) {
                    Text("JNI round-trip")
                }
                OutlinedButton(onClick = onStartCore, enabled = state.nativeHandle != 0L) {
                    Text("Start")
                }
            }
        }
    }
}

@Composable
private fun AuthorizationScreen(
    state: NativeUiState,
    padding: PaddingValues,
    onConfigureTelegram: (String, String) -> Unit,
    onStartCore: () -> Unit,
    onPhone: (String) -> Unit,
    onCode: (String) -> Unit,
    onPassword: (String) -> Unit,
) {
    var apiId by rememberSaveable { mutableStateOf("") }
    var apiHash by rememberSaveable { mutableStateOf("") }
    var phone by rememberSaveable { mutableStateOf("") }
    var code by rememberSaveable { mutableStateOf("") }
    var password by rememberSaveable { mutableStateOf("") }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(padding)
            .padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Text(
            "Authorization",
            style = MaterialTheme.typography.headlineSmall,
            fontWeight = FontWeight.SemiBold,
        )

        if (state.busy) {
            CircularProgressIndicator()
        }

        if (!state.telegramConfigured) {
            Text(
                "Telegram API configuration",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                "API credentials are passed to Rust at runtime and are not saved by this screen. You can also provide TELEGRAM_API_ID / TELEGRAM_API_HASH at build time.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedTextField(
                value = apiId,
                onValueChange = { apiId = it.filter(Char::isDigit) },
                label = { Text("API ID") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = apiHash,
                onValueChange = { apiHash = it },
                label = { Text("API hash") },
                singleLine = true,
                visualTransformation = PasswordVisualTransformation(),
                modifier = Modifier.fillMaxWidth(),
            )
            Button(
                onClick = { onConfigureTelegram(apiId, apiHash) },
                enabled = apiId.isNotBlank() && apiHash.isNotBlank() && !state.busy,
            ) {
                Text("Connect Telegram backend")
            }
        } else {
            when (state.authorization) {
                AuthorizationUiState.Closed,
                AuthorizationUiState.Unknown,
                AuthorizationUiState.LoggingOut,
                -> {
                    Text("Telegram backend is configured.")
                    Button(onClick = onStartCore, enabled = !state.busy) {
                        Text("Start authorization")
                    }
                }

                AuthorizationUiState.WaitingForPhoneNumber -> {
                    OutlinedTextField(
                        value = phone,
                        onValueChange = { phone = it },
                        label = { Text("Phone number") },
                        placeholder = { Text("+123456789") },
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Phone),
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Button(
                        onClick = { onPhone(phone) },
                        enabled = phone.isNotBlank() && !state.busy,
                    ) {
                        Text("Send code")
                    }
                }

                AuthorizationUiState.WaitingForCode -> {
                    OutlinedTextField(
                        value = code,
                        onValueChange = { code = it },
                        label = { Text("Telegram code") },
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Button(
                        onClick = { onCode(code) },
                        enabled = code.isNotBlank() && !state.busy,
                    ) {
                        Text("Confirm code")
                    }
                }

                AuthorizationUiState.WaitingForPassword -> {
                    OutlinedTextField(
                        value = password,
                        onValueChange = { password = it },
                        label = { Text("2FA password") },
                        singleLine = true,
                        visualTransformation = PasswordVisualTransformation(),
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Button(
                        onClick = { onPassword(password) },
                        enabled = password.isNotEmpty() && !state.busy,
                    ) {
                        Text("Unlock")
                    }
                }

                AuthorizationUiState.Ready -> {
                    Text(
                        "Authorized",
                        style = MaterialTheme.typography.titleLarge,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text("The Rust MTProto session is ready.")
                }
            }
        }

        state.error?.let { ErrorCard(it) }
    }
}

@Composable
private fun ChatListScreen(
    state: NativeUiState,
    padding: PaddingValues,
    onRefresh: () -> Unit,
    onLoadMessages: (Long, Int) -> Unit,
    onSendText: (Long, String) -> Unit,
) {
    var selectedChatId by rememberSaveable { mutableLongStateOf(0L) }
    var messageText by rememberSaveable { mutableStateOf("") }
    val selected = state.chats.firstOrNull { it.id == selectedChatId }

    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .padding(padding),
        contentPadding = PaddingValues(20.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        item {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "Chats",
                    style = MaterialTheme.typography.headlineSmall,
                    fontWeight = FontWeight.SemiBold,
                )
                OutlinedButton(
                    onClick = onRefresh,
                    enabled = state.telegramConfigured &&
                        state.authorization == AuthorizationUiState.Ready,
                ) {
                    Text("Refresh")
                }
            }
            Spacer(Modifier.height(10.dp))
        }

        if (state.authorization != AuthorizationUiState.Ready) {
            item {
                Text("Authorize Telegram first.")
            }
        } else if (state.chats.isEmpty()) {
            item {
                Text(
                    "No chats loaded yet.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            items(state.chats, key = ChatUi::id) { chat ->
                ChatRow(
                    chat = chat,
                    selected = chat.id == selectedChatId,
                    onClick = {
                        selectedChatId = chat.id
                        onLoadMessages(chat.id, 100)
                    },
                )
            }
        }

        selected?.let { chat ->
            item {
                Spacer(Modifier.height(14.dp))
                Text(
                    chat.title,
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.SemiBold,
                )
                Spacer(Modifier.height(8.dp))
            }

            val messages = state.messages[chat.id].orEmpty()
            if (messages.isEmpty()) {
                item {
                    Text(
                        "No cached messages yet. New incoming messages will appear here live.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                items(messages, key = MessageUi::id) { message ->
                    MessageRow(message)
                }
            }

            item {
                Spacer(Modifier.height(8.dp))
                OutlinedTextField(
                    value = messageText,
                    onValueChange = { messageText = it },
                    label = { Text("Message") },
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(8.dp))
                Button(
                    onClick = {
                        val text = messageText.trim()
                        if (text.isNotEmpty()) {
                            onSendText(chat.id, text)
                            messageText = ""
                        }
                    },
                    enabled = messageText.isNotBlank(),
                ) {
                    Text("Send")
                }
            }
        }

        state.error?.let { error ->
            item {
                ErrorCard(error)
            }
        }
    }
}

@Composable
private fun ChatRow(
    chat: ChatUi,
    selected: Boolean,
    onClick: () -> Unit,
) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick),
        shape = RoundedCornerShape(16.dp),
        colors = if (selected) {
            CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.secondaryContainer)
        } else {
            CardDefaults.cardColors()
        },
    ) {
        Column(Modifier.padding(14.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    chat.title,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.weight(1f),
                )
                if (chat.unreadCount > 0) {
                    Text(
                        chat.unreadCount.toString(),
                        color = MaterialTheme.colorScheme.primary,
                        fontWeight = FontWeight.Bold,
                    )
                }
            }
            chat.lastMessage?.let {
                Spacer(Modifier.height(4.dp))
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
    HorizontalDivider()
}

@Composable
private fun MessageRow(message: MessageUi) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = if (message.outgoing) Arrangement.End else Arrangement.Start,
    ) {
        Card(
            shape = RoundedCornerShape(14.dp),
            colors = if (message.outgoing) {
                CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer)
            } else {
                CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant)
            },
        ) {
            Column(Modifier.padding(horizontal = 12.dp, vertical = 9.dp)) {
                Text(message.text, style = MaterialTheme.typography.bodyMedium)
                Spacer(Modifier.height(3.dp))
                Text(
                    if (message.outgoing) "You" else "Sender ${message.senderId}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun StatusCard(title: String, value: String, detail: String) {
    Card(modifier = Modifier.fillMaxWidth(), shape = RoundedCornerShape(18.dp)) {
        Column(Modifier.padding(16.dp)) {
            Text(
                title,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(6.dp))
            Text(value, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(4.dp))
            Text(
                text = detail,
                style = MaterialTheme.typography.bodySmall,
                maxLines = 4,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun ErrorCard(error: String) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer),
    ) {
        Column(Modifier.padding(16.dp)) {
            Text("Error", fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(6.dp))
            Text(error, style = MaterialTheme.typography.bodySmall)
        }
    }
}
