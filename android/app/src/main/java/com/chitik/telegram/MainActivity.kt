package com.chitik.telegram

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.AnimatedContent
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
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
            Text(
                text = if (state.libraryLoaded) "Loading Rust core…" else "Checking native bridge…",
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
            AppDestination.Authorization -> AuthorizationPlaceholder(padding)
            AppDestination.Chats -> ChatListPlaceholder(padding)
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
                title = "Native handle",
                value = if (state.nativeHandle != 0L) "Active" else "Not created",
                detail = if (state.nativeHandle != 0L) "Opaque handle #\${state.nativeHandle}" else "No Rust session",
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
                Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer)) {
                    Column(Modifier.padding(16.dp)) {
                        Text("Bridge error", fontWeight = FontWeight.SemiBold)
                        Spacer(Modifier.height(6.dp))
                        Text(error, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
        }
        item {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Button(onClick = onProbe, enabled = state.libraryLoaded) {
                    Text("JNI round-trip")
                }
                OutlinedButton(onClick = onStartCore, enabled = state.nativeHandle != 0L) {
                    Text("CoreCommand::Start")
                }
            }
        }
        item {
            Text(
                text = "The temporary disconnected Rust backend intentionally reports an error for Telegram commands until the protocol backend agent is linked. The command still travels through the real CoreHandle/CoreEvent pipeline.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
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
private fun AuthorizationPlaceholder(padding: PaddingValues) {
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
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(Modifier.padding(18.dp)) {
                Text("Phone / code / password flow")
                Spacer(Modifier.height(8.dp))
                Text(
                    "UI placeholder is ready for CoreEvent.AuthorizationChanged. Credentials and Telegram API secrets are not stored in the Android layer.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun ChatListPlaceholder(padding: PaddingValues) {
    val rows = listOf("Saved Messages", "Example chat", "Rust integration")
    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .padding(padding),
        contentPadding = PaddingValues(20.dp),
    ) {
        item {
            Text(
                "Chats",
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.SemiBold,
            )
            Spacer(Modifier.height(14.dp))
        }
        items(rows) { title ->
            Column(
                Modifier
                    .fillMaxWidth()
                    .padding(vertical = 12.dp),
            ) {
                Text(title, style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(4.dp))
                Text(
                    "ChatSummary placeholder — data will come from CoreEvent.ChatsLoaded",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            HorizontalDivider()
        }
    }
}
