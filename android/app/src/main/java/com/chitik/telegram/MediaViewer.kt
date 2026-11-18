package com.chitik.telegram

import android.net.Uri
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.PlayerView
import coil3.compose.AsyncImage
import java.io.File
import kotlin.math.abs

@Composable
fun MediaViewerDialog(
    attachment: MediaAttachmentUi,
    onDismiss: () -> Unit,
    onRetry: () -> Unit,
    itemPosition: Int = 1,
    itemCount: Int = 1,
    onPrevious: (() -> Unit)? = null,
    onNext: (() -> Unit)? = null,
) {
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(
            dismissOnBackPress = true,
            dismissOnClickOutside = false,
            usePlatformDefaultWidth = false,
            decorFitsSystemWindows = false,
        ),
    ) {
        Surface(
            modifier = Modifier.fillMaxSize(),
            color = Color.Black,
            contentColor = Color.White,
        ) {
            Box(modifier = Modifier.fillMaxSize()) {
                when (attachment.kind) {
                    MediaKindUi.Photo,
                    MediaKindUi.Document,
                    -> if (isInlineImageMedia(attachment)) PhotoViewer(
                        file = attachment.downloadedFile,
                        onDismiss = onDismiss,
                        onRetry = onRetry,
                    ) else ViewerUnavailable(onRetry)

                    MediaKindUi.Video,
                    MediaKindUi.VideoNote,
                    -> VideoViewer(
                        file = attachment.downloadedFile,
                        onRetry = onRetry,
                    )

                    else -> ViewerUnavailable(onRetry = onRetry)
                }

                TextButton(
                    onClick = onDismiss,
                    modifier = Modifier
                        .align(Alignment.TopEnd)
                        .padding(12.dp),
                ) {
                    Text("Закрыть", color = Color.White)
                }

                if (itemCount > 1) {
                    Row(
                        modifier = Modifier
                            .align(Alignment.BottomCenter)
                            .padding(bottom = 20.dp),
                        horizontalArrangement = Arrangement.spacedBy(16.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        TextButton(
                            onClick = { onPrevious?.invoke() },
                            enabled = onPrevious != null,
                            colors = ButtonDefaults.textButtonColors(contentColor = Color.White),
                        ) {
                            Text("Предыдущее")
                        }
                        Text("$itemPosition / $itemCount", color = Color.White)
                        TextButton(
                            onClick = { onNext?.invoke() },
                            enabled = onNext != null,
                            colors = ButtonDefaults.textButtonColors(contentColor = Color.White),
                        ) {
                            Text("Следующее")
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun PhotoViewer(
    file: File?,
    onDismiss: () -> Unit,
    onRetry: () -> Unit,
) {
    if (file == null) {
        MissingMedia(onRetry)
        return
    }

    var scale by remember(file.absolutePath) { mutableFloatStateOf(1f) }
    var translation by remember(file.absolutePath) { mutableStateOf(Offset.Zero) }
    var verticalDrag by remember(file.absolutePath) { mutableFloatStateOf(0f) }
    var decodeFailed by remember(file.absolutePath) { mutableStateOf(false) }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black),
        contentAlignment = Alignment.Center,
    ) {
        AsyncImage(
            model = file,
            contentDescription = "Фото",
            contentScale = ContentScale.Fit,
            onSuccess = { decodeFailed = false },
            onError = { decodeFailed = true },
            modifier = Modifier
                .fillMaxSize()
                .graphicsLayer {
                    scaleX = scale
                    scaleY = scale
                    translationX = translation.x
                    translationY = translation.y
                }
                .pointerInput(file.absolutePath) {
                    detectTapGestures(
                        onDoubleTap = {
                            if (scale > 1.05f) {
                                scale = 1f
                                translation = Offset.Zero
                            } else {
                                scale = 2.5f
                            }
                        },
                    )
                }
                .pointerInput(file.absolutePath) {
                    detectTransformGestures { _, pan, zoom, _ ->
                        val nextScale = (scale * zoom).coerceIn(1f, 5f)
                        scale = nextScale
                        translation = if (nextScale <= 1.01f) {
                            Offset.Zero
                        } else {
                            translation + pan
                        }
                    }
                }
                .pointerInput(file.absolutePath, scale) {
                    detectVerticalDragGestures(
                        onVerticalDrag = { _, amount ->
                            if (scale <= 1.01f) verticalDrag += amount
                        },
                        onDragCancel = { verticalDrag = 0f },
                        onDragEnd = {
                            if (scale <= 1.01f && abs(verticalDrag) > 160f) {
                                onDismiss()
                            }
                            verticalDrag = 0f
                        },
                    )
                },
        )

        if (decodeFailed) {
            ViewerError(
                title = "Не удалось открыть изображение",
                onRetry = onRetry,
            )
        }
    }
}

@Composable
private fun VideoViewer(
    file: File?,
    onRetry: () -> Unit,
) {
    if (file == null) {
        MissingMedia(onRetry)
        return
    }

    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    var playbackError by remember(file.absolutePath) { mutableStateOf<String?>(null) }
    val player = remember(file.absolutePath) {
        ExoPlayer.Builder(context).build().apply {
            setMediaItem(MediaItem.fromUri(Uri.fromFile(file)))
            prepare()
        }
    }

    DisposableEffect(player, lifecycleOwner) {
        var resumeAfterForeground = false
        val listener = object : Player.Listener {
            override fun onPlayerError(error: PlaybackException) {
                playbackError = "Не удалось воспроизвести видео"
            }
        }
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_STOP -> {
                    resumeAfterForeground = player.playWhenReady
                    player.pause()
                }

                Lifecycle.Event.ON_START -> {
                    if (resumeAfterForeground) {
                        player.play()
                        resumeAfterForeground = false
                    }
                }

                else -> Unit
            }
        }
        player.addListener(listener)
        lifecycleOwner.lifecycle.addObserver(observer)

        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
            player.removeListener(listener)
            player.release()
        }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black),
        contentAlignment = Alignment.Center,
    ) {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { viewContext ->
                PlayerView(viewContext).apply {
                    useController = true
                    this.player = player
                }
            },
            update = { view ->
                view.player = player
            },
        )

        playbackError?.let {
            ViewerError(
                title = it,
                onRetry = onRetry,
            )
        }
    }
}

@Composable
private fun MissingMedia(onRetry: () -> Unit) {
    ViewerError(
        title = "Файл больше недоступен в кэше",
        onRetry = onRetry,
    )
}

@Composable
private fun ViewerUnavailable(onRetry: () -> Unit) {
    ViewerError(
        title = "Предпросмотр этого типа файла недоступен",
        onRetry = onRetry,
    )
}

@Composable
private fun ViewerError(
    title: String,
    onRetry: () -> Unit,
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black.copy(alpha = 0.72f)),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(12.dp),
            modifier = Modifier.padding(24.dp),
        ) {
            Text(
                text = title,
                style = MaterialTheme.typography.bodyLarge,
                color = Color.White,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = onRetry) {
                    Text("Скачать заново")
                }
            }
        }
    }
}
