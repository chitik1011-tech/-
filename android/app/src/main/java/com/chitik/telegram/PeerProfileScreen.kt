package com.chitik.telegram

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.widget.Toast
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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
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
    val members: List<ChatMemberUi>?,
    val membersTruncated: Boolean,
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

data class ChatMemberUi(
    val userId: Long,
    val displayName: String,
    val username: String?,
    val role: String,
)

@Composable
fun PeerProfileDialog(
    profile: PeerProfileUi?,
    loading: Boolean,
    error: String?,
    onDismiss: () -> Unit,
    onLeaveChat: (Long) -> Unit = {},
    /** Returns to the already-open conversation so the user can message this peer. */
    onMessage: () -> Unit = onDismiss,
) {
    val presentation = profile?.toPeerProfilePresentation()
    val context = LocalContext.current
    var confirmLeave by remember(profile?.peerId) { mutableStateOf(false) }
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
                    details.usernameActions?.let { actions ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            TextButton(onClick = { copyProfileUsername(context, actions.copyText) }) {
                                Text("Copy username")
                            }
                            TextButton(onClick = {
                                if (!openProfileUsername(context, actions.uri)) {
                                    Toast.makeText(
                                        context,
                                        "No app can open Telegram links",
                                        Toast.LENGTH_SHORT,
                                    ).show()
                                }
                            }) {
                                Text("Open in Telegram")
                            }
                        }
                    }
                    details.bio?.let { ProfileInfoRow(label = "Bio", value = it) }
                    details.memberCount?.let { ProfileInfoRow(label = "Members", value = it.toString()) }
                    if (profile.kind in setOf("basic_group", "supergroup", "channel")) {
                        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                        Text("Group members", style = MaterialTheme.typography.labelLarge)
                        val members = profile.members
                        if (members == null) {
                            Text(
                                "Member list unavailable from Telegram",
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                style = MaterialTheme.typography.bodySmall,
                            )
                        } else if (members.isEmpty()) {
                            Text("No members returned")
                        } else {
                            members.forEach { member ->
                                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                    Text(
                                        member.displayName.ifBlank { "User ${member.userId}" },
                                        style = MaterialTheme.typography.bodyMedium,
                                    )
                                    val details = buildList {
                                        member.username?.takeIf(String::isNotBlank)?.let { add("@$it") }
                                        add(member.role.replaceFirstChar(Char::uppercase))
                                    }.joinToString(" · ")
                                    Text(
                                        details,
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                            if (profile.membersTruncated) {
                                Text(
                                    "Showing the first ${members.size} members",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
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
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (profile?.kind in setOf("basic_group", "supergroup", "channel")) {
                    TextButton(onClick = { confirmLeave = true }) { Text("Leave chat") }
                }
                if (presentation?.canMessage == true) {
                    TextButton(onClick = onDismiss) { Text("Close") }
                }
            }
        },
    )
    if (confirmLeave && profile != null) {
        AlertDialog(
            onDismissRequest = { confirmLeave = false },
            title = { Text("Leave chat?") },
            text = {
                Text("Leaving removes this chat from your dialogs and clears its cached conversation history.")
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmLeave = false
                    onLeaveChat(profile.peerId)
                }) { Text("Leave") }
            },
            dismissButton = {
                TextButton(onClick = { confirmLeave = false }) { Text("Cancel") }
            },
        )
    }
}

private fun copyProfileUsername(context: Context, username: String) {
    val clipboard = context.getSystemService(ClipboardManager::class.java) ?: return
    clipboard.setPrimaryClip(ClipData.newPlainText("Telegram username", username))
    Toast.makeText(context, "Username copied", Toast.LENGTH_SHORT).show()
}

private fun openProfileUsername(context: Context, uri: String): Boolean = try {
    context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(uri)))
    true
} catch (_: ActivityNotFoundException) {
    false
} catch (_: SecurityException) {
    false
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
