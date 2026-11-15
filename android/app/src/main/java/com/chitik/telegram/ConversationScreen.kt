package com.chitik.telegram

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage
import kotlin.math.roundToInt

data class ConversationPollOptionUi(
    val optionId: List<Int>,
    val text: String,
    val voterCount: Int?,
    val chosen: Boolean,
    val correct: Boolean?,
)

data class ConversationPollUi(
    val id: Long,
    val chatId: Long,
    val messageId: Long,
    val question: String,
    val options: List<ConversationPollOptionUi>,
    val totalVoters: Int?,
    val publicVoters: Boolean,
    val multipleChoice: Boolean,
    val quiz: Boolean,
    val closed: Boolean,
    val explanation: String?,
)

/**
 * Display-ready message data supplied by the Rust-backed state layer.
 *
 * [timestamp] is formatted by the caller for the user's locale and timezone.
 * [id] should be the stable Telegram message identifier scoped to [chatId].
 */
data class ConversationMessageUi(
    val id: String,
    val chatId: String,
    val text: String,
    val timestamp: String,
    val isOutgoing: Boolean,
    val attachments: List<MediaAttachmentUi> = emptyList(),
    val poll: ConversationPollUi? = null,
    val pollVoting: Boolean = false,
    val pollError: String? = null,
)

data class ConversationScreenState(
    val chatId: String,
    val chatTitle: String,
    val messages: List<ConversationMessageUi>,
    val draft: String,
    val loading: Boolean = false,
    val sending: Boolean = false,
    val error: String? = null,
)

