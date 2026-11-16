package com.chitik.telegram

import androidx.compose.foundation.layout.Column
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
        title = { Text(title) },
        text = {
            Column {
                if (loading && profile == null) Text("Loading profile…")
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                profile?.let {
                    it.avatarLocalPath?.let { path ->
                        AsyncImage(
                            model = File(path),
                            contentDescription = "Profile photo",
                            modifier = Modifier.size(80.dp).clip(CircleShape).align(Alignment.CenterHorizontally),
                        )
                    }
                    Text("Type: ${it.kind}")
                    it.username?.let { value -> Text("@${value}") }
                    it.bio?.takeIf(String::isNotBlank)?.let { value ->
                        Spacer(Modifier.height(8.dp))
                        Text(value)
                    }
                    it.memberCount?.let { value -> Text("Members: ${value}") }
                    it.presence?.let { value -> Text("Presence: ${value}") }
                    it.isBot?.let { value -> Text(if (value) "Bot account" else "User account") }
                    if (it.isVerified == true) Text("Verified")
                    if (it.isPremium == true) Text("Premium")
                    val permissions = listOfNotNull(
                        it.canSendMessages?.let { allowed -> "Send messages: ${if (allowed) "yes" else "no"}" },
                        it.canInviteUsers?.let { allowed -> "Invite users: ${if (allowed) "yes" else "no"}" },
                        it.canChangeInfo?.let { allowed -> "Change info: ${if (allowed) "yes" else "no"}" },
                        it.canPostMessages?.let { allowed -> "Post messages: ${if (allowed) "yes" else "no"}" },
                    )
                    if (permissions.isNotEmpty()) {
                        Spacer(Modifier.height(8.dp))
                        permissions.forEach { Text(it) }
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Close") } },
    )
}
