package com.chitik.telegram

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
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
    /** Returns to the already-open conversation so the user can message this peer. */
    onMessage: () -> Unit = onDismiss,
) {
    val presentation = profile?.toPeerProfilePresentation()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            if (profile != null && presentation != null) {
                PeerProfileHeader(profile, presentation)
            } else {
                Text("Profile", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
            }
        },
        text = {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                if (loading && profile == null) {
                    CircularProgressIndicator(modifier = Modifier.align(Alignment.CenterHorizontally))
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                presentation?.let { details ->
                    details.username?.let { ProfileInfoRow(label = "Username", value = "@$it") }
                    details.bio?.let { ProfileInfoRow(label = "Bio", value = it) }
                    details.memberCount?.let { ProfileInfoRow(label = "Members", value = it.toString()) }
                    if (details.badges.isNotEmpty()) {
                        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            details.badges.forEach { label -> ProfileBadge(label) }
                        }
                    }
                    if (details.permissions.isNotEmpty()) {
                        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                        Text(
                            "Available actions",
                            style = MaterialTheme.typography.labelLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        details.permissions.forEach { permission ->
                            ProfileInfoRow(label = permission.label, value = permission.value)
                        }
                    }
                }
            }
        },
        confirmButton = {
            if (presentation?.canMessage == true) {
                TextButton(onClick = onMessage) { Text("Message") }
            } else {
                TextButton(onClick = onDismiss) { Text("Close") }
            }
        },
        dismissButton = {
            if (presentation?.canMessage == true) {
                TextButton(onClick = onDismiss) { Text("Close") }
            }
        },
    )
}

@Composable
private fun PeerProfileHeader(profile: PeerProfileUi, presentation: PeerProfilePresentation) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (!profile.avatarLocalPath.isNullOrBlank()) {
            AsyncImage(
                model = File(profile.avatarLocalPath),
                contentDescription = "Profile photo",
                modifier = Modifier.size(76.dp).clip(CircleShape),
            )
        } else {
            AvatarPlaceholder(displayName = presentation.title, size = 76.dp)
        }
        Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(
                text = presentation.title,
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = presentation.kindLabel,
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            presentation.presence?.let { status ->
                Text(
                    text = status,
                    style = MaterialTheme.typography.bodySmall,
                    color = if (status == "Online") MaterialTheme.colorScheme.primary
                    else MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun ProfileBadge(label: String) {
    Surface(
        color = MaterialTheme.colorScheme.secondaryContainer,
        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
        shape = CircleShape,
    ) {
        Text(
            text = label,
            modifier = Modifier.padding(horizontal = 9.dp, vertical = 4.dp),
            style = MaterialTheme.typography.labelSmall,
        )
    }
}

@Composable
private fun ProfileInfoRow(label: String, value: String) {
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(value, style = MaterialTheme.typography.bodyMedium)
        Text(label, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