/**
 * Stateless conversation UI. Message history and poll results always come from the Rust-backed
 * state. Only an unsubmitted multiple-choice selection is kept locally.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ConversationScreen(
    state: ConversationScreenState,
    onBack: () -> Unit,
    onDraftChange: (String) -> Unit,
    onSend: (chatId: String, text: String) -> Unit,
    onPollVote: (poll: ConversationPollUi, optionIds: List<List<Int>>) -> Unit,
    onRetry: () -> Unit,
    onDownloadMedia: (messageId: Long, attachmentIndex: Int, retry: Boolean) -> Unit,
    onCancelMedia: (messageId: Long, attachmentIndex: Int) -> Unit,
    onOpenDocument: (MediaAttachmentUi) -> Unit,
    modifier: Modifier = Modifier,
) {
    val listState = rememberLazyListState()
    var viewerKey by rememberSaveable { mutableStateOf<String?>(null) }
    val contentMode = resolveAsyncContentMode(
        isLoading = state.loading,
        isEmpty = state.messages.isEmpty(),
        hasError = state.error != null,
    )
    val latestMessageId = state.messages.lastOrNull()?.id
    val submitMessage = remember(state.chatId, state.draft, state.sending, onSend) {
        {
            val text = state.draft.trim()
            if (text.isNotEmpty() && !state.sending) {
                onSend(state.chatId, text)
            }
        }
    }

    LaunchedEffect(state.messages.size, latestMessageId, state.loading) {
        if (state.messages.isNotEmpty()) {
            listState.animateScrollToItem(state.messages.lastIndex)
        }
    }

    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text(
                            text = state.chatTitle.ifBlank { "Диалог" },
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                        Text(
                            text = if (state.loading) "Обновление…" else " ",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                },
                navigationIcon = {
                    TextButton(onClick = onBack) {
                        Text("←", style = MaterialTheme.typography.titleLarge)
                    }
                },
            )
        },
        bottomBar = {
            ComposerBar(
                draft = state.draft,
                sending = state.sending,
                onDraftChange = onDraftChange,
                onSend = submitMessage,
            )
        },
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding),
        ) {
            state.error?.takeIf { contentMode != AsyncContentMode.Loading }?.let { message ->
                ErrorRetryState(
                    title = "Не удалось загрузить сообщения",
                    message = message,
                    retryLabel = "Повторить",
                    retryEnabled = !state.loading,
                    onRetry = onRetry,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                )
            }

            when (contentMode) {
                AsyncContentMode.Loading -> LoadingState(
                    message = "Загрузка сообщений…",
                    modifier = Modifier.fillMaxSize(),
                )

                AsyncContentMode.Empty -> EmptyState(
                    title = "Сообщений пока нет",
                    modifier = Modifier.fillMaxSize().padding(24.dp),
                )

                AsyncContentMode.Error -> Unit

                AsyncContentMode.Content -> {
                    LazyColumn(
                        state = listState,
                        modifier = Modifier
                            .fillMaxSize()
                            .weight(1f),
                        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 16.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        items(
                            items = state.messages,
                            key = { message -> "${message.chatId}:${message.id}" },
                        ) { message ->
                            MessageBubble(
                                message = message,
                                onPollVote = onPollVote,
                                onDownloadMedia = { attachmentIndex, retry ->
                                    message.id.toLongOrNull()?.let { messageId ->
                                        onDownloadMedia(messageId, attachmentIndex, retry)
                                    }
                                },
                                onCancelMedia = { attachmentIndex ->
                                    message.id.toLongOrNull()?.let { messageId ->
                                        onCancelMedia(messageId, attachmentIndex)
                                    }
                                },
                                onOpenViewer = { attachment ->
                                    viewerKey = "${message.id}:${attachment.index}"
                                },
                                onOpenDocument = onOpenDocument,
                            )
                        }

                        if (state.loading) {
                            item(key = "history-loading") {
                                Row(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .padding(vertical = 8.dp),
                                    horizontalArrangement = Arrangement.Center,
                                ) {
                                    CircularProgressIndicator(
                                        modifier = Modifier.width(20.dp),
                                        strokeWidth = 2.dp,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    val viewerSelection = remember(state.messages, viewerKey) {
        val key = viewerKey ?: return@remember null
        val separator = key.lastIndexOf(':')
        if (separator <= 0) return@remember null
        val messageId = key.substring(0, separator)
        val attachmentIndex = key.substring(separator + 1).toIntOrNull() ?: return@remember null
        state.messages
            .firstOrNull { it.id == messageId }
            ?.attachments
            ?.firstOrNull { it.index == attachmentIndex }
            ?.let { attachment -> messageId.toLongOrNull()?.let { it to attachment } }
    }

    viewerSelection?.let { (messageId, attachment) ->
        MediaViewerDialog(
            attachment = attachment,
            onDismiss = { viewerKey = null },
            onRetry = {
                onDownloadMedia(messageId, attachment.index, true)
                viewerKey = null
            },
        )
    }
}

@Composable
private fun MessageBubble(
    message: ConversationMessageUi,
    onPollVote: (ConversationPollUi, List<List<Int>>) -> Unit,
    onDownloadMedia: (attachmentIndex: Int, retry: Boolean) -> Unit,
    onCancelMedia: (attachmentIndex: Int) -> Unit,
    onOpenViewer: (MediaAttachmentUi) -> Unit,
    onOpenDocument: (MediaAttachmentUi) -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = if (message.isOutgoing) Arrangement.End else Arrangement.Start,
    ) {
        Surface(
            modifier = Modifier.fillMaxWidth(0.84f),
            shape = RoundedCornerShape(
                topStart = 18.dp,
                topEnd = 18.dp,
                bottomEnd = if (message.isOutgoing) 6.dp else 18.dp,
                bottomStart = if (message.isOutgoing) 18.dp else 6.dp,
            ),
            color = if (message.isOutgoing) {
                MaterialTheme.colorScheme.primaryContainer
            } else {
                MaterialTheme.colorScheme.surfaceVariant
            },
            contentColor = if (message.isOutgoing) {
                MaterialTheme.colorScheme.onPrimaryContainer
            } else {
                MaterialTheme.colorScheme.onSurfaceVariant
            },
        ) {
            Column(modifier = Modifier.padding(horizontal = 14.dp, vertical = 10.dp)) {
                message.attachments.forEach { attachment ->
                    MediaAttachmentCard(
                        attachment = attachment,
                        onDownload = { retry -> onDownloadMedia(attachment.index, retry) },
                        onCancel = { onCancelMedia(attachment.index) },
                        onOpenViewer = { onOpenViewer(attachment) },
                        onOpenDocument = { onOpenDocument(attachment) },
                    )
                    Spacer(Modifier.height(8.dp))
                }

                if (message.text.isNotBlank()) {
                    Text(
                        text = message.text,
                        style = MaterialTheme.typography.bodyLarge,
                    )
                }
                message.poll?.let { poll ->
                    if (message.text.isNotBlank()) {
                        Spacer(Modifier.height(8.dp))
                    }
                    PollContent(
                        poll = poll,
                        loading = message.pollVoting,
                        error = message.pollError,
                        onVote = { optionIds -> onPollVote(poll, optionIds) },
                    )
                }
                if (message.timestamp.isNotBlank()) {
                    Spacer(Modifier.height(5.dp))
                    Text(
                        text = message.timestamp,
                        modifier = Modifier.align(Alignment.End),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

@Composable
private fun PollContent(
    poll: ConversationPollUi,
    loading: Boolean,
    error: String?,
    onVote: (List<List<Int>>) -> Unit,
) {
    val serverChosen = remember(poll.options) {
        poll.options.filter { it.chosen }.map { it.optionId }.toSet()
    }
    var selected by remember(poll.id, serverChosen) { mutableStateOf(serverChosen) }
    val enabled = !poll.closed && !loading
    val resultsVisible = poll.totalVoters != null ||
        poll.options.any { it.voterCount != null || it.chosen || it.correct != null }

    Text(
        text = poll.question,
        style = MaterialTheme.typography.titleMedium,
        fontWeight = FontWeight.SemiBold,
    )
    Spacer(Modifier.height(6.dp))

    poll.options.forEach { option ->
        val checked = if (poll.multipleChoice) {
            option.optionId in selected
        } else {
            option.chosen
        }
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (poll.multipleChoice) {
                Checkbox(
                    checked = checked,
                    enabled = enabled,
                    onCheckedChange = { value ->
                        selected = selected.toMutableSet().apply {
                            if (value) {
                                add(option.optionId)
                            } else {
                                remove(option.optionId)
                            }
                        }
                    },
                )
            } else {
                RadioButton(
                    selected = checked,
                    enabled = enabled,
                    onClick = {
                        if (enabled) onVote(listOf(option.optionId))
                    },
                )
            }

            Column(modifier = Modifier.weight(1f)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = option.text,
                        modifier = Modifier.weight(1f),
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    optionStatus(option)?.let { status ->
                        Text(
                            text = status,
                            style = MaterialTheme.typography.labelMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                    }
                }
                if (resultsVisible && option.voterCount != null) {
                    val percent = pollPercent(option.voterCount, poll.totalVoters)
                    Text(
                        text = buildString {
                            if (percent != null) append("$percent% · ")
                            append(option.voterCount)
                            append(if (option.voterCount == 1) " голос" else " голосов")
                        },
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }

    if (poll.multipleChoice && !poll.closed) {
        Spacer(Modifier.height(4.dp))
        Button(
            onClick = { onVote(selected.toList()) },
            enabled = enabled && selected.isNotEmpty(),
            modifier = Modifier.fillMaxWidth(),
        ) {
            if (loading) {
                CircularProgressIndicator(
                    modifier = Modifier.width(18.dp),
                    strokeWidth = 2.dp,
                )
            } else {
                Text("Отправить голос")
            }
        }
    } else if (loading) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.Center,
        ) {
            CircularProgressIndicator(
                modifier = Modifier.width(18.dp),
                strokeWidth = 2.dp,
            )
        }
    }

    if (poll.totalVoters != null) {
        Spacer(Modifier.height(4.dp))
        Text(
            text = "Всего проголосовало: ${poll.totalVoters}",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    if (poll.closed) {
        Text(
            text = "Опрос закрыт",
            style = MaterialTheme.typography.labelSmall,
            fontWeight = FontWeight.SemiBold,
        )
    }
    poll.explanation?.takeIf { it.isNotBlank() }?.let { explanation ->
        Spacer(Modifier.height(4.dp))
        Text(
            text = explanation,
            style = MaterialTheme.typography.bodySmall,
        )
    }
    error?.takeIf { it.isNotBlank() }?.let { message ->
        Spacer(Modifier.height(4.dp))
        Text(
            text = message,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.error,
        )
    }
}

private fun optionStatus(option: ConversationPollOptionUi): String? = when {
    option.correct == true -> "✓"
    option.chosen && option.correct == false -> "✕"
    option.chosen -> "Выбрано"
    else -> null
}

internal fun pollPercent(voterCount: Int?, totalVoters: Int?): Int? {
    val votes = voterCount ?: return null
    val total = totalVoters ?: return null
    if (votes < 0 || total <= 0) return null
    return ((votes.toDouble() / total.toDouble()) * 100.0)
        .roundToInt()
        .coerceIn(0, 100)
}

@Composable
private fun MediaAttachmentCard(
    attachment: MediaAttachmentUi,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
    onOpenViewer: () -> Unit,
    onOpenDocument: () -> Unit,
) {
    when (attachment.kind) {
        MediaKindUi.Photo -> PhotoAttachmentCard(
            attachment = attachment,
            onDownload = onDownload,
            onCancel = onCancel,
            onOpen = onOpenViewer,
        )

        MediaKindUi.Video,
        MediaKindUi.VideoNote,
        -> VideoAttachmentCard(
            attachment = attachment,
            onDownload = onDownload,
            onCancel = onCancel,
            onOpen = onOpenViewer,
        )

        else -> DocumentAttachmentCard(
            attachment = attachment,
            onDownload = onDownload,
            onCancel = onCancel,
            onOpen = onOpenDocument,
        )
    }
}

@Composable
private fun PhotoAttachmentCard(
    attachment: MediaAttachmentUi,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
    onOpen: () -> Unit,
) {
    val file = attachment.downloadedFile
    var decodeFailed by remember(file?.absolutePath) { mutableStateOf(false) }
    val ratio = attachment.aspectRatio?.coerceIn(0.25f, 4f) ?: (4f / 3f)

    Box(
        modifier = Modifier
            .fillMaxWidth()
            .aspectRatio(ratio)
            .heightIn(min = 120.dp, max = 360.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(MaterialTheme.colorScheme.surface.copy(alpha = 0.35f))
            .clickable(enabled = file != null && !decodeFailed, onClick = onOpen),
        contentAlignment = Alignment.Center,
    ) {
        if (file != null && !decodeFailed) {
            AsyncImage(
                model = file,
                contentDescription = "Фото",
                contentScale = ContentScale.Crop,
                onSuccess = { decodeFailed = false },
                onError = { decodeFailed = true },
                modifier = Modifier
                    .fillMaxSize()
                    .background(MaterialTheme.colorScheme.surfaceVariant),
            )
        } else {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                Text(if (decodeFailed) "Изображение повреждено" else "Фото")
            }
        }

        MediaTransferOverlay(
            attachment = attachment,
            localFileMissing = attachment.downloadState is MediaDownloadUiState.Downloaded && file == null,
            localDecodeFailed = decodeFailed,
            onDownload = onDownload,
            onCancel = onCancel,
        )
    }
}

@Composable
private fun VideoAttachmentCard(
    attachment: MediaAttachmentUi,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
    onOpen: () -> Unit,
) {
    val file = attachment.downloadedFile
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(180.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(Color.Black.copy(alpha = 0.72f))
            .clickable(enabled = file != null, onClick = onOpen),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text(
                text = if (file != null) "▶" else "Видео",
                style = MaterialTheme.typography.headlineMedium,
                color = Color.White,
            )
            formatMediaDuration(attachment.durationMs)?.let {
                Text(
                    text = it,
                    style = MaterialTheme.typography.labelMedium,
                    color = Color.White,
                )
            }
        }

        MediaTransferOverlay(
            attachment = attachment,
            localFileMissing = attachment.downloadState is MediaDownloadUiState.Downloaded && file == null,
            onDownload = onDownload,
            onCancel = onCancel,
        )
    }
}

@Composable
private fun DocumentAttachmentCard(
    attachment: MediaAttachmentUi,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
    onOpen: () -> Unit,
) {
    val file = attachment.downloadedFile
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = file != null, onClick = onOpen),
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.surface.copy(alpha = 0.38f),
    ) {
        Column(
            modifier = Modifier.padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text(
                text = sanitizeDisplayFilename(attachment.filename),
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.SemiBold,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            val details = listOfNotNull(
                formatMediaSize(attachment.sizeBytes),
                attachment.mimeType?.takeIf { it.isNotBlank() },
                formatMediaDuration(attachment.durationMs),
            ).joinToString(" • ")
            if (details.isNotBlank()) {
                Text(
                    text = details,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (file != null) {
                Text(
                    text = "Нажмите, чтобы открыть",
                    style = MaterialTheme.typography.labelMedium,
                )
            }
            MediaTransferControls(
                attachment = attachment,
                localFileMissing = attachment.downloadState is MediaDownloadUiState.Downloaded && file == null,
                onDownload = onDownload,
                onCancel = onCancel,
            )
        }
    }
}

@Composable
private fun MediaTransferOverlay(
    attachment: MediaAttachmentUi,
    localFileMissing: Boolean,
    localDecodeFailed: Boolean = false,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
) {
    val showOverlay = attachment.downloadState !is MediaDownloadUiState.Downloaded ||
        localFileMissing ||
        localDecodeFailed
    if (!showOverlay) return

    Surface(
        modifier = Modifier.fillMaxSize(),
        color = Color.Black.copy(alpha = 0.56f),
    ) {
        Box(contentAlignment = Alignment.Center) {
            MediaTransferControls(
                attachment = attachment,
                localFileMissing = localFileMissing,
                localDecodeFailed = localDecodeFailed,
                onDownload = onDownload,
                onCancel = onCancel,
                lightContent = true,
            )
        }
    }
}

@Composable
private fun MediaTransferControls(
    attachment: MediaAttachmentUi,
    localFileMissing: Boolean,
    localDecodeFailed: Boolean = false,
    onDownload: (retry: Boolean) -> Unit,
    onCancel: () -> Unit,
    lightContent: Boolean = false,
) {
    val textColor = if (lightContent) Color.White else MaterialTheme.colorScheme.onSurface
    Column(
        modifier = Modifier.padding(10.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        when (val download = attachment.downloadState) {
            MediaDownloadUiState.NotDownloaded -> {
                Button(onClick = { onDownload(false) }) {
                    Text("Скачать")
                }
            }

            MediaDownloadUiState.Queued -> {
                CircularProgressIndicator(modifier = Modifier.width(28.dp))
                Text("В очереди", color = textColor)
                TextButton(onClick = onCancel) { Text("Отмена") }
            }

            is MediaDownloadUiState.Downloading -> {
                val total = download.totalBytes
                val fraction = total
                    ?.takeIf { it > 0L }
                    ?.let { (download.bytesTransferred.toDouble() / it).coerceIn(0.0, 1.0).toFloat() }
                if (fraction != null) {
                    LinearProgressIndicator(
                        progress = { fraction },
                        modifier = Modifier.fillMaxWidth(0.72f),
                    )
                    Text("${(fraction * 100).toInt()}%", color = textColor)
                } else {
                    CircularProgressIndicator(modifier = Modifier.width(28.dp))
                    Text(
                        formatMediaSize(download.bytesTransferred) ?: "Загрузка…",
                        color = textColor,
                    )
                }
                TextButton(onClick = onCancel) { Text("Отмена") }
            }

            is MediaDownloadUiState.Failed -> {
                Text(download.reason ?: "Не удалось скачать", color = textColor)
                Button(onClick = { onDownload(true) }) {
                    Text("Повторить")
                }
            }

            MediaDownloadUiState.Cancelled -> {
                Text("Загрузка отменена", color = textColor)
                Button(onClick = { onDownload(true) }) {
                    Text("Повторить")
                }
            }

            is MediaDownloadUiState.Downloaded -> {
                if (localFileMissing || localDecodeFailed) {
                    Text(
                        if (localDecodeFailed) "Файл не удалось прочитать" else "Файл удалён из кэша",
                        color = textColor,
                    )
                    Button(onClick = { onDownload(localDecodeFailed) }) {
                        Text("Скачать заново")
                    }
                }
            }
        }
    }
}

@Composable
private fun ComposerBar(
    draft: String,
    sending: Boolean,
    onDraftChange: (String) -> Unit,
    onSend: () -> Unit,
) {
    val canSend = draft.isNotBlank() && !sending

    Surface(
        tonalElevation = 2.dp,
        modifier = Modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .imePadding(),
    ) {
        Column {
            HorizontalDivider()
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 12.dp, vertical = 8.dp),
                verticalAlignment = Alignment.Bottom,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                OutlinedTextField(
                    value = draft,
                    onValueChange = onDraftChange,
                    modifier = Modifier.weight(1f),
                    placeholder = { Text("Сообщение") },
                    enabled = !sending,
                    maxLines = 5,
                    keyboardOptions = KeyboardOptions(
                        keyboardType = KeyboardType.Text,
                        imeAction = ImeAction.Send,
                    ),
                    keyboardActions = KeyboardActions(
                        onSend = { if (canSend) onSend() },
                    ),
                )
                Button(
                    onClick = onSend,
                    enabled = canSend,
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 12.dp),
                ) {
                    Text(if (sending) "…" else "Отправить")
                }
            }
        }
    }
}
