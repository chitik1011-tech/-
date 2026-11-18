package com.chitik.telegram

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.unit.dp
import com.chitik.telegram.nativebridge.GlobalMessageSearchResultUi
import com.chitik.telegram.nativebridge.GlobalMessageSearchUiState
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun GlobalMessageSearchScreen(
    state: GlobalMessageSearchUiState,
    chatTitle: (Long) -> String,
    onBack: () -> Unit,
    onSearch: (String) -> Unit,
    onOpenResult: (GlobalMessageSearchResultUi) -> Unit,
    modifier: Modifier = Modifier,
) {
    var query by rememberSaveable { mutableStateOf("") }
    val keyboard = LocalSoftwareKeyboardController.current
    val dateFormatter = DateTimeFormatter.ofPattern("MMM d, HH:mm", Locale.getDefault())
        .withZone(ZoneId.systemDefault())

    Scaffold(
        modifier = modifier,
        topBar = {
            TopAppBar(
                title = { Text("Search all messages", fontWeight = FontWeight.SemiBold) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { insets ->
        Column(
            modifier = Modifier.fillMaxSize().padding(insets).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextField(
                    value = query,
                    onValueChange = { query = it },
                    modifier = Modifier.weight(1f),
                    singleLine = true,
                    label = { Text("Search Telegram messages") },
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    keyboardActions = KeyboardActions(onSearch = {
                        keyboard?.hide()
                        onSearch(query)
                    }),
                )
                OutlinedButton(
                    onClick = {
                        keyboard?.hide()
                        onSearch(query)
                    },
                    enabled = query.isNotBlank() && !state.loading,
                ) {
                    Icon(Icons.Default.Search, contentDescription = "Search")
                }
            }

            state.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            if (state.loading) {
                CircularProgressIndicator()
            } else if (state.query.isNotBlank() && state.results.isEmpty() && state.error == null) {
                Text("No matching messages found.")
            }

            LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                items(state.results, key = { "${it.chatId}:${it.messageId}" }) { result ->
                    Card(
                        modifier = Modifier.fillMaxWidth().clickable { onOpenResult(result) },
                    ) {
                        Column(modifier = Modifier.padding(14.dp)) {
                            Text(chatTitle(result.chatId), style = MaterialTheme.typography.labelLarge)
                            Text(result.text, maxLines = 3)
                            Text(
                                dateFormatter.format(Instant.ofEpochSecond(result.timestampUnix)),
                                style = MaterialTheme.typography.labelSmall,
                            )
                        }
                    }
                }
            }
            if (state.results.size >= 50) {
                Text(
                    "Showing the first 50 matches. Refine your search for more specific results.",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
    }
}
