package com.chitik.telegram

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.Alignment
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.draw.clip
import coil3.compose.AsyncImage
import java.io.File

data class PeerProfileUi(
    val peerId: Long,
    val kind: String,
    val displayName: String?,
    val firstName: String?,
    val lastName: String?,
    val username: String?,
    val bio: String?,
    val memberCount: Int?,
    val presence: String?,
    val isBot: Boolean?,
    val isVerified: Boolean?,
    val isPremium: Boolean?,
    val canSendMessages: Boolean?,
    val canInviteUsers: Boolean?,
    val canChangeInfo: Boolean?,
    val canPostMessages: Boolean?,
    val avatarFileId: String?,
    val avatarLocalPath: String?,
)

@Composable
fun PeerProfileDialog(
    profile: PeerProfileUi?,
    loading: Boolean,
    error: String?,
    onDismiss: () -> Unit,
) {
    val title = profile?.displayName?.takeIf(String::isNotBlank) ?: "Profile"
    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Text(
                text = "Info",
                style = MaterialTheme.typography.titleLarge,
                fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold,
            )
        },
        text = {
            Column(
                modifier = Modifier.fillMaxWidth().verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                if (loading && profile == null) {
                    CircularProgressIndicator(modifier = Modifier.align(Alignment.CenterHorizontally))
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                profile?.let { peer ->
                    if (!peer.avatarLocalPath.isNullOrBlank()) {
                        AsyncImage(
                            model = File(peer.avatarLocalPath),
                            contentDescription = "Profile photo",
                            modifier = Modifier.size(92.dp).clip(CircleShape).align(Alignment.CenterHorizontally),
                        )
                    } else {
                        AvatarPlaceholder(
                            displayName = title,
                            size = 92.dp,
                            modifier = Modifier.align(Alignment.CenterHorizontally),
                        )
                    }
                    Text(
                        text = title,
                        style = MaterialTheme.typography.titleLarge,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    peer.presence?.takeIf(String::isNotBlank)?.let { presence ->
                        Text(
                            presence,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.primary,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    peer.username?.takeIf(String::isNotBlank)?.let {
                        ProfileInfoRow(label = "Username", value = "@$it")
                    }
                    peer.bio?.takeIf(String::isNotBlank)?.let {
                        ProfileInfoRow(label = "Bio", value = it)
                    }
                    peer.memberCount?.let {
                        ProfileInfoRow(label = "Members", value = it.toString())
                    }
                    if (peer.isVerified == true) ProfileInfoRow("Account", "Verified")
                    if (peer.isBot == true) ProfileInfoRow("Type", "Bot")
                    if (peer.isPremium == true) ProfileInfoRow("Telegram", "Premium")
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Close") } },
    )
}

@Composable
private fun ProfileInfoRow(label: String, value: String) {
    Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
        Text(value, style = MaterialTheme.typography.bodyLarge)
        Text(label, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
